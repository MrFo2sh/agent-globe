//! Git awareness for the watched session's working directory: branch, uncommitted changes,
//! remote URL. Polled by a thread so the frame never waits on git.

use bevy::prelude::*;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GitInfo {
    pub cwd: String,
    pub branch: String,
    pub dirty: usize,
    pub remote: String,
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
    Some(GitInfo {
        cwd: cwd.to_string(),
        branch,
        dirty,
        remote,
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
                        std::thread::sleep(Duration::from_secs(12));
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
