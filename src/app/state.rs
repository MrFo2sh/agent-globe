//! App-side state, the system that drains the bridge once per frame, and the alarms
//! (loops, context bloat, stalls) that turn raw events into "look here" moments.

use super::settings::Settings;
use super::sounds::Sfx;
use super::theme;
use agent_globe::bridge::{Bridge, ToApp, ToWorker};
use agent_globe::data::claude::SessionRef;
use agent_globe::data::model::{Event, ROOT_AGENT, Session, now_millis};
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
    Warn,
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
    pub show_debrief: bool,
    /// Cinematic mode: no panels, full-window scene.
    pub hide_ui: bool,
    /// Set by the UI; the camera system consumes it.
    pub reset_view: bool,
    /// Tool-call ids whose output is expanded past the 10-line preview.
    pub expanded_calls: HashSet<String>,
    /// Panel sizes in logical points: (left, top, right, bottom). The 3D camera renders between them.
    pub scene_insets: Option<(f32, f32, f32, f32)>,
    /// Sounds queued this frame; the audio system drains them.
    pub sfx: Vec<Sfx>,
    /// Open tool-call ids that have been running longer than the stall threshold.
    pub stalled: HashSet<String>,
    /// One-shot alarm keys already raised (loops, context), so they fire once.
    alarmed: HashSet<String>,
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
            show_debrief: false,
            hide_ui: false,
            reset_view: false,
            expanded_calls: HashSet::new(),
            scene_insets: None,
            sfx: Vec::new(),
            stalled: HashSet::new(),
            alarmed: HashSet::new(),
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
        self.stalled.clear();
        self.alarmed.clear();
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

    pub fn play(&mut self, sfx: Sfx) {
        self.sfx.push(sfx);
    }

    /// Raise an alarm once per key: toast + sound.
    fn alarm(&mut self, key: String, text: String, sfx: Sfx) {
        if self.alarmed.insert(key) {
            self.toast(ToastKind::Warn, text);
            self.play(sfx);
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

/// Toast and sound for a live event in the current session.
fn react(store: &Store, ev: &Event) -> (Option<(ToastKind, String)>, Option<Sfx>) {
    let Some(current) = store.current.as_deref() else {
        return (None, None);
    };
    if ev.session_id() != current {
        return (None, None);
    }
    let Some(session) = store.current_session() else {
        return (None, None);
    };
    match ev {
        Event::Message { role, agent_id, .. } if role == "user" && agent_id == ROOT_AGENT => (None, Some(Sfx::Prompt)),
        Event::AgentSpawn { name, kind, .. } => {
            let label = if name.is_empty() { kind.clone() } else { name.clone() };
            (
                Some((ToastKind::Spawn, format!("+ unit deployed: {label}"))),
                Some(Sfx::Spawn),
            )
        }
        Event::AgentEnd { agent_id, .. } => {
            let name = session.agent(agent_id).map(|a| a.name.clone()).unwrap_or_default();
            (
                Some((ToastKind::Done, format!("✔ mission accomplished: {name}"))),
                Some(Sfx::Done),
            )
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
            (Some((ToastKind::Error, format!("✖ {tool} failed"))), Some(Sfx::Error))
        }
        Event::TurnEnd { agent_id, .. } if agent_id == ROOT_AGENT => (
            Some((ToastKind::Done, "✔ turn complete: waiting for you".into())),
            Some(Sfx::Complete),
        ),
        Event::ToolStart { .. } => (None, Some(Sfx::Blip)),
        _ => (None, None),
    }
}

/// Pattern alarms that teach: loops and context bloat. Called after the event was applied.
fn check_patterns(store: &mut Store, ev: &Event) {
    match ev {
        Event::ToolStart {
            agent_id,
            command: Some(cmd),
            ..
        } => {
            let repeats = store
                .current_session()
                .and_then(|s| s.agent(agent_id))
                .map(|a| {
                    a.calls
                        .iter()
                        .filter(|c| c.command.as_deref() == Some(cmd.as_str()))
                        .count()
                })
                .unwrap_or(0);
            if repeats >= 3 {
                let short: String = cmd.lines().next().unwrap_or("").chars().take(60).collect();
                store.alarm(
                    format!("loop:{agent_id}:{cmd}"),
                    format!(
                        "⟳ LOOP: same command {repeats}× — give the agent the error output or a constraint: {short}"
                    ),
                    Sfx::Loop,
                );
            }
        }
        Event::Message {
            agent_id,
            context_tokens,
            model,
            ..
        } if *context_tokens >= theme::context_alarm_tokens(model) => {
            store.alarm(
                format!("ctx:{agent_id}"),
                format!(
                    "▲ CONTEXT {}k tokens — near this model's limit; split the task or start a fresh session",
                    context_tokens / 1000
                ),
                Sfx::Alarm,
            );
        }
        _ => {}
    }
}

pub fn drain_bridge(mut store: ResMut<Store>, bridge: Res<BridgeRes>, settings: Res<Settings>) {
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
                    let (toast, sfx) = if live { react(&store, &ev) } else { (None, None) };
                    let sid = ev.session_id().to_string();
                    let session = store
                        .sessions
                        .entry(sid.clone())
                        .or_insert_with(|| Session::new(sid, "", 0));
                    session.apply(ev.clone());
                    if let Some((kind, text)) = toast {
                        store.toast(kind, text);
                    }
                    if let Some(s) = sfx
                        && (s != Sfx::Blip || settings.tool_blips)
                    {
                        store.play(s);
                    }
                    if live && Some(ev.session_id()) == store.current.as_deref() {
                        check_patterns(&mut store, &ev);
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

/// Once a second: flag open tool calls that have run past the stall threshold.
pub fn watch_stalls(time: Res<Time>, mut store: ResMut<Store>, mut next: Local<f32>) {
    let t = time.elapsed_secs();
    if t < *next {
        return;
    }
    *next = t + 1.0;
    let now = now_millis();
    let mut newly: Vec<(String, String, i64)> = Vec::new();
    let mut open: HashSet<String> = HashSet::new();
    if let Some(session) = store.current_session() {
        for a in &session.agents {
            for c in a.calls.iter().filter(|c| c.ended.is_none() && c.started > 0) {
                open.insert(c.id.clone());
                if now - c.started > theme::STALL_AFTER_MS && !store.stalled.contains(&c.id) {
                    newly.push((c.id.clone(), c.tool.clone(), now - c.started));
                }
            }
        }
    }
    store.stalled.retain(|id| open.contains(id));
    for (id, tool, ms) in newly {
        store.stalled.insert(id);
        store.toast(
            ToastKind::Warn,
            format!(
                "⏳ STALL: {tool} has been running {}s — check for a hung command or a prompt waiting for you",
                ms / 1000
            ),
        );
        store.play(Sfx::Stall);
    }
}
