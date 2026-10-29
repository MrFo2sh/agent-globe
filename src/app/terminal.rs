//! An integrated shell per session: one persistent PowerShell behind a pipe, started in the
//! session's folder. Commands go in on stdin, output lines come back on a reader thread, and a
//! sentinel line marks the end of each command and carries the folder the shell is in now.
//! Tab completion comes from PowerShell's own engine (TabExpansion2) in a side process, Ctrl+C
//! ends the running command by restarting the shell in place, and a session can hold several
//! tabs. Not a terminal emulator: no cursor control, no full-screen programs. Right for
//! `git status`, `npm run build`, `cargo test` and friends, which is what a session's folder needs.

use bevy::prelude::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Printed by the shell after every command; what follows is the current folder.
const SENTINEL: &str = "<<AG_END>>";
/// Lines kept per shell; older ones are dropped from the top.
const SCROLLBACK: usize = 4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// The command as typed.
    Cmd,
    /// Output, stdout and stderr merged.
    Out,
    /// A note from the app itself (shell started, restarted, died).
    Note,
}

#[derive(Debug, Clone)]
pub struct Line {
    pub text: String,
    pub kind: LineKind,
}

/// One answer from TabExpansion2: what to replace in `base`, and the candidates.
#[derive(Debug, Clone)]
pub struct Completion {
    pub base: String,
    pub index: usize,
    pub len: usize,
    pub matches: Vec<String>,
    pub pick: usize,
}

pub struct Shell {
    child: Child,
    stdin: ChildStdin,
    pub lines: Arc<Mutex<Vec<Line>>>,
    pub cwd: Arc<Mutex<String>>,
    pub busy: Arc<AtomicBool>,
    pub dead: Arc<AtomicBool>,
    pub input: String,
    pub history: Vec<String>,
    pub hist_pos: Option<usize>,
    /// Put the caret in the input on the next frame (just opened, or just ran a command).
    pub want_focus: bool,
    /// Tab label.
    pub title: String,
    /// The completion answer landing from the side process.
    inbox: Arc<Mutex<Option<Completion>>>,
    comp_inflight: Arc<AtomicBool>,
    /// The completion being cycled with Tab, and the input text it last produced.
    active_comp: Option<Completion>,
    applied: Option<String>,
}

fn push(lines: &Mutex<Vec<Line>>, line: Line) {
    if let Ok(mut l) = lines.lock() {
        l.push(line);
        if l.len() > SCROLLBACK + 200 {
            let extra = l.len() - SCROLLBACK;
            l.drain(..extra);
        }
    }
}

fn reader<R: std::io::Read + Send + 'static>(
    src: R,
    lines: Arc<Mutex<Vec<Line>>>,
    cwd: Arc<Mutex<String>>,
    busy: Arc<AtomicBool>,
    dead: Arc<AtomicBool>,
    name: &'static str,
) {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            for line in BufReader::new(src).lines() {
                let Ok(line) = line else { break };
                if let Some(rest) = line.strip_prefix(SENTINEL) {
                    if let Ok(mut c) = cwd.lock() {
                        *c = rest.trim().to_string();
                    }
                    busy.store(false, Ordering::Relaxed);
                    continue;
                }
                push(
                    &lines,
                    Line {
                        text: line,
                        kind: LineKind::Out,
                    },
                );
            }
            dead.store(true, Ordering::Relaxed);
            busy.store(false, Ordering::Relaxed);
        })
        .expect("spawn shell reader");
}

impl Shell {
    pub fn spawn(cwd: &str) -> std::io::Result<Shell> {
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-NoLogo", "-NonInteractive", "-Command", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !cwd.is_empty() && std::path::Path::new(cwd).is_dir() {
            cmd.current_dir(cwd);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let lines = Arc::new(Mutex::new(vec![Line {
            text: format!("powershell · {cwd}"),
            kind: LineKind::Note,
        }]));
        let cwd_now = Arc::new(Mutex::new(cwd.to_string()));
        let busy = Arc::new(AtomicBool::new(false));
        let dead = Arc::new(AtomicBool::new(false));
        reader(
            stdout,
            lines.clone(),
            cwd_now.clone(),
            busy.clone(),
            dead.clone(),
            "shell-out",
        );
        reader(
            stderr,
            lines.clone(),
            cwd_now.clone(),
            busy.clone(),
            dead.clone(),
            "shell-err",
        );
        Ok(Shell {
            child,
            stdin,
            lines,
            cwd: cwd_now,
            busy,
            dead,
            input: String::new(),
            history: Vec::new(),
            hist_pos: None,
            want_focus: true,
            title: String::new(),
            inbox: Arc::new(Mutex::new(None)),
            comp_inflight: Arc::new(AtomicBool::new(false)),
            active_comp: None,
            applied: None,
        })
    }

