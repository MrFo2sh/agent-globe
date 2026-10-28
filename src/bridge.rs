//! Worker threads and the channels that connect them to the app. No Bevy here.
//!
//! - scanner: lists sessions under the Claude dir every few seconds
//! - tailer: follows the currently opened session and its subagents
//! - http: `POST /events` (event schema, one JSON object or newline-delimited),
//!   `POST /hooks/claude` (raw Claude Code hook payload), `GET /health`

use crate::data::claude::{SessionRef, Tailer, discover};
use crate::data::hooks::from_claude_hook;
use crate::data::model::Event;
use crossbeam_channel::{Receiver, Sender, unbounded};
use std::io::Read;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

pub enum ToApp {
    Sessions(Vec<SessionRef>),
    Events(Vec<Event>),
    Status(String),
    Error(String),
}

pub enum ToWorker {
    Open(SessionRef),
    Rescan,
}

pub struct BridgeConfig {
    pub claude_dir: PathBuf,
    pub http_port: Option<u16>,
    pub poll_interval: Duration,
}

pub struct Bridge {
    pub to_app: Receiver<ToApp>,
    pub to_worker: Sender<ToWorker>,
}

pub fn start(cfg: BridgeConfig) -> Bridge {
    let (app_tx, app_rx) = unbounded::<ToApp>();
    let (worker_tx, worker_rx) = unbounded::<ToWorker>();

    spawn_scanner(cfg.claude_dir.clone(), app_tx.clone());
    spawn_tailer(worker_rx, app_tx.clone(), cfg.poll_interval);
    if let Some(port) = cfg.http_port {
        spawn_http(port, app_tx);
    }
    Bridge {
        to_app: app_rx,
        to_worker: worker_tx,
    }
}

fn spawn_scanner(claude_dir: PathBuf, tx: Sender<ToApp>) {
    thread::Builder::new()
        .name("scanner".into())
        .spawn(move || {
            let mut last: Vec<SessionRef> = Vec::new();
            loop {
                let found = discover(&claude_dir);
                if found != last {
                    last = found.clone();
                    if tx.send(ToApp::Sessions(found)).is_err() {
                        return;
                    }
                }
                thread::sleep(Duration::from_secs(3));
            }
        })
        .expect("spawn scanner");
}

fn spawn_tailer(rx: Receiver<ToWorker>, tx: Sender<ToApp>, interval: Duration) {
    thread::Builder::new()
        .name("tailer".into())
        .spawn(move || {
            let mut tailer: Option<Tailer> = None;
            loop {
                // Drain commands; the last Open wins.
                while let Ok(cmd) = rx.try_recv() {
                    match cmd {
                        ToWorker::Open(s) => {
                            let _ = tx.send(ToApp::Status(format!("opening {}", s.id)));
                            tailer = Some(Tailer::new(s));
                        }
                        ToWorker::Rescan => {}
                    }
                }
                if let Some(t) = tailer.as_mut() {
                    let events = t.poll();
                    if !events.is_empty() && tx.send(ToApp::Events(events)).is_err() {
                        return;
                    }
                    if t.parse_errors > 0 {
                        let n = t.parse_errors;
                        t.parse_errors = 0;
                        let _ = tx.send(ToApp::Error(format!("{n} unparseable line(s) in {}", t.session_id())));
                    }
                }
                match rx.recv_timeout(interval) {
                    Ok(cmd) => {
                        if let ToWorker::Open(s) = cmd {
                            let _ = tx.send(ToApp::Status(format!("opening {}", s.id)));
                            tailer = Some(Tailer::new(s));
                        }
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
                }
            }
        })
        .expect("spawn tailer");
}

fn spawn_http(port: u16, tx: Sender<ToApp>) {
    thread::Builder::new()
        .name("http".into())
        .spawn(move || {
            let server = match tiny_http::Server::http(("127.0.0.1", port)) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(ToApp::Error(format!("ingest http disabled: {e}")));
                    return;
                }
            };
            let _ = tx.send(ToApp::Status(format!("ingest listening on 127.0.0.1:{port}")));
            for mut req in server.incoming_requests() {
                let url = req.url().to_string();
                let method = req.method().as_str().to_string();
                let mut body = String::new();
                let _ = req.as_reader().take(4 * 1024 * 1024).read_to_string(&mut body);
                let (status, reply) = match (method.as_str(), url.as_str()) {
                    ("GET", "/health") => (200, "ok".to_string()),
                    ("POST", "/events") => match parse_events(&body) {
                        Ok(events) => {
                            let n = events.len();
                            let _ = tx.send(ToApp::Events(events));
                            (200, format!("accepted {n}"))
                        }
                        Err(e) => (400, e),
                    },
                    ("POST", "/hooks/claude") => match serde_json::from_str::<serde_json::Value>(&body) {
                        Ok(v) => match from_claude_hook(&v) {
                            Some(ev) => {
                                let _ = tx.send(ToApp::Events(vec![ev]));
                                (200, "accepted 1".to_string())
                            }
                            None => (200, "ignored".to_string()),
                        },
                        Err(e) => (400, format!("bad json: {e}")),
                    },
                    _ => (404, "not found".to_string()),
                };
                let resp = tiny_http::Response::from_string(reply).with_status_code(status);
                let _ = req.respond(resp);
            }
        })
        .expect("spawn http");
}

/// One JSON object, a JSON array, or newline-delimited objects.
pub fn parse_events(body: &str) -> Result<Vec<Event>, String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err("empty body".into());
    }
    if trimmed.starts_with('[') {
        return serde_json::from_str::<Vec<Event>>(trimmed).map_err(|e| format!("bad array: {e}"));
    }
    if let Ok(one) = serde_json::from_str::<Event>(trimmed) {
        return Ok(vec![one]);
    }
    let mut out = Vec::new();
    for (i, line) in trimmed.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let ev = serde_json::from_str::<Event>(line).map_err(|e| format!("line {}: {e}", i + 1))?;
        out.push(ev);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_array_and_ndjson() {
        let one = r#"{"type":"agent_end","session_id":"s","agent_id":"a"}"#;
        assert_eq!(parse_events(one).unwrap().len(), 1);
        let arr = format!("[{one},{one}]");
        assert_eq!(parse_events(&arr).unwrap().len(), 2);
        let nd = format!("{one}\n{one}\n{one}\n");
        assert_eq!(parse_events(&nd).unwrap().len(), 3);
        assert!(parse_events("nope").is_err());
    }
}
