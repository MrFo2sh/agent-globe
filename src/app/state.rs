//! App-side state and the system that drains the bridge once per frame.

use agent_globe::bridge::{Bridge, ToApp, ToWorker};
use agent_globe::data::claude::SessionRef;
use agent_globe::data::model::{Event, Session};
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Resource)]
pub struct BridgeRes(pub Bridge);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Spawn,
    Done,
    Error,
    Info,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub born: Instant,
}

#[derive(Resource)]
pub struct Store {
    pub claude_dir: PathBuf,
    pub http_port: Option<u16>,
    pub session_list: Vec<SessionRef>,
    pub sessions: HashMap<String, Session>,
    pub current: Option<String>,
    pub selected: Option<String>,
    pub hovered: Option<String>,
    pub status: String,
    pub errors: Vec<String>,
    pub toasts: Vec<Toast>,
    pub events_total: u64,
    pub layout_dirty: bool,
    /// True while the mouse is over an egui panel or window; the camera ignores input then.
    pub pointer_over_ui: bool,
    pub show_settings: bool,
    /// Set by the UI; the camera system consumes it.
    pub reset_view: bool,
    /// Tool-call ids whose output is expanded past the 10-line preview.
    pub expanded_calls: HashSet<String>,
    /// Panel sizes in logical points: (left, top, right, bottom). The 3D camera renders between them.
    pub scene_insets: Option<(f32, f32, f32, f32)>,
    preferred: Option<String>,
    opened_once: bool,
    /// When the current session was opened; batches in the first moments are history, not news.
    opened_at: Instant,
}

const HISTORY_GRACE_S: f32 = 2.0;

impl Store {
    pub fn new(preferred: Option<String>, claude_dir: PathBuf, http_port: Option<u16>) -> Self {
        Store {
            claude_dir,
            http_port,
            session_list: Vec::new(),
            sessions: HashMap::new(),
            current: None,
            selected: None,
            hovered: None,
            status: "starting".into(),
            errors: Vec::new(),
            toasts: Vec::new(),
            events_total: 0,
            layout_dirty: true,
            pointer_over_ui: false,
            show_settings: false,
            reset_view: false,
            expanded_calls: HashSet::new(),
            scene_insets: None,
            preferred,
            opened_once: false,
            opened_at: Instant::now(),
        }
    }

    pub fn current_session(&self) -> Option<&Session> {
        self.current.as_ref().and_then(|id| self.sessions.get(id))
    }

    pub fn open(&mut self, r: &SessionRef, bridge: &Bridge) {
        self.sessions.remove(&r.id);
        self.current = Some(r.id.clone());
        self.selected = None;
        self.hovered = None;
        self.layout_dirty = true;
        self.opened_once = true;
        self.opened_at = Instant::now();
        self.toasts.clear();
        let _ = bridge.to_worker.send(ToWorker::Open(r.clone()));
    }

    /// Show a session that only exists through HTTP ingest (no file behind it).
    pub fn show(&mut self, id: &str) {
        self.current = Some(id.to_string());
        self.selected = None;
        self.hovered = None;
        self.layout_dirty = true;
        self.opened_at = Instant::now();
    }

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toasts.push(Toast {
            text: text.into(),
            kind,
            born: Instant::now(),
        });
        if self.toasts.len() > 8 {
            self.toasts.remove(0);
        }
    }

    fn push_error(&mut self, e: String) {
        self.errors.push(e);
        if self.errors.len() > 50 {
            self.errors.remove(0);
        }
    }

    fn is_live(&self) -> bool {
        self.opened_at.elapsed().as_secs_f32() > HISTORY_GRACE_S
    }
}

fn toast_for(store: &Store, ev: &Event) -> Option<(ToastKind, String)> {
    if ev.session_id() != store.current.as_deref()? {
        return None;
    }
    let session = store.current_session()?;
    match ev {
        Event::AgentSpawn { name, kind, .. } => {
            let label = if name.is_empty() { kind.clone() } else { name.clone() };
            Some((ToastKind::Spawn, format!("+ agent spawned: {label}")))
        }
        Event::AgentEnd { agent_id, .. } => {
            let name = session.agent(agent_id).map(|a| a.name.clone()).unwrap_or_default();
            Some((ToastKind::Done, format!("✔ agent finished: {name}")))
        }
        Event::ToolEnd {
            call_id,
            is_error: true,
            ..
        } => {
            let tool = session
                .agents
                .iter()
                .flat_map(|a| a.calls.iter())
                .find(|c| &c.id == call_id)
                .map(|c| c.tool.clone())
                .unwrap_or_else(|| "tool".into());
            Some((ToastKind::Error, format!("✖ {tool} failed")))
        }
        _ => None,
    }
}

pub fn drain_bridge(mut store: ResMut<Store>, bridge: Res<BridgeRes>) {
    while let Ok(msg) = bridge.0.to_app.try_recv() {
        match msg {
            ToApp::Sessions(list) => {
                store.session_list = list;
                if !store.opened_once {
                    let pick = match &store.preferred {
                        Some(p) => store
                            .session_list
                            .iter()
                            .find(|s| s.id.starts_with(p.as_str()))
                            .cloned(),
                        None => store.session_list.first().cloned(),
                    };
                    if let Some(s) = pick {
                        store.open(&s, &bridge.0);
                    }
                }
            }
            ToApp::Events(events) => {
                store.events_total += events.len() as u64;
                let live = store.is_live();
                for ev in events {
                    let toast = if live { toast_for(&store, &ev) } else { None };
                    let sid = ev.session_id().to_string();
                    let session = store
                        .sessions
                        .entry(sid.clone())
                        .or_insert_with(|| Session::new(sid, "", 0));
                    session.apply(ev);
                    if let Some((kind, text)) = toast {
                        store.toast(kind, text);
                    }
                }
                if store.current.is_none() {
                    let first = store.sessions.keys().next().cloned();
                    store.current = first;
                }
                store.layout_dirty = true;
            }
            ToApp::Status(s) => {
                store.toast(ToastKind::Info, s.clone());
                store.status = s;
            }
            ToApp::Error(e) => store.push_error(e),
        }
    }
    let cutoff = std::time::Duration::from_secs(7);
    store.toasts.retain(|t| t.born.elapsed() < cutoff);
}
