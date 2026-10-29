//! A real terminal per session, inside the app: ConPTY on Windows through alacritty's tty layer,
//! alacritty's VT state machine behind a cell grid with scrollback, and PowerShell in it. Full
//! screen programs (Claude Code, PSReadLine, vim) work because the shell sees a true console.
//! Rendering and key translation live in `ui.rs`; this module owns the process, the grid and
//! the pure helpers (palette, key encoding) so they stay testable.

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode, test::TermSize};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};
use bevy::prelude::*;
use bevy_egui::egui;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const SCROLLBACK: usize = 10_000;

/// What the terminal reports back to the app from its reader thread.
#[derive(Clone, Default)]
struct Proxy {
    title: Arc<Mutex<Option<String>>>,
    dead: Arc<AtomicBool>,
    bell: Arc<AtomicBool>,
    clipboard: Arc<Mutex<Option<String>>>,
    sender: Arc<Mutex<Option<EventLoopSender>>>,
}

impl EventListener for Proxy {
    fn send_event(&self, event: Event) {
        match event {
            Event::Title(t) => {
                if let Ok(mut s) = self.title.lock() {
                    *s = Some(t);
                }
            }
            Event::ResetTitle => {
                if let Ok(mut s) = self.title.lock() {
                    *s = None;
                }
            }
            Event::ChildExit(_) | Event::Exit => self.dead.store(true, Ordering::Relaxed),
            Event::Bell => self.bell.store(true, Ordering::Relaxed),
            Event::ClipboardStore(_, text) => {
                if let Ok(mut c) = self.clipboard.lock() {
                    *c = Some(text);
                }
            }
            Event::PtyWrite(text) => {
                if let Ok(s) = self.sender.lock()
                    && let Some(s) = s.as_ref()
                {
                    let _ = s.send(Msg::Input(text.into_bytes().into()));
                }
            }
            _ => {}
        }
    }
}

/// One cell ready to draw: the glyph and resolved colours.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Px {
    pub c: char,
    pub fg: [u8; 3],
    /// `None` is the terminal background: nothing to paint behind the glyph.
    pub bg: Option<[u8; 3]>,
    pub bold: bool,
    pub dim: bool,
    pub underline: bool,
    /// The second half of a wide glyph: draw nothing here.
    pub spacer: bool,
    /// A double-width glyph: the next cell is its spacer.
    pub wide: bool,
}

/// One frame of the visible grid.
#[derive(Debug, Clone, Default)]
pub struct Frame {
    pub cells: Vec<Vec<Px>>,
    /// (row, column, shape) when the cursor is visible.
    pub cursor: Option<(usize, usize, CursorShape)>,
    /// Lines scrolled up from the bottom.
    pub display_offset: usize,
    /// Lines of history above the screen.
    pub history: usize,
    pub app_cursor: bool,
}

pub struct PtyTab {
    term: Arc<FairMutex<Term<Proxy>>>,
    sender: EventLoopSender,
    proxy: Proxy,
    pub label: String,
    pub cwd: String,
    cols: u16,
    rows: u16,
    /// The console's startup clear pushes a blank page into the history; it is dropped once.
    history_trimmed: bool,
}

impl PtyTab {
    /// Start PowerShell in `cwd` behind a pseudo console of `cols` x `rows`.
    pub fn spawn(cwd: &str, cols: u16, rows: u16) -> std::io::Result<PtyTab> {
        Self::spawn_with("powershell.exe", &["-NoLogo"], cwd, cols, rows)
    }

