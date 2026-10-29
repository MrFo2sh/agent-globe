//! What a session has running in the background: the servers it started (from served URLs and
//! server-like commands) and the background processes it spawned. Pure helpers here; the thread
//! that asks the OS lives in `app/servers.rs`.

use super::artifacts::ArtifactKind;
use super::model::Session;
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

/// What to look for on this machine for one session.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Targets {
    /// Local ports with the URL that named them, as the session's output showed it.
    pub ports: Vec<(u16, String)>,
    /// Command-line fragments of processes the session started in the background, with the
    /// command they came from.
    pub needles: Vec<(String, String)>,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

static LOCAL_URL: OnceLock<Regex> = OnceLock::new();
static PORT_FLAG: OnceLock<Regex> = OnceLock::new();
static TITLE: OnceLock<Regex> = OnceLock::new();
static ENV_PREFIX: OnceLock<Regex> = OnceLock::new();

/// Port of a URL on this machine, if it is one.
pub fn local_port(url: &str) -> Option<u16> {
    let r = re(
        &LOCAL_URL,
        r"^https?://(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\])(?::(\d{2,5}))?(?:/|$)",
    );
    let m = r.captures(url)?;
    match m.get(1) {
        Some(p) => p.as_str().parse().ok(),
        None => Some(if url.starts_with("https") { 443 } else { 80 }),
    }
}

/// Commands that start something meant to keep running.
pub fn looks_like_server(cmd: &str) -> bool {
    let c = cmd.to_ascii_lowercase();
    const HINTS: [&str; 20] = [
        "run dev",
        "npm start",
        "pnpm dev",
        "yarn dev",
        "vite",
        "next dev",
        "serve",
        "server",
        "uvicorn",
        "gunicorn",
        "flask run",
        "http.server",
        "cargo run",
        "docker compose up",
        "docker-compose up",
        "rails s",
        "php -s",
        "dotnet run",
        "go run",
        "ng serve",
    ];
    HINTS.iter().any(|h| c.contains(h))
}

/// The part of a command worth matching against a process command line: no `cd … &&`, no
/// env assignments, no redirections, no trailing `&`, at most sixty characters.
pub fn needle_for(cmd: &str) -> Option<String> {
    let mut c = cmd.trim().to_string();
    // last segment after a chain of cd / && ; the server is the final command
    if let Some(i) = c.rfind("&&") {
        c = c[i + 2..].trim().to_string();
    }
    c = re(&ENV_PREFIX, r"^(?:[A-Z_][A-Z0-9_]*=\S+\s+)+")
        .replace(&c, "")
        .to_string();
    for prefix in ["nohup ", "start /b ", "start ", "cmd /c ", "cmd.exe /c "] {
        if let Some(rest) = c.strip_prefix(prefix) {
            c = rest.trim().to_string();
        }
    }
    if let Some(i) = c.find(" >") {
        c.truncate(i);
    }
    if let Some(i) = c.find(" 2>") {
        c.truncate(i);
    }
    let c = c.trim_end_matches('&').trim().chars().take(60).collect::<String>();
    (c.len() >= 4).then_some(c)
}

/// Ports named on a command line: `--port 3000`, `-p 8080`, `:4000`.
pub fn ports_in_command(cmd: &str) -> Vec<u16> {
    // flags, host:port, or a bare port as the last argument (`http.server 8000`)
    let r = re(
        &PORT_FLAG,
        r"(?:--port[= ]|-p ?|--listen[= ]|:)(\d{2,5})\b|\s(\d{4,5})\s*$",
    );
    let mut out: Vec<u16> = r
        .captures_iter(cmd)
        .filter_map(|c| {
            c.get(1)
                .or_else(|| c.get(2))
                .and_then(|m| m.as_str().parse::<u16>().ok())
        })
        .filter(|p| *p >= 80)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Everything worth watching for this session.
pub fn session_targets(session: &Session) -> Targets {
    let mut ports: Vec<(u16, String)> = Vec::new();
    let mut needles: Vec<(String, String)> = Vec::new();
    for a in &session.agents {
        for art in a.artifacts.iter().filter(|x| x.kind == ArtifactKind::Url) {
            if let Some(p) = local_port(&art.target)
                && !ports.iter().any(|(q, _)| *q == p)
            {
                ports.push((p, art.target.clone()));
            }
        }
        for c in &a.calls {
            let Some(cmd) = c.command.as_deref() else { continue };
            if !(c.background || looks_like_server(cmd)) {
                continue;
            }
            if let Some(n) = needle_for(cmd)
                && !needles.iter().any(|(m, _)| *m == n)
            {
                needles.push((n, cmd.to_string()));
            }
            for p in ports_in_command(cmd) {
                if !ports.iter().any(|(q, _)| *q == p) {
                    ports.push((p, format!("http://localhost:{p}/")));
                }
            }
        }
    }
    Targets { ports, needles }
}

/// `netstat -ano -p tcp` lines with LISTENING: local port -> owning pid.
pub fn parse_netstat(text: &str) -> HashMap<u16, u32> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 5 || f[0] != "TCP" || f[3] != "LISTENING" {
            continue;
        }
        let Some(port) = f[1].rsplit(':').next().and_then(|p| p.parse::<u16>().ok()) else {
            continue;
        };
        let Ok(pid) = f[4].parse::<u32>() else { continue };
        out.entry(port).or_insert(pid);
    }
    out
}

