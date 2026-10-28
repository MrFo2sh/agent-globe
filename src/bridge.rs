//! Worker threads and the channels that connect them to the app. No Bevy here.
//!
//! - scanner: lists sessions under the Claude dir every few seconds
//! - tailer: follows the currently opened session and its subagents, and relays decisions
//! - http: `POST /events` (event schema, one JSON object or newline-delimited),
//!   `POST /hooks/claude` (raw Claude Code hook payload), `POST /approvals` (a PreToolUse hook
//!   payload; the request blocks until the human decides in the app), `GET /health`

use crate::data::claude::summarize_tool;
use crate::data::claude::{SessionRef, Tailer, discover};
use crate::data::hooks::from_claude_hook;
use crate::data::model::{Event, ROOT_AGENT};
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// A tool call waiting for a human yes or no.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    pub id: u64,
    pub session_id: String,
    pub agent_id: String,
    pub tool: String,
    pub summary: String,
    pub command: Option<String>,
    pub cwd: String,
}

pub enum ToApp {
    Sessions(Vec<SessionRef>),
    Events(Vec<Event>),
    Approval(ApprovalRequest),
    Status(String),
    Error(String),
}

pub enum ToWorker {
    Open(SessionRef),
    /// Answer an approval: "allow", "deny", or "ask" (fall back to the editor's own prompt).
    Decide {
        id: u64,
        decision: String,
    },
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

type Pending = Arc<Mutex<HashMap<u64, Sender<String>>>>;

/// How long an approval request may wait for a click before the editor prompts instead.
const APPROVAL_WAIT: Duration = Duration::from_secs(15 * 60);

pub fn start(cfg: BridgeConfig) -> Bridge {
    let (app_tx, app_rx) = unbounded::<ToApp>();
    let (worker_tx, worker_rx) = unbounded::<ToWorker>();
    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

    spawn_scanner(cfg.claude_dir.clone(), app_tx.clone());
    spawn_tailer(worker_rx, app_tx.clone(), cfg.poll_interval, pending.clone());
    if let Some(port) = cfg.http_port {
        spawn_http(port, app_tx, pending);
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

fn handle_command(cmd: ToWorker, tailer: &mut Option<Tailer>, tx: &Sender<ToApp>, pending: &Pending) {
    match cmd {
        ToWorker::Open(s) => {
            let _ = tx.send(ToApp::Status(format!("opening {}", s.id)));
            *tailer = Some(Tailer::new(s));
        }
        ToWorker::Decide { id, decision } => {
            if let Some(reply) = pending.lock().ok().and_then(|mut p| p.remove(&id)) {
                let _ = reply.send(decision);
            }
        }
        ToWorker::Rescan => {}
    }
}

fn spawn_tailer(rx: Receiver<ToWorker>, tx: Sender<ToApp>, interval: Duration, pending: Pending) {
    thread::Builder::new()
        .name("tailer".into())
        .spawn(move || {
            let mut tailer: Option<Tailer> = None;
            loop {
                while let Ok(cmd) = rx.try_recv() {
                    handle_command(cmd, &mut tailer, &tx, &pending);
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
                    Ok(cmd) => handle_command(cmd, &mut tailer, &tx, &pending),
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
                }
            }
        })
        .expect("spawn tailer");
}

/// Hook output that Claude Code understands for a PreToolUse decision.
fn decision_json(decision: &str) -> String {
    match decision {
        "allow" | "deny" => format!(
            "{{\"hookSpecificOutput\":{{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"{decision}\",\"permissionDecisionReason\":\"Decided in Agent Globe\"}}}}"
        ),
        _ => "{}".to_string(),
    }
}

fn approval_from_hook(id: u64, v: &serde_json::Value) -> Option<ApprovalRequest> {
    let session_id = v["session_id"].as_str()?.to_string();
    let tool = v["tool_name"].as_str().unwrap_or("tool").to_string();
    let input = &v["tool_input"];
    Some(ApprovalRequest {
        id,
        session_id,
        agent_id: v["agent_id"].as_str().unwrap_or(ROOT_AGENT).to_string(),
        summary: summarize_tool(&tool, input),
        command: input["command"].as_str().map(String::from),
        cwd: v["cwd"].as_str().unwrap_or("").to_string(),
        tool,
    })
}

fn spawn_http(port: u16, tx: Sender<ToApp>, pending: Pending) {
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
            let counter = Arc::new(AtomicU64::new(1));
            for req in server.incoming_requests() {
                let (tx, pending, counter) = (tx.clone(), pending.clone(), counter.clone());
                // One thread per request: approvals block for minutes and must not stall ingest.
                thread::spawn(move || handle_request(req, &tx, &pending, &counter));
            }
        })
        .expect("spawn http");
}

fn handle_request(mut req: tiny_http::Request, tx: &Sender<ToApp>, pending: &Pending, counter: &AtomicU64) {
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
        ("POST", "/approvals") => match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(v) => {
                let id = counter.fetch_add(1, Ordering::Relaxed);
                match approval_from_hook(id, &v) {
                    Some(request) => {
                        let (reply_tx, reply_rx) = bounded::<String>(1);
                        if let Ok(mut p) = pending.lock() {
                            p.insert(id, reply_tx);
                        }
                        let _ = tx.send(ToApp::Approval(request));
                        let decision = reply_rx.recv_timeout(APPROVAL_WAIT).unwrap_or_else(|_| "ask".into());
                        if let Ok(mut p) = pending.lock() {
                            p.remove(&id);
                        }
                        (200, decision_json(&decision))
                    }
                    None => (200, "{}".to_string()),
                }
            }
            Err(e) => (400, format!("bad json: {e}")),
        },
        _ => (404, "not found".to_string()),
    };
    let resp = tiny_http::Response::from_string(reply).with_status_code(status);
    let _ = req.respond(resp);
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

    #[test]
    fn decision_json_shapes() {
        assert!(decision_json("allow").contains("\"permissionDecision\":\"allow\""));
        assert!(decision_json("deny").contains("\"permissionDecision\":\"deny\""));
        assert_eq!(decision_json("ask"), "{}");
        let v = serde_json::json!({"session_id":"s","tool_name":"Bash","tool_input":{"command":"rm -rf build","description":"clean"},"cwd":"D:/x"});
        let a = approval_from_hook(7, &v).unwrap();
        assert_eq!(a.command.as_deref(), Some("rm -rf build"));
        assert_eq!(a.summary, "clean");
        assert_eq!(a.agent_id, ROOT_AGENT);
    }
}