    /// Kill the child and start a fresh shell in the same folder, keeping the scrollback, the
    /// history and the tab. Ctrl+C and ⟲ both land here.
    pub fn restart_in_place(&mut self, note: &str) {
        let cwd = self.cwd();
        let _ = self.child.kill();
        let Ok(mut fresh) = Shell::spawn(&cwd) else {
            push(
                &self.lines,
                Line {
                    text: "could not restart the shell".into(),
                    kind: LineKind::Note,
                },
            );
            return;
        };
        if let (Ok(mut new), Ok(old)) = (fresh.lines.lock(), self.lines.lock()) {
            let keep: Vec<Line> = old.iter().cloned().collect();
            new.splice(0..0, keep);
            new.push(Line {
                text: note.to_string(),
                kind: LineKind::Note,
            });
        }
        fresh.history = std::mem::take(&mut self.history);
        fresh.title = std::mem::take(&mut self.title);
        fresh.want_focus = true;
        *self = fresh;
    }

    /// Tab: cycle the current completion, or ask PowerShell for one.
    pub fn tab(&mut self, backwards: bool) {
        self.input = self.input.replace('\t', "");
        if self.active_comp.is_some() && self.applied.as_deref() == Some(self.input.as_str()) {
            self.cycle(if backwards { -1 } else { 1 });
            return;
        }
        self.active_comp = None;
        self.request_completion();
    }

    fn request_completion(&mut self) {
        if self.comp_inflight.swap(true, Ordering::Relaxed) || self.input.trim().is_empty() {
            self.comp_inflight.store(false, Ordering::Relaxed);
            return;
        }
        let input = self.input.clone();
        let cwd = self.cwd();
        let inbox = self.inbox.clone();
        let inflight = self.comp_inflight.clone();
        std::thread::Builder::new()
            .name("shell-complete".into())
            .spawn(move || {
                let got = complete_via_powershell(&cwd, &input);
                if let Ok(mut slot) = inbox.lock() {
                    *slot = got;
                }
                inflight.store(false, Ordering::Relaxed);
            })
            .expect("spawn completion");
    }

    /// Take a landed completion if it is still about the current input, and apply its first match.
    pub fn poll_completion(&mut self) {
        let landed = self.inbox.lock().ok().and_then(|mut s| s.take());
        if let Some(c) = landed
            && c.base == self.input
            && !c.matches.is_empty()
        {
            self.active_comp = Some(c);
            self.apply_comp();
        }
    }

    fn cycle(&mut self, dir: i32) {
        if let Some(c) = self.active_comp.as_mut() {
            let n = c.matches.len() as i32;
            c.pick = ((c.pick as i32 + dir).rem_euclid(n.max(1))) as usize;
        }
        self.apply_comp();
    }

    fn apply_comp(&mut self) {
        let Some(c) = &self.active_comp else { return };
        let Some(m) = c.matches.get(c.pick) else { return };
        let chars: Vec<char> = c.base.chars().collect();
        let i = c.index.min(chars.len());
        let j = (c.index + c.len).min(chars.len());
        let head: String = chars[..i].iter().collect();
        let tail: String = chars[j..].iter().collect();
        self.input = format!("{head}{m}{tail}");
        self.applied = Some(self.input.clone());
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }

    pub fn is_dead(&self) -> bool {
        self.dead.load(Ordering::Relaxed)
    }

    pub fn cwd(&self) -> String {
        self.cwd.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// Run one line. Dot-sourced so `cd` and variables stick; errors are merged into the output;
    /// the sentinel on its own line survives a parse error in the command.
    pub fn run(&mut self, cmd: &str) {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return;
        }
        if cmd.eq_ignore_ascii_case("cls") || cmd.eq_ignore_ascii_case("clear") {
            self.clear();
            return;
        }
        push(
            &self.lines,
            Line {
                text: format!("PS {}> {cmd}", self.cwd()),
                kind: LineKind::Cmd,
            },
        );
        if self.history.last().map(String::as_str) != Some(cmd) {
            self.history.push(cmd.to_string());
        }
        self.hist_pos = None;
        self.active_comp = None;
        self.applied = None;
        self.busy.store(true, Ordering::Relaxed);
        let wrapped =
            format!(". {{ {cmd} }} 2>&1 | Out-String -Stream\nWrite-Output ('{SENTINEL}' + (Get-Location).Path)\n");
        if self
            .stdin
            .write_all(wrapped.as_bytes())
            .and_then(|_| self.stdin.flush())
            .is_err()
        {
            self.dead.store(true, Ordering::Relaxed);
            self.busy.store(false, Ordering::Relaxed);
            push(
                &self.lines,
                Line {
                    text: "shell is gone: restart it".into(),
                    kind: LineKind::Note,
                },
            );
        }
    }