    pub fn spawn_with(program: &str, args: &[&str], cwd: &str, cols: u16, rows: u16) -> std::io::Result<PtyTab> {
        let size = WindowSize {
            num_lines: rows,
            num_cols: cols,
            cell_width: 8,
            cell_height: 16,
        };
        let mut env = HashMap::new();
        env.insert("COLORTERM".to_string(), "truecolor".to_string());
        env.insert("TERM_PROGRAM".to_string(), "agent-globe".to_string());
        let opts = tty::Options {
            shell: Some(tty::Shell::new(
                program.into(),
                args.iter().map(|a| a.to_string()).collect(),
            )),
            working_directory: if cwd.is_empty() || !std::path::Path::new(cwd).is_dir() {
                None
            } else {
                Some(PathBuf::from(cwd))
            },
            drain_on_exit: false,
            env,
            escape_args: false,
        };
        // A process started as its own process group (how shells and cargo launch us) carries an
        // "ignore Ctrl+C" flag that every child inherits, console children included. Clear it
        // here, in our own process, so the shell and whatever it runs can be interrupted.
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::System::Console::SetConsoleCtrlHandler(None, 0);
        }
        let pty = tty::new(&opts, size, 0)?;
        let proxy = Proxy::default();
        let config = Config {
            scrolling_history: SCROLLBACK,
            ..Config::default()
        };
        let term = Term::new(config, &TermSize::new(cols as usize, rows as usize), proxy.clone());
        let term = Arc::new(FairMutex::new(term));
        let event_loop = EventLoop::new(term.clone(), proxy.clone(), pty, false, false)?;
        let sender = event_loop.channel();
        if let Ok(mut s) = proxy.sender.lock() {
            *s = Some(sender.clone());
        }
        event_loop.spawn();
        Ok(PtyTab {
            term,
            sender,
            proxy,
            label: String::new(),
            cwd: cwd.to_string(),
            cols,
            rows,
            history_trimmed: false,
        })
    }

    pub fn write(&self, bytes: impl Into<Vec<u8>>) {
        let v: Vec<u8> = bytes.into();
        let _ = self.sender.send(Msg::Input(v.into()));
    }

    /// Paste: wrapped in the bracketed-paste markers when the program asked for them.
    pub fn paste(&self, text: &str) {
        let bracketed = self.term.lock().mode().contains(TermMode::BRACKETED_PASTE);
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        if bracketed {
            self.write(format!("\x1b[200~{text}\x1b[201~").into_bytes());
        } else {
            self.write(text.into_bytes());
        }
    }

    /// Resize both the grid and the console when the card changed shape.
    pub fn ensure_size(&mut self, cols: u16, rows: u16) {
        if (cols, rows) == (self.cols, self.rows) || cols < 2 || rows < 1 {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        self.term.lock().resize(TermSize::new(cols as usize, rows as usize));
        let _ = self.sender.send(Msg::Resize(WindowSize {
            num_lines: rows,
            num_cols: cols,
            cell_width: 8,
            cell_height: 16,
        }));
    }

    pub fn scroll(&self, lines: i32) {
        if lines != 0 {
            self.term.lock().scroll_display(Scroll::Delta(lines));
        }
    }

    pub fn scroll_bottom(&self) {
        self.term.lock().scroll_display(Scroll::Bottom);
    }

    pub fn is_dead(&self) -> bool {
        self.proxy.dead.load(Ordering::Relaxed)
    }

    /// True once, when the program rang the bell since the last ask.
    pub fn take_bell(&self) -> bool {
        self.proxy.bell.swap(false, Ordering::Relaxed)
    }

    pub fn take_clipboard(&self) -> Option<String> {
        self.proxy.clipboard.lock().ok().and_then(|mut c| c.take())
    }

    /// What the program called its window, unless that is just its own exe path (PowerShell's
    /// default), which says nothing.
    pub fn title(&self) -> Option<String> {
        self.proxy
            .title
            .lock()
            .ok()
            .and_then(|t| t.clone())
            .filter(|t| !t.to_ascii_lowercase().ends_with(".exe") && !t.trim().is_empty())
    }

    /// Every line, scrollback first, with trailing blanks trimmed.
    pub fn all_lines(&self) -> Vec<String> {
        let term = self.term.lock();
        let grid = term.grid();
        let cols = grid.columns();
        let mut out = Vec::new();
        let top = grid.topmost_line().0;
        let bottom = grid.bottommost_line().0;
        for l in top..=bottom {
            let row = &grid[Line(l)];
            let s: String = (0..cols.min(row.len())).map(|c| row[Column(c)].c).collect();
            out.push(s.trim_end().to_string());
        }
        while out.last().is_some_and(|l| l.is_empty()) {
            out.pop();
        }
        out
    }

    /// The visible grid, colours resolved, ready to paint.
    pub fn frame(&mut self) -> Frame {
        let mut term = self.term.lock();
        // Until something real has scrolled off, a history made only of blank lines is noise:
        // the wheel would scroll the prompt away into nothing.
        if !self.history_trimmed {
            let grid = term.grid();
            let hs = grid.history_size();
            if hs > 0 {
                let cols = grid.columns();
                let blank = (grid.topmost_line().0..0).all(|l| {
                    let row = &grid[Line(l)];
                    (0..cols.min(row.len())).all(|c| row[Column(c)].c == ' ')
                });
                if blank {
                    term.grid_mut().clear_history();
                } else {
                    self.history_trimmed = true;
                }
            }
        }
        let term = &*term;
        let content = term.renderable_content();
        let cols = term.grid().columns();
        let rows = term.grid().screen_lines();
        let colors = content.colors;
        let mut cells = vec![vec![BLANK; cols]; rows];
        for cell in content.display_iter {
            let r = cell.point.line.0 as usize;
            let c = cell.point.column.0;
            if r >= rows || c >= cols {
                continue;
            }
            let flags = cell.flags;
            let inverse = flags.contains(Flags::INVERSE);
            let mut fg = resolve(cell.fg, colors);
            let mut bg = resolve(cell.bg, colors);
            if inverse {
                std::mem::swap(&mut fg, &mut bg);
            }
            let bg_is_default = !inverse && matches!(cell.bg, Color::Named(NamedColor::Background));
            cells[r][c] = Px {
                c: if flags.contains(Flags::HIDDEN) { ' ' } else { cell.c },
                fg,
                bg: if bg_is_default { None } else { Some(bg) },
                bold: flags.intersects(Flags::BOLD),
                dim: flags.intersects(Flags::DIM),
                underline: flags.intersects(Flags::ALL_UNDERLINES),
                spacer: flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
                wide: flags.contains(Flags::WIDE_CHAR),
            };
        }
        let cursor = if content.mode.contains(TermMode::SHOW_CURSOR)
            && content.cursor.shape != CursorShape::Hidden
            && content.display_offset == 0
        {
            Some((
                content.cursor.point.line.0.max(0) as usize,
                content.cursor.point.column.0,
                content.cursor.shape,
            ))
        } else {
            None
        };
        Frame {
            cells,
            cursor,
            display_offset: content.display_offset,
            history: term.grid().history_size(),
            app_cursor: content.mode.contains(TermMode::APP_CURSOR),
        }
    }
}

