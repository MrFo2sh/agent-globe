//! A thread that asks Windows which of the session's servers are listening, which process owns
//! them, what their index page is called, and which background processes the session spawned.
//! Killing is one `taskkill` on the owning process tree.

use agent_globe::data::servers::{Targets, html_title, parse_netstat};
use bevy::prelude::*;
use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct ServerInfo {
    pub port: u16,
    pub url: String,
    pub alive: bool,
    pub pid: Option<u32>,
    pub name: String,
    pub title: Option<String>,
    /// Owned by Docker, the system or this app: shown, never killable from here.
    pub protected: bool,
}

/// Processes a ✕ must never reach: ending them would take Docker Desktop, Windows services or
/// Agent Globe itself down with the port.
pub fn is_protected(name: &str, pid: u32) -> bool {
    if pid == std::process::id() || pid <= 4 {
        return true;
    }
    let n = name.to_ascii_lowercase();
    [
        "com.docker",
        "docker",
        "wslrelay",
        "wslhost",
        "vmmem",
        "svchost",
        "system",
        "agent-globe",
        "lsass",
        "services",
    ]
    .iter()
    .any(|p| n.starts_with(p))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    pub cmdline: String,
    /// The command in the session that this process matched.
    pub from: String,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub servers: Vec<ServerInfo>,
    pub procs: Vec<ProcInfo>,
}

#[derive(Resource, Clone)]
pub struct ServerWatch {
    wanted: Arc<Mutex<Targets>>,
    snap: Arc<Mutex<Snapshot>>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WinProcess {
    process_id: u32,
    name: Option<String>,
    command_line: Option<String>,
}

fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

fn listeners() -> HashMap<u16, u32> {
    let out = quiet(Command::new("netstat").args(["-ano", "-p", "tcp"])).output();
    match out {
        Ok(o) => parse_netstat(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => HashMap::new(),
    }
}

fn processes() -> Vec<WinProcess> {
    let script = "Get-CimInstance Win32_Process | Select-Object ProcessId,Name,CommandLine | ConvertTo-Json -Compress";
    let out = quiet(Command::new("powershell").args(["-NoProfile", "-Command", script])).output();
    let Ok(o) = out else { return Vec::new() };
    let text = String::from_utf8_lossy(&o.stdout);
    serde_json::from_str::<Vec<WinProcess>>(text.trim())
        .or_else(|_| serde_json::from_str::<WinProcess>(text.trim()).map(|p| vec![p]))
        .unwrap_or_default()
}

fn title_of(port: u16) -> Option<String> {
    let body = ureq::get(&format!("http://127.0.0.1:{port}/"))
        .timeout(Duration::from_secs(2))
        .call()
        .ok()?
        .into_string()
        .ok()?;
    html_title(&body)
}

impl Default for ServerWatch {
    fn default() -> Self {
        let wanted: Arc<Mutex<Targets>> = Arc::new(Mutex::new(Targets::default()));
        let snap: Arc<Mutex<Snapshot>> = Arc::new(Mutex::new(Snapshot::default()));
        {
            let (wanted, snap) = (wanted.clone(), snap.clone());
            std::thread::Builder::new()
                .name("servers".into())
                .spawn(move || {
                    let mut titles: HashMap<u16, (Instant, Option<String>)> = HashMap::new();
                    loop {
                        let t = wanted.lock().map(|w| w.clone()).unwrap_or_default();
                        if t.ports.is_empty() && t.needles.is_empty() {
                            if let Ok(mut s) = snap.lock() {
                                *s = Snapshot::default();
                            }
                            std::thread::sleep(Duration::from_secs(3));
                            continue;
                        }
                        let listening = listeners();
                        let need_scan = !t.needles.is_empty() || t.ports.iter().any(|(p, _)| listening.contains_key(p));
                        let procs = if need_scan { processes() } else { Vec::new() };
                        let by_pid: HashMap<u32, &WinProcess> = procs.iter().map(|p| (p.process_id, p)).collect();
                        let mut servers = Vec::new();
                        for (port, url) in &t.ports {
                            let pid = listening.get(port).copied();
                            let alive = pid.is_some();
                            let name = pid
                                .and_then(|p| by_pid.get(&p))
                                .and_then(|p| p.name.clone())
                                .unwrap_or_default();
                            let title = if alive {
                                let fresh = titles
                                    .get(port)
                                    .is_some_and(|(at, _)| at.elapsed() < Duration::from_secs(60));
                                if !fresh {
                                    titles.insert(*port, (Instant::now(), title_of(*port)));
                                }
                                titles.get(port).and_then(|(_, t)| t.clone())
                            } else {
                                titles.remove(port);
                                None
                            };
                            let protected = pid.is_some_and(|p| is_protected(&name, p));
                            servers.push(ServerInfo {
                                port: *port,
                                url: url.clone(),
                                alive,
                                pid,
                                name,
                                title,
                                protected,
                            });
                        }
                        let server_pids: Vec<u32> = servers.iter().filter_map(|s| s.pid).collect();
                        let mut found: Vec<ProcInfo> = Vec::new();
                        for (needle, from) in &t.needles {
                            let n = needle.to_ascii_lowercase();
                            for p in &procs {
                                let Some(cl) = p.command_line.as_deref() else { continue };
                                if !cl.to_ascii_lowercase().contains(&n) {
                                    continue;
                                }
                                if server_pids.contains(&p.process_id) || found.iter().any(|f| f.pid == p.process_id) {
                                    continue;
                                }
                                // powershell / cmd wrappers that merely mention the command are not it
                                let name = p.name.clone().unwrap_or_default();
                                if name.eq_ignore_ascii_case("powershell.exe")
                                    || name.eq_ignore_ascii_case("cmd.exe")
                                    || is_protected(&name, p.process_id)
                                {
                                    continue;
                                }
                                found.push(ProcInfo {
                                    pid: p.process_id,
                                    name,
                                    cmdline: cl.chars().take(120).collect(),
                                    from: from.clone(),
                                });
                            }
                        }
                        if let Ok(mut s) = snap.lock() {
                            *s = Snapshot { servers, procs: found };
                        }
                        std::thread::sleep(Duration::from_secs(5));
                    }
                })
                .expect("spawn servers");
        }
        ServerWatch { wanted, snap }
    }
}

impl ServerWatch {
    pub fn set_wanted(&self, t: Targets) {
        if let Ok(mut w) = self.wanted.lock()
            && *w != t
        {
            *w = t;
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snap.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

/// End a process and everything it started. Returns the message for the toast.
pub fn kill(pid: u32) -> String {
    let out = quiet(Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"])).output();
    match out {
        Ok(o) if o.status.success() => format!("✖ process {pid} ended"),
        Ok(o) => format!("could not end {pid}: {}", String::from_utf8_lossy(&o.stderr).trim()),
        Err(e) => format!("could not run taskkill: {e}"),
    }
}