    /// Previous (`-1`) or next (`+1`) history entry into the input.
    pub fn recall(&mut self, dir: i32) {
        self.hist_pos = hist_step(self.history.len(), self.hist_pos, dir);
        self.input = self.hist_pos.map(|i| self.history[i].clone()).unwrap_or_default();
    }

    pub fn snapshot(&self) -> Vec<Line> {
        self.lines.lock().map(|l| l.clone()).unwrap_or_default()
    }

    pub fn clear(&self) {
        if let Ok(mut l) = self.lines.lock() {
            l.clear();
        }
    }
}

/// Where ↑ (`dir < 0`) or ↓ lands in a history of `n` entries from `pos`; `None` is the empty
/// input past the newest entry. ↑ stops at the oldest.
pub fn hist_step(n: usize, pos: Option<usize>, dir: i32) -> Option<usize> {
    if n == 0 {
        return None;
    }
    match (pos, dir < 0) {
        (None, true) => Some(n - 1),
        (None, false) => None,
        (Some(i), true) => Some(i.saturating_sub(1)),
        (Some(i), false) if i + 1 >= n => None,
        (Some(i), false) => Some(i + 1),
    }
}

/// Ask a side PowerShell what TabExpansion2 would offer for `input` in `cwd`.
fn complete_via_powershell(cwd: &str, input: &str) -> Option<Completion> {
    let quoted = input.replace('\'', "''");
    let cwd_q = cwd.replace('\'', "''");
    let script = format!(
        "Set-Location '{cwd_q}'; $r = TabExpansion2 -inputScript '{quoted}' -cursorColumn {}; \
         \"$($r.ReplacementIndex)|$($r.ReplacementLength)\"; $r.CompletionMatches | ForEach-Object {{ $_.CompletionText }}",
        input.chars().count()
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NoLogo", "-NonInteractive", "-Command", &script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let head = lines.next()?;
    let (i, l) = head.split_once('|')?;
    let matches: Vec<String> = lines
        .map(|m| m.trim_end().to_string())
        .filter(|m| !m.is_empty())
        .collect();
    Some(Completion {
        base: input.to_string(),
        index: i.trim().parse().ok()?,
        len: l.trim().parse().ok()?,
        matches,
        pick: 0,
    })
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// The tabs of one session: several shells, one active.
pub struct TabSet {
    pub shells: Vec<Shell>,
    pub active: usize,
    next_no: usize,
}

impl TabSet {
    fn new(cwd: &str) -> std::io::Result<TabSet> {
        let mut t = TabSet {
            shells: Vec::new(),
            active: 0,
            next_no: 1,
        };
        t.open(cwd)?;
        Ok(t)
    }

    /// A new tab in `cwd`, made active.
    pub fn open(&mut self, cwd: &str) -> std::io::Result<()> {
        let mut sh = Shell::spawn(cwd)?;
        sh.title = self.next_no.to_string();
        self.next_no += 1;
        self.shells.push(sh);
        self.active = self.shells.len() - 1;
        Ok(())
    }

    /// Close a tab; the last one stays.
    pub fn close(&mut self, i: usize) {
        if self.shells.len() > 1 && i < self.shells.len() {
            self.shells.remove(i);
            self.active = self.active.min(self.shells.len() - 1);
        }
    }

    pub fn active_mut(&mut self) -> &mut Shell {
        let i = self.active.min(self.shells.len() - 1);
        &mut self.shells[i]
    }

    /// (title, folder name, busy) per tab, for the tab bar.
    pub fn tabs(&self) -> Vec<(String, String, bool)> {
        self.shells
            .iter()
            .map(|s| {
                let cwd = s.cwd();
                let folder = cwd
                    .rsplit(['\\', '/'])
                    .find(|p| !p.is_empty())
                    .unwrap_or("")
                    .to_string();
                (s.title.clone(), folder, s.is_busy())
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
    pub fn set_mut(&mut self, session_id: &str, cwd: &str) -> Option<&mut TabSet> {
        if !self.sets.contains_key(session_id) {
            match TabSet::new(cwd) {
                Ok(t) => {
                    self.sets.insert(session_id.to_string(), t);
                }
                Err(e) => {
                    self.error = Some(format!("could not start powershell: {e}"));
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
    fn history_recall_walks_back_and_forward() {
        assert_eq!(hist_step(0, None, -1), None);
        assert_eq!(hist_step(2, None, -1), Some(1));
        assert_eq!(hist_step(2, Some(1), -1), Some(0));
        assert_eq!(hist_step(2, Some(0), -1), Some(0), "stays at the oldest");
        assert_eq!(hist_step(2, Some(0), 1), Some(1));
        assert_eq!(hist_step(2, Some(1), 1), None, "past the newest clears the input");
        let lines = Mutex::new(Vec::new());
        push(
            &lines,
            Line {
                text: "x".into(),
                kind: LineKind::Out,
            },
        );
        assert_eq!(lines.lock().unwrap().len(), 1);
    }
}
