//! Git awareness for the watched session's working directory: branch, uncommitted changes,
//! remote URL. Polled by a thread so the frame never waits on git.

use bevy::prelude::*;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One file the working tree changed against HEAD.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileChange {
    /// Repo-relative path with forward slashes.
    pub path: String,
    pub added: usize,
    pub deleted: usize,
    /// Not yet tracked by git.
    pub untracked: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GitInfo {
    pub cwd: String,
    pub branch: String,
    pub dirty: usize,
    pub remote: String,
    /// Changed files, modified ones first, then untracked; capped so a huge tree stays cheap.
    pub changes: Vec<FileChange>,
}

const MAX_CHANGES: usize = 200;

/// Parse `git diff --numstat` (modified) and `git status --porcelain` (untracked) output.
pub fn parse_changes(numstat: &str, porcelain: &str) -> Vec<FileChange> {
    let mut out: Vec<FileChange> = Vec::new();
    for line in numstat.lines() {
        let mut parts = line.split('\t');
        let (Some(a), Some(d), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        out.push(FileChange {
            path: path.trim().to_string(),
            added: a.trim().parse().unwrap_or(0),
            deleted: d.trim().parse().unwrap_or(0),
            untracked: false,
        });
    }
    for line in porcelain.lines() {
        if let Some(path) = line.strip_prefix("?? ") {
            out.push(FileChange {
                path: path.trim().trim_matches('"').to_string(),
                added: 0,
                deleted: 0,
                untracked: true,
            });
        }
    }
    out.truncate(MAX_CHANGES);
    out
}

/// Unified diff of one file against HEAD; untracked files show as all additions.
pub fn diff_text(cwd: &str, path: &str, untracked: bool) -> Option<String> {
    if untracked {
        let full = std::path::Path::new(cwd).join(path);
        let text = std::fs::read_to_string(full).ok()?;
        let mut out = format!("+++ {path} (new file)\n");
        for l in text.lines() {
            out.push('+');
            out.push_str(l);
            out.push('\n');
        }
        return Some(out);
    }
    git(cwd, &["diff", "HEAD", "--", path])
}

impl GitInfo {
    /// `https://github.com/owner/repo` from an https or ssh remote, if it is GitHub-like.
    pub fn web_url(&self) -> Option<String> {
        let r = self.remote.trim_end_matches(".git");
        if let Some(rest) = r.strip_prefix("git@") {
            let (host, path) = rest.split_once(':')?;
            return Some(format!("https://{host}/{path}"));
        }
        if r.starts_with("https://") || r.starts_with("http://") {
            return Some(r.to_string());
        }
        None
    }
}

#[derive(Resource, Clone)]
pub struct GitWatch {
    cwd: Arc<Mutex<String>>,
    info: Arc<Mutex<Option<GitInfo>>>,
}

fn git(cwd: &str, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(cwd).args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn read(cwd: &str) -> Option<GitInfo> {
    let branch = git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let dirty = git(cwd, &["status", "--porcelain"])
        .map(|s| s.lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0);
    let remote = git(cwd, &["remote", "get-url", "origin"]).unwrap_or_default();
    let numstat = git(cwd, &["diff", "HEAD", "--numstat"]).unwrap_or_default();
    let porcelain = git(cwd, &["status", "--porcelain", "--untracked-files=all"]).unwrap_or_default();
    Some(GitInfo {
        cwd: cwd.to_string(),
        branch,
        dirty,
        remote,
        changes: parse_changes(&numstat, &porcelain),
    })
}

impl Default for GitWatch {
    fn default() -> Self {
        let cwd: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let info: Arc<Mutex<Option<GitInfo>>> = Arc::new(Mutex::new(None));
        {
            let (cwd, info) = (cwd.clone(), info.clone());
            std::thread::Builder::new()
                .name("git-watch".into())
                .spawn(move || {
                    loop {
                        let dir = cwd.lock().map(|c| c.clone()).unwrap_or_default();
                        let next = if dir.is_empty() { None } else { read(&dir) };
                        if let Ok(mut i) = info.lock() {
                            *i = next;
                        }
                        std::thread::sleep(Duration::from_secs(6));
                    }
                })
                .expect("spawn git-watch");
        }
        GitWatch { cwd, info }
    }
}

impl GitWatch {
    pub fn watch(&self, cwd: &str) {
        if let Ok(mut c) = self.cwd.lock()
            && *c != cwd
        {
            *c = cwd.to_string();
            if let Ok(mut i) = self.info.lock() {
                *i = None;
            }
        }
    }

    pub fn info(&self) -> Option<GitInfo> {
        self.info.lock().ok().and_then(|i| i.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numstat_and_porcelain_become_changes() {
        let c = parse_changes(
            "12\t3\tsrc/app/ui.rs\n-\t-\tassets/icon.png\n",
            " M src/app/ui.rs\n?? notes.md\n?? \"with space.txt\"\n",
        );
        assert_eq!(c.len(), 4);
        assert_eq!((c[0].added, c[0].deleted, c[0].untracked), (12, 3, false));
        assert_eq!((c[1].added, c[1].deleted), (0, 0), "binary counts read as zero");
        assert!(c[2].untracked && c[2].path == "notes.md");
        assert_eq!(c[3].path, "with space.txt");
    }

    #[test]
    fn remote_to_web_url() {
        let g = GitInfo {
            remote: "git@github.com:MrFo2sh/agent-globe.git".into(),
            ..Default::default()
        };
        assert_eq!(g.web_url().as_deref(), Some("https://github.com/MrFo2sh/agent-globe"));
        let h = GitInfo {
            remote: "https://github.com/MrFo2sh/agent-globe.git".into(),
            ..Default::default()
        };
        assert_eq!(h.web_url().as_deref(), Some("https://github.com/MrFo2sh/agent-globe"));
    }
}