impl Drop for PtyTab {
    fn drop(&mut self) {
        let _ = self.sender.send(Msg::Shutdown);
    }
}

const BLANK: Px = Px {
    c: ' ',
    fg: FG,
    bg: None,
    bold: false,
    dim: false,
    underline: false,
    spacer: false,
    wide: false,
};

/// Default foreground and background: the console's phosphor on ink.
pub const FG: [u8; 3] = [190, 228, 180];
pub const BG: [u8; 3] = [6, 12, 5];

/// The 16 ANSI colours, tuned to sit on the ink background.
pub const ANSI: [[u8; 3]; 16] = [
    [40, 46, 40],
    [225, 85, 85],
    [120, 220, 100],
    [230, 200, 90],
    [100, 150, 235],
    [205, 115, 225],
    [95, 215, 215],
    [200, 205, 195],
    [110, 120, 110],
    [255, 120, 120],
    [160, 255, 140],
    [255, 230, 120],
    [140, 185, 255],
    [235, 150, 255],
    [130, 245, 245],
    [240, 245, 235],
];

/// xterm's 256-colour table: 16 ANSI, a 6x6x6 cube, 24 greys.
pub fn indexed(i: u8) -> [u8; 3] {
    match i {
        0..=15 => ANSI[i as usize],
        16..=231 => {
            let n = i - 16;
            let step = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            [step(n / 36), step((n / 6) % 6), step(n % 6)]
        }
        _ => {
            let g = 8 + (i - 232) * 10;
            [g, g, g]
        }
    }
}

/// A cell colour to RGB: the program's own OSC palette when it set one, ours otherwise.
fn resolve(color: Color, colors: &alacritty_terminal::term::color::Colors) -> [u8; 3] {
    match color {
        Color::Spec(rgb) => [rgb.r, rgb.g, rgb.b],
        Color::Indexed(i) => colors[i as usize]
            .map(|c| [c.r, c.g, c.b])
            .unwrap_or_else(|| indexed(i)),
        Color::Named(n) => {
            let idx = n as usize;
            if let Some(c) = colors[idx] {
                return [c.r, c.g, c.b];
            }
            match n {
                NamedColor::Foreground | NamedColor::Cursor => FG,
                NamedColor::Background => BG,
                NamedColor::DimForeground => [120, 150, 115],
                NamedColor::BrightForeground => [230, 255, 220],
                _ if idx < 16 => ANSI[idx],
                // Dim variants: their normal colour, darkened.
                _ => {
                    let base = ANSI[(idx.saturating_sub(NamedColor::DimBlack as usize)).min(7)];
                    [base[0] * 2 / 3, base[1] * 2 / 3, base[2] * 2 / 3]
                }
            }
        }
    }
}