/// The `<title>` of an HTML page, trimmed and shortened.
pub fn html_title(body: &str) -> Option<String> {
    let r = re(&TITLE, r"(?is)<title[^>]*>\s*(.*?)\s*</title>");
    let t = r.captures(body)?.get(1)?.as_str();
    let t: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t: String = t.chars().take(60).collect();
    (!t.is_empty()).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::model::{Event, ROOT_AGENT};

    #[test]
    fn local_urls_give_ports_and_commands_give_needles() {
        assert_eq!(local_port("http://localhost:3000/"), Some(3000));
        assert_eq!(local_port("http://127.0.0.1:8080/api"), Some(8080));
        assert_eq!(local_port("http://localhost"), Some(80));
        assert_eq!(local_port("https://example.com:3000/"), None);
        assert!(looks_like_server("npm run dev"));
        assert!(looks_like_server("cd api && uvicorn app:app --port 8000"));
        assert!(!looks_like_server("cargo test"));
        assert_eq!(
            needle_for("cd proxy && PORT=3000 nohup node src/server.js > out.log 2>&1 &"),
            Some("node src/server.js".into())
        );
        assert_eq!(needle_for("npm run dev"), Some("npm run dev".into()));
        assert_eq!(needle_for("ls"), None);
        assert_eq!(
            ports_in_command("uvicorn app:app --port 8000 -p 9000 http://localhost:3000"),
            vec![3000, 8000, 9000]
        );
    }

    #[test]
    fn netstat_and_titles_parse() {
        let text = "  TCP    0.0.0.0:135    0.0.0.0:0    LISTENING    932\n  TCP    127.0.0.1:8080   0.0.0.0:0   LISTENING   1234\n  TCP    [::]:8080   [::]:0   LISTENING   1234\n  UDP    0.0.0.0:500   *:*   4\n";
        let m = parse_netstat(text);
        assert_eq!(m.get(&8080), Some(&1234));
        assert_eq!(m.get(&135), Some(&932));
        assert_eq!(m.len(), 2);
        assert_eq!(
            html_title("<html><head><TITLE>\n  CTF  Watch \n</TITLE></head>"),
            Some("CTF Watch".into())
        );
        assert_eq!(html_title("<p>no title</p>"), None);
    }

    #[test]
    fn session_targets_come_from_urls_and_background_commands() {
        let mut s = Session::new("s", "p", 1);
        let start = |id: &str, cmd: &str, background: bool| Event::ToolStart {
            session_id: "s".into(),
            agent_id: ROOT_AGENT.into(),
            call_id: id.into(),
            tool: "Bash".into(),
            summary: cmd.into(),
            command: Some(cmd.into()),
            note: String::new(),
            todos: None,
            background,
            at: 5,
        };
        s.apply(start("1", "npm run dev -- --port 5173", true));
        s.apply(start("2", "cargo test", false));
        s.apply(start("3", "python -m http.server 8000", false));
        let t = session_targets(&s);
        assert_eq!(t.ports.iter().map(|(p, _)| *p).collect::<Vec<_>>(), vec![5173, 8000]);
        assert_eq!(t.needles.len(), 2, "the test command is not a server");
        assert_eq!(t.needles[0].0, "npm run dev -- --port 5173");
    }
}
