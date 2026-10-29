//! An integrated shell per session: one persistent PowerShell behind a pipe, started in the
//! session's folder. Commands go in on stdin, output lines come back on a reader thread, and a
//! sentinel line marks the end of each command and carries the folder the shell is in now.
//! Not a terminal emulator: no cursor control, no interactive programs. Right for `git status`,
//! `npm run build`, `cargo test` and friends, which is what a session's folder needs.

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
        })
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
        push(
            &self.lines,
            Line {
                text: format!("❯ {cmd}"),
                kind: LineKind::Cmd,
            },
        );
        if self.history.last().map(String::as_str) != Some(cmd) {
            self.history.push(cmd.to_string());
        }
        self.hist_pos = None;
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

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// One shell per session, started on first use in that session's folder.
#[derive(Resource, Default)]
pub struct Terminals {
    shells: HashMap<String, Shell>,
    pub error: Option<String>,
}

impl Terminals {
    pub fn get_or_spawn(&mut self, session_id: &str, cwd: &str) -> Option<&mut Shell> {
        if !self.shells.contains_key(session_id) {
            match Shell::spawn(cwd) {
                Ok(s) => {
                    self.shells.insert(session_id.to_string(), s);
                }
                Err(e) => {
                    self.error = Some(format!("could not start powershell: {e}"));
                    return None;
                }
            }
        }
        self.shells.get_mut(session_id)
    }

    /// Kill the session's shell and start a fresh one in the same folder.
    pub fn restart(&mut self, session_id: &str, cwd: &str) {
        let keep: Vec<String> = self
            .shells
            .get(session_id)
            .map(|s| s.history.clone())
            .unwrap_or_default();
        self.shells.remove(session_id);
        if let Some(s) = self.get_or_spawn(session_id, cwd) {
            s.history = keep;
            push(
                &s.lines,
                Line {
                    text: "shell restarted".into(),
                    kind: LineKind::Note,
                },
            );
        }
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