/// Bytes for a key press, as a terminal expects them. Printable text does not come here (it
/// arrives as text events); this is for editing and control keys.
pub fn key_bytes(key: egui::Key, mods: egui::Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    use egui::Key as K;
    let ctrl = mods.ctrl || mods.command;
    let arrow = |plain: &str, app: &str| -> Vec<u8> {
        if ctrl {
            format!("\x1b[1;5{plain}").into_bytes()
        } else if mods.shift {
            format!("\x1b[1;2{plain}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{app}").into_bytes()
        } else {
            format!("\x1b[{plain}").into_bytes()
        }
    };
    let seq = match key {
        K::Enter => b"\r".to_vec(),
        K::Backspace => {
            if ctrl {
                b"\x17".to_vec()
            } else {
                b"\x7f".to_vec()
            }
        }
        K::Tab => {
            if mods.shift {
                b"\x1b[Z".to_vec()
            } else {
                b"\t".to_vec()
            }
        }
        K::Escape => b"\x1b".to_vec(),
        K::ArrowUp => arrow("A", "A"),
        K::ArrowDown => arrow("B", "B"),
        K::ArrowRight => arrow("C", "C"),
        K::ArrowLeft => arrow("D", "D"),
        K::Home => b"\x1b[H".to_vec(),
        K::End => b"\x1b[F".to_vec(),
        K::PageUp => b"\x1b[5~".to_vec(),
        K::PageDown => b"\x1b[6~".to_vec(),
        K::Insert => b"\x1b[2~".to_vec(),
        K::Delete => b"\x1b[3~".to_vec(),
        K::F1 => b"\x1bOP".to_vec(),
        K::F2 => b"\x1bOQ".to_vec(),
        K::F3 => b"\x1bOR".to_vec(),
        K::F4 => b"\x1bOS".to_vec(),
        K::F5 => b"\x1b[15~".to_vec(),
        K::F6 => b"\x1b[17~".to_vec(),
        K::F7 => b"\x1b[18~".to_vec(),
        K::F8 => b"\x1b[19~".to_vec(),
        K::F9 => b"\x1b[20~".to_vec(),
        K::F10 => b"\x1b[21~".to_vec(),
        K::F11 => b"\x1b[23~".to_vec(),
        K::F12 => b"\x1b[24~".to_vec(),
        K::Space if ctrl => b"\x00".to_vec(),
        // Ctrl + letter: the control byte. Ctrl+V is paste (arrives as text), never 0x16.
        k if ctrl => {
            let name = k.name();
            let mut chars = name.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_alphabetic() && c != 'V' => {
                    vec![(c.to_ascii_lowercase() as u8) - b'a' + 1]
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    Some(seq)
}

/// The tabs of one session: several consoles, one active.
pub struct TabSet {
    pub tabs: Vec<PtyTab>,
    pub active: usize,
    next_no: usize,
}

impl TabSet {
    fn new(cwd: &str) -> std::io::Result<TabSet> {
        let mut t = TabSet {
            tabs: Vec::new(),
            active: 0,
            next_no: 1,
        };
        t.open(cwd)?;
        Ok(t)
    }

    /// A new tab in `cwd`, made active.
    pub fn open(&mut self, cwd: &str) -> std::io::Result<()> {
        let mut tab = PtyTab::spawn(cwd, 100, 30)?;
        tab.label = self.next_no.to_string();
        self.next_no += 1;
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        Ok(())
    }

    /// Close a tab. Closing the last one leaves the set empty, which closes the card.
    pub fn close(&mut self, i: usize) {
        if i < self.tabs.len() {
            self.tabs.remove(i);
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn active_mut(&mut self) -> &mut PtyTab {
        let i = self.active.min(self.tabs.len() - 1);
        &mut self.tabs[i]
    }

    /// Replace the active tab with a fresh shell in its starting folder.
    pub fn restart_active(&mut self) {
        let i = self.active.min(self.tabs.len() - 1);
        let (cwd, label) = (self.tabs[i].cwd.clone(), self.tabs[i].label.clone());
        let (cols, rows) = (self.tabs[i].cols, self.tabs[i].rows);
        if let Ok(mut fresh) = PtyTab::spawn(&cwd, cols, rows) {
            fresh.label = label;
            self.tabs[i] = fresh;
        }
    }

    /// (label, title, dead) per tab for the tab bar: the title is what the program set.
    pub fn titles(&self) -> Vec<(String, String, bool)> {
        self.tabs
            .iter()
            .map(|t| {
                let title = t.title().unwrap_or_else(|| {
                    t.cwd
                        .rsplit(['\\', '/'])
                        .find(|p| !p.is_empty())
                        .unwrap_or("")
                        .to_string()
                });
                (t.label.clone(), title, t.is_dead())
            })
            .collect()
    }
}

/// One tab set per session, started on first use in that session's folder.
#[derive(Resource, Default)]
pub struct Terminals {
    sets: HashMap<String, TabSet>,
    pub error: Option<String>,
}

impl Terminals {
    /// Drop a session's consoles (after its last tab was closed).
    pub fn remove(&mut self, session_id: &str) {
        self.sets.remove(session_id);
    }

    pub fn set_mut(&mut self, session_id: &str, cwd: &str) -> Option<&mut TabSet> {
        if !self.sets.contains_key(session_id) {
            match TabSet::new(cwd) {
                Ok(t) => {
                    self.sets.insert(session_id.to_string(), t);
                }
                Err(e) => {
                    self.error = Some(format!("could not start a console: {e}"));
                    return None;
                }
            }
        }
        self.sets.get_mut(session_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_encode_like_a_terminal() {
        let none = egui::Modifiers::NONE;
        let ctrl = egui::Modifiers::CTRL;
        assert_eq!(key_bytes(egui::Key::Enter, none, false), Some(b"\r".to_vec()));
        assert_eq!(key_bytes(egui::Key::ArrowUp, none, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_bytes(egui::Key::ArrowUp, none, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(key_bytes(egui::Key::C, ctrl, false), Some(vec![3]), "Ctrl+C interrupts");
        assert_eq!(
            key_bytes(egui::Key::V, ctrl, false),
            None,
            "Ctrl+V is a paste, not a byte"
        );
        assert_eq!(key_bytes(egui::Key::A, none, false), None, "letters arrive as text");
        assert_eq!(
            key_bytes(egui::Key::Tab, egui::Modifiers::SHIFT, false),
            Some(b"\x1b[Z".to_vec())
        );
    }

    /// A real console on this machine: PowerShell answers through ConPTY.
    #[test]
    #[cfg(windows)]
    fn console_runs_a_command() {
        let tab = PtyTab::spawn("", 80, 24).expect("conpty");
        tab.write(b"Write-Output (6*7)\r".to_vec());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if tab.all_lines().iter().any(|l| l.trim() == "42") {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "no answer from the console: {:?}",
                tab.all_lines()
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    /// Ctrl+C as the ETX byte interrupts a running command. This needs the inherited
    /// "ignore Ctrl+C" flag cleared before the console child is created (see `spawn_with`).
    #[test]
    #[cfg(windows)]
    fn ctrl_c_interrupts_a_running_command() {
        let tab = PtyTab::spawn("", 80, 24).expect("conpty");
        std::thread::sleep(std::time::Duration::from_millis(1200));
        tab.write(b"ping -t 127.0.0.1".to_vec());
        tab.write(vec![13u8]);
        std::thread::sleep(std::time::Duration::from_millis(1500));
        tab.write(vec![3u8]);
        std::thread::sleep(std::time::Duration::from_millis(600));
        tab.write(b"echo AG_AFTER".to_vec());
        tab.write(vec![13u8]);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        loop {
            if tab.all_lines().iter().any(|l| l.trim() == "AG_AFTER") {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "ping was not interrupted");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    #[test]
    fn palette_covers_all_256() {
        assert_eq!(indexed(1), ANSI[1]);
        assert_eq!(indexed(16), [0, 0, 0]);
        assert_eq!(indexed(231), [255, 255, 255]);
        assert_eq!(indexed(232), [8, 8, 8]);
        assert_eq!(indexed(255), [238, 238, 238]);
    }
}
