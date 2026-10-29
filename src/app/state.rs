//! App-side state, the system that drains the bridge once per frame, and the alarms
//! (loops, context bloat, stalls) that turn raw events into "look here" moments.

use super::files::DocKind;
use super::settings::Settings;
use super::sounds::Sfx;
use super::theme;
use agent_globe::bridge::{ApprovalRequest, Bridge, ToApp, ToWorker};
use agent_globe::data::claude::SessionRef;
use agent_globe::data::model::{Event, ROOT_AGENT, Session, now_millis, snippet};
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadgeKind {
    /// A unit finished its mission (glowing lamp) until acknowledged.
    Done,
    /// The commander is waiting for the human.
    Waiting,
    /// A tool call failed, until acknowledged.
    Error,
    /// A tool call is blocked on your yes or no.
    Approval,
    /// A tool call has run past the stall threshold.
    Stalled,
}

/// One thing that needs the human, in any watched session.
#[derive(Clone, Debug)]
pub struct Attention {
    pub session_id: String,
    pub project: String,
    pub agent_id: String,
    pub kind: BadgeKind,
    pub since: i64,
    pub label: String,
}

/// Screen position of another session's commander, for its floating label.
#[derive(Clone, Debug)]
pub struct Satellite {
    pub session_id: String,
    pub x: f32,
    pub y: f32,
}

/// What a tool call is for, as far as the radio cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallKind {
    Scout,
    Dig,
    Intel,
    Test,
    Commit,
    Other,
}

/// Classify a call by its tool and command: reads and searches scout, edits dig, web calls gather
/// intel, build/test/check commands test the line, `git commit` fortifies.
pub fn call_kind(tool: &str, command: Option<&str>) -> CallKind {
    match tool {
        "Read" | "Grep" | "Glob" => return CallKind::Scout,
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => return CallKind::Dig,
        "WebFetch" | "WebSearch" => return CallKind::Intel,
        _ => {}
    }
    let Some(cmd) = command else { return CallKind::Other };
    let c = cmd.to_ascii_lowercase();
    if c.contains("git commit") {
        return CallKind::Commit;
    }
    const TESTS: [&str; 14] = [
        "cargo test",
        "cargo clippy",
        "cargo check",
        "cargo build",
        "npm test",
        "npm run build",
        "npm run lint",
        "npm run typecheck",
        "pnpm test",
        "pytest",
        "go test",
        "dotnet test",
        "docker build",
        "factory.mjs check",
    ];
    if TESTS.iter().any(|t| c.contains(t)) {
        return CallKind::Test;
    }
    CallKind::Other
}

/// Did output of a test-like command report a failure even though the exit code did not?
pub fn output_says_failed(stdout: &str, stderr: &str) -> bool {
    let text = format!("{stdout}\n{stderr}");
    text.contains("CHECK FAILED")
        || text.contains("test result: FAILED")
        || text.contains("error[E")
        || text.contains(" failed, ")
        || text.contains("FAIL ")
}

/// `hh:mm` local time of a unix-millisecond timestamp.
pub fn clock_hm(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_else(|| "?".into())
}

/// Which sessions to tail: the current one plus the most recently modified ones still moving.
/// How many previously viewed constellations stay around as satellites after you leave them.
pub const PINNED_MAX: usize = 3;

pub fn desired_watch(list: &[SessionRef], current: Option<&str>, pinned: &[String], now: i64) -> Vec<SessionRef> {
    let mut out: Vec<SessionRef> = Vec::new();
    if let Some(c) = current
        && let Some(s) = list.iter().find(|s| s.id == c)
    {
        out.push(s.clone());
    }
    // The constellations you came from stay watched, so you can always go back.
    for id in pinned {
        if out.iter().any(|s| &s.id == id) {
            continue;
        }
        if let Some(s) = list.iter().find(|s| &s.id == id) {
            out.push(s.clone());
        }
    }
    let mut recent: Vec<&SessionRef> = list
        .iter()
        .filter(|s| now - s.modified < theme::WATCH_RECENT_MS && !out.iter().any(|o| o.id == s.id))
        .collect();
    recent.sort_by_key(|s| std::cmp::Reverse(s.modified));
    let room = theme::WATCH_MAX.saturating_sub(out.len());
    out.extend(recent.into_iter().take(room).cloned());
    out
}

/// Images of one unit, browsable with previous / next.
#[derive(Clone, Debug)]
pub struct Viewer {
    /// Paths of everything viewable in-app for the unit: images, markdown, pdf.
    pub items: Vec<String>,
    pub index: usize,
    pub zoom: f32,
    /// Scroll offset of the content, driven by dragging.
    pub offset: (f32, f32),
}

#[derive(Clone, Debug)]
pub struct Badge {
    pub session_id: String,
    pub agent_id: String,
    pub kind: BadgeKind,
    /// Window logical position of the point above the unit.
    pub x: f32,
    pub y: f32,
}

/// What a constellation shows: the units and the whole file system, or one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SceneView {
    #[default]
    Both,
    Files,
    Agents,
}

impl SceneView {
    pub fn next(self) -> SceneView {
        match self {
            SceneView::Both => SceneView::Files,
            SceneView::Files => SceneView::Agents,
            SceneView::Agents => SceneView::Both,
        }
    }
}

/// A file open in the editor card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    pub path: String,
    pub kind: DocKind,
    pub dirty: bool,
    /// A preview tab (single click): the next single click reuses it. Double-click or an edit pins it.
    pub preview: bool,
}

/// What the UI asks the editor page to do; `webview.rs` carries it out on the main thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorCmd {
    Open {
        path: String,
    },
    Mode {
        preview: bool,
    },
    Save,
    Close {
        path: String,
    },
    /// Font steps for the page: +1 / -1, or a jump back to the default.
    Zoom(i32),
}

/// A card's floating position (None: its default), whether its title is being dragged, and its
/// own text zoom.
#[derive(Debug, Clone)]
pub struct CardState {
    pub pos: Option<[f32; 2]>,
    pub dragging: bool,
    pub zoom: f32,
}

impl Default for CardState {
    fn default() -> Self {
        CardState {
            pos: None,
            dragging: false,
            zoom: 1.0,
        }
    }
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
    /// A satellite constellation under the pointer (another session's unit).
    pub hovered_session: Option<String>,
    /// Session ids currently tailed by the worker.
    pub watched: Vec<String>,
    /// Constellations you viewed before the current one, newest first; they stay as satellites.
    pub pinned: Vec<String>,
    /// When each session's first event arrived; bursts right after are history, not news.
    seen: HashMap<String, Instant>,
    /// Set by the scene: switch to this session (by id) on the next frame.
    pub pending_open: Option<String>,
    /// Agent to select once the pending session is open.
    pub pending_select: Option<String>,
    /// Commanders of other shown sessions, in window logical coordinates.
    pub satellites: Vec<Satellite>,
    /// File nodes of the current session on screen: (path, label, x, y, reads, edits).
    pub file_screen: Vec<(String, String, f32, f32, u32, u32)>,
    /// A file node was clicked: open it in the editor on the next frame.
    pub pending_open_file: Option<String>,
    pub status: String,
    pub errors: Vec<String>,
    pub toasts: Vec<Toast>,
    pub events_total: u64,
    pub layout_dirty: bool,
    /// True while the mouse is over an egui panel or window; the camera ignores input then.
    pub pointer_over_ui: bool,
    pub show_settings: bool,
    /// The plan-usage window (session, weekly, per-model limits).
    pub show_usage: bool,
    /// One-frame request from the "fold all" button: collapse every prompt group.
    pub fold_groups: Option<bool>,
    /// The reply card: (session, agent) whose last message is open for reading.
    pub reply_open: Option<(String, String)>,
    /// The reply card fills the screen (⛶) instead of floating next to the unit.
    pub reply_max: bool,
    /// The integrated terminal: the session whose shell is open.
    pub terminal_open: Option<String>,
    pub terminal_max: bool,
    pub settings_max: bool,
    /// A prompt group opened as a card: (session, agent, turn index).
    pub turn_open: Option<(String, String, usize)>,
    pub turn_max: bool,
    /// 0 = conversation, 1 = details.
    pub turn_tab: u8,
    /// Search text of each card, kept while the card is open.
    pub reply_query: String,
    pub terminal_query: String,
    pub turn_query: String,
    /// Per-card floating position and drag state, by card id.
    pub cards: HashMap<String, CardState>,
    /// The files card: the session folder as a tree plus the finder.
    /// The files sidebar is expanded (collapsed: a strip with the 🗂 button).
    pub files_open: bool,
    pub files_query: String,
    pub files_focus_search: bool,
    pub files_open_dirs: HashSet<String>,
    /// The editor card: open documents, the active one, what to tell the page next.
    pub editor_open: bool,
    pub editor_max: bool,
    pub docs: Vec<Doc>,
    pub active_doc: Option<usize>,
    pub editor_cmds: Vec<EditorCmd>,
    /// Markdown opens rendered (true) or as source.
    pub md_preview: bool,
    /// The quick-open palette (Ctrl+P): its query, selection, and a pending focus request.
    pub quick_open: bool,
    pub quick_query: String,
    pub quick_sel: usize,
    pub quick_focus: bool,
    /// The card under the pointer this frame, by id: Ctrl+plus/minus act on it, not the whole UI.
    pub hot_card: Option<String>,
    /// A zoom step (+1 / -1 / 0 = reset) asked for the card under the pointer.
    pub zoom_request: Option<(String, i32)>,
    /// Terminal font size in points.
    pub term_font: f32,
    /// Where the monitors meet, in window points, left to right.
    pub monitor_seams: Vec<f32>,
    /// Units and files, files only, or units only.
    pub scene_view: SceneView,
    /// The file node under the pointer (its label shows even when it is not hot).
    pub hovered_file: Option<String>,
    /// A file just opened or selected: the camera glides to its node once.
    pub focus_file: Option<String>,
    /// The file picked with one click: lit, its connections shown; a second click opens it.
    pub selected_file: Option<String>,
    /// A folder opened by hand: the file system to show when no session is on screen.
    pub workspace: Option<String>,
    /// Ask the host to hand the keyboard back to the app window (after the page asked for it).
    pub focus_app: bool,
    /// Where the WebView surface goes this frame, in physical pixels (x, y, w, h); None hides it.
    pub webview_rect: Option<[f32; 4]>,
    pub webview_logged: bool,
    /// An egui text field has the keyboard: scene shortcuts (R, Esc) stay out of the way.
    pub typing: bool,
    /// Screen rects of the open cards this frame (min x, min y, max x, max y): badges under
    /// them are not drawn.
    pub card_rects: Vec<[f32; 4]>,
    /// A ✕ was clicked once on this pid; a second click within three seconds ends the process.
    pub kill_armed: Option<(u32, std::time::Instant)>,
    /// Container id armed for a kill (first click), like kill_armed but for Docker.
    pub docker_armed: Option<(String, std::time::Instant)>,
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
    /// Badges the user has dismissed by clicking the unit: "done:<agent>", "err:<agent>".
    pub acked: HashSet<String>,
    /// Badges to draw this frame, in window logical coordinates.
    pub badges: Vec<Badge>,
    /// Where every unit of the current session sits on screen this frame (logical coords).
    pub unit_screen: HashMap<String, (f32, f32)>,
    /// Tool calls waiting for a human decision, oldest first.
    pub approvals: Vec<ApprovalRequest>,
    /// Artifact targets the user dismissed from the speech bubbles.
    pub dismissed_bubbles: HashSet<String>,
    pub tray_mood: Option<super::alerts::Mood>,
    /// The in-app viewer, when open.
    pub viewer: Option<Viewer>,
    /// Filter text for the expanded artifact shelf.
    pub artifact_query: String,
    /// Filter text for the selected unit's tool-call history.
    pub history_query: String,
    /// In the compact layout the sidebar collapses to a strip unless this is set.
    pub sidebar_open: bool,
    preferred: Option<String>,
    opened_once: bool,
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
            hovered_session: None,
            watched: Vec::new(),
            pinned: Vec::new(),
            seen: HashMap::new(),
            pending_open: None,
            pending_select: None,
            satellites: Vec::new(),
            file_screen: Vec::new(),
            pending_open_file: None,
            status: "starting".into(),
            errors: Vec::new(),
            toasts: Vec::new(),
            events_total: 0,
            layout_dirty: true,
            pointer_over_ui: false,
            show_settings: false,
            show_usage: false,
            fold_groups: None,
            kill_armed: None,
            docker_armed: None,
            reply_open: None,
            reply_max: false,
            terminal_open: None,
            terminal_max: false,
            settings_max: false,
            turn_open: None,
            turn_max: false,
            turn_tab: 0,
            reply_query: String::new(),
            terminal_query: String::new(),
            turn_query: String::new(),
            cards: HashMap::new(),
            files_open: false,
            files_query: String::new(),
            files_focus_search: false,
            files_open_dirs: HashSet::new(),
            editor_open: false,
            editor_max: false,
            docs: Vec::new(),
            active_doc: None,
            editor_cmds: Vec::new(),
            md_preview: true,
            quick_open: false,
            quick_query: String::new(),
            quick_sel: 0,
            quick_focus: false,
            hot_card: None,
            zoom_request: None,
            term_font: 13.0,
            monitor_seams: Vec::new(),
            scene_view: SceneView::Both,
            hovered_file: None,
            focus_file: None,
            selected_file: None,
            workspace: None,
            focus_app: false,
            webview_rect: None,
            webview_logged: false,
            typing: false,
            card_rects: Vec::new(),
            hide_ui: false,
            reset_view: false,
            expanded_calls: HashSet::new(),
            scene_insets: None,
            sfx: Vec::new(),
            stalled: HashSet::new(),
            alarmed: HashSet::new(),
            acked: HashSet::new(),
            badges: Vec::new(),
            unit_screen: HashMap::new(),
            approvals: Vec::new(),
            dismissed_bubbles: HashSet::new(),
            tray_mood: None,
            viewer: None,
            artifact_query: String::new(),
            history_query: String::new(),
            sidebar_open: false,
            preferred,
            opened_once: false,
        }
    }

    /// The latest throttle whose reset time is still ahead, if any.
    pub fn throttled_until(&self, now: i64) -> Option<i64> {
        self.sessions
            .values()
            .flat_map(|s| s.throttles.iter())
            .map(|t| t.resets_at)
            .filter(|&r| r > now)
            .max()
    }

    /// Sessions drawn in the scene: the current one first, then satellites that moved recently
    /// or need the human, in a stable order.
    pub fn shown_sessions(&self, now: i64, satellites: bool) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(c) = &self.current
            && self.sessions.contains_key(c)
        {
            out.push(c.clone());
        }
        if satellites {
            let needy: HashSet<String> = self.attention(now).into_iter().map(|a| a.session_id).collect();
            let mut others: Vec<&Session> = self
                .sessions
                .values()
                .filter(|s| Some(&s.id) != self.current.as_ref())
                .filter(|s| {
                    now - s.last_activity < theme::SATELLITE_RECENT_MS
                        || needy.contains(&s.id)
                        || self.pinned.contains(&s.id)
                })
                .collect();
            others.sort_by(|a, b| a.id.cmp(&b.id));
            out.extend(others.into_iter().take(5).map(|s| s.id.clone()));
        }
        out
    }

    /// Everything waiting on the human across all watched sessions: approvals first, then
    /// failures, then commanders waiting for orders, then stalls; newest first within a kind.
    pub fn attention(&self, now: i64) -> Vec<Attention> {
        let mut out: Vec<Attention> = Vec::new();
        for s in self.sessions.values() {
            for a in self.approvals.iter().filter(|a| a.session_id == s.id) {
                out.push(Attention {
                    session_id: s.id.clone(),
                    project: s.project.clone(),
                    agent_id: a.agent_id.clone(),
                    kind: BadgeKind::Approval,
                    since: now,
                    label: format!("{} wants {}", a.tool, a.summary),
                });
            }
            for agent in &s.agents {
                if agent.last_call_failed() && !self.acked.contains(&format!("err:{}:{}", s.id, agent.id)) {
                    let c = agent
                        .calls
                        .iter()
                        .filter(|c| c.ended.is_some())
                        .max_by_key(|c| c.ended.unwrap_or(0));
                    out.push(Attention {
                        session_id: s.id.clone(),
                        project: s.project.clone(),
                        agent_id: agent.id.clone(),
                        kind: BadgeKind::Error,
                        since: c.and_then(|c| c.ended).unwrap_or(agent.last_activity),
                        label: format!("{} failed", c.map(|c| c.tool.as_str()).unwrap_or("call")),
                    });
                }
                if agent.depth == 0 && agent.waiting_for_user {
                    out.push(Attention {
                        session_id: s.id.clone(),
                        project: s.project.clone(),
                        agent_id: agent.id.clone(),
                        kind: BadgeKind::Waiting,
                        since: s.last_activity,
                        label: if agent.last_said.trim().is_empty() {
                            "awaiting your orders".into()
                        } else {
                            snippet(&agent.last_said, 90)
                        },
                    });
                }
                for c in agent
                    .calls
                    .iter()
                    .filter(|c| c.ended.is_none() && self.stalled.contains(&c.id))
                {
                    out.push(Attention {
                        session_id: s.id.clone(),
                        project: s.project.clone(),
                        agent_id: agent.id.clone(),
                        kind: BadgeKind::Stalled,
                        since: c.started,
                        label: format!("{} stalled", c.tool),
                    });
                }
            }
        }
        let rank = |k: BadgeKind| match k {
            BadgeKind::Approval => 0,
            BadgeKind::Error => 1,
            BadgeKind::Waiting => 2,
            BadgeKind::Stalled => 3,
            BadgeKind::Done => 4,
        };
        out.sort_by(|a, b| rank(a.kind).cmp(&rank(b.kind)).then(b.since.cmp(&a.since)));
        out
    }

    /// Keep the worker tailing exactly the sessions worth watching.
    fn sync_watch(&mut self, bridge: &Bridge, now: i64) {
        let desired = desired_watch(&self.session_list, self.current.as_deref(), &self.pinned, now);
        let mut ids: Vec<String> = desired.iter().map(|s| s.id.clone()).collect();
        ids.sort();
        let mut have = self.watched.clone();
        have.sort();
        if ids == have {
            return;
        }
        for gone in have.iter().filter(|id| !ids.contains(id)) {
            self.sessions.remove(gone);
            self.seen.remove(gone);
        }
        self.watched = ids;
        let _ = bridge.to_worker.send(ToWorker::Watch(desired));
    }

    fn is_live_for(&self, session_id: &str) -> bool {
        self.seen
            .get(session_id)
            .is_some_and(|t| t.elapsed().as_secs_f32() > HISTORY_GRACE_S)
    }

    /// Open a file in the editor card as a preview tab: focus it if it is already open, reuse
    /// the current preview tab if there is one, otherwise add a tab.
    pub fn open_doc(&mut self, path: &str) {
        self.open_doc_as(path, true);
    }

    /// Open a file pinned (double-click): it keeps its tab.
    pub fn open_doc_pinned(&mut self, path: &str) {
        self.open_doc_as(path, false);
    }

    fn open_doc_as(&mut self, path: &str, preview: bool) {
        let path = path.replace('/', "\\");
        if let Some(i) = self.docs.iter().position(|d| d.path == path) {
            self.active_doc = Some(i);
            if !preview {
                self.docs[i].preview = false;
            }
        } else if preview && let Some(i) = self.docs.iter().position(|d| d.preview && !d.dirty) {
            let old = std::mem::replace(
                &mut self.docs[i],
                Doc {
                    path: path.clone(),
                    kind: super::files::kind_of(&path),
                    dirty: false,
                    preview: true,
                },
            );
            self.editor_cmds.push(EditorCmd::Close { path: old.path });
            self.active_doc = Some(i);
        } else {
            self.docs.push(Doc {
                path: path.clone(),
                kind: super::files::kind_of(&path),
                dirty: false,
                preview,
            });
            self.active_doc = Some(self.docs.len() - 1);
        }
        self.editor_open = true;
        info!("editor: open {path}");
        self.focus_file = Some(path.clone());
        self.editor_cmds.push(EditorCmd::Open { path });
    }

    /// The tab at `i` stops being a preview.
    pub fn pin_doc(&mut self, i: usize) {
        if let Some(d) = self.docs.get_mut(i) {
            d.preview = false;
        }
    }

    /// Close a tab; the neighbour becomes active.
    pub fn close_doc(&mut self, i: usize) {
        if i >= self.docs.len() {
            return;
        }
        let d = self.docs.remove(i);
        self.editor_cmds.push(EditorCmd::Close { path: d.path });
        self.active_doc = if self.docs.is_empty() {
            None
        } else {
            Some(i.min(self.docs.len() - 1))
        };
        if let Some(a) = self.active_doc
            && let Some(d) = self.docs.get(a)
        {
            let path = d.path.clone();
            self.editor_cmds.push(EditorCmd::Open { path });
        }
    }

    /// The folder the file system follows: the session's, else the one opened by hand.
    pub fn root_dir(&self) -> Option<String> {
        self.current_session()
            .map(|s| s.cwd.clone())
            .filter(|c| !c.is_empty())
            .or_else(|| self.workspace.clone())
    }

    pub fn current_session(&self) -> Option<&Session> {
        self.current.as_ref().and_then(|id| self.sessions.get(id))
    }

    fn pin_current(&mut self, next: &str) {
        if let Some(prev) = self.current.clone()
            && prev != next
        {
            self.pinned.retain(|p| p != &prev && p != next);
            self.pinned.insert(0, prev);
            self.pinned.truncate(PINNED_MAX);
        }
        self.pinned.retain(|p| p != next);
    }

    pub fn open(&mut self, r: &SessionRef, bridge: &Bridge) {
        let already = self.watched.iter().any(|w| w == &r.id) && self.sessions.contains_key(&r.id);
        self.pin_current(&r.id);
        self.current = Some(r.id.clone());
        self.selected = None;
        self.hovered = None;
        self.hovered_session = None;
        self.layout_dirty = true;
        self.opened_once = true;
        self.toasts.clear();
        self.alarmed.clear();
        if already {
            return;
        }
        self.sessions.remove(&r.id);
        self.seen.remove(&r.id);
        if !self.watched.contains(&r.id) {
            self.watched.push(r.id.clone());
        }
        let _ = bridge.to_worker.send(ToWorker::Open(r.clone()));
    }

    /// Show a session that only exists through HTTP ingest (no file behind it).
    pub fn show(&mut self, id: &str) {
        self.pin_current(id);
        self.current = Some(id.to_string());
        self.selected = None;
        self.hovered = None;
        self.hovered_session = None;
        self.layout_dirty = true;
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

    /// Answer an approval request and drop it from the queue.
    pub fn decide(&mut self, bridge: &Bridge, id: u64, decision: &str) {
        self.approvals.retain(|a| a.id != id);
        let _ = bridge.to_worker.send(ToWorker::Decide {
            id,
            decision: decision.to_string(),
        });
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
            is_error,
            stdout,
            stderr,
            ..
        } if session
            .agents
            .iter()
            .flat_map(|a| a.calls.iter())
            .find(|c| &c.id == call_id)
            .is_some_and(|c| {
                matches!(
                    call_kind(&c.tool, c.command.as_deref()),
                    CallKind::Test | CallKind::Commit
                )
            }) =>
        {
            let call = session
                .agents
                .iter()
                .flat_map(|a| a.calls.iter())
                .find(|c| &c.id == call_id)
                .expect("matched above");
            let kind = call_kind(&call.tool, call.command.as_deref());
            let failed = *is_error || (kind == CallKind::Test && output_says_failed(stdout, stderr));
            match (kind, failed) {
                (CallKind::Commit, false) => (Some((ToastKind::Done, "✔ committed".into())), Some(Sfx::Fortify)),
                (CallKind::Commit, true) => (Some((ToastKind::Error, "✖ commit failed".into())), Some(Sfx::Error)),
                (_, true) => (
                    Some((ToastKind::Error, "✖ tests or checks failed".into())),
                    Some(Sfx::Casualties),
                ),
                (_, false) => (
                    Some((ToastKind::Done, "✔ tests and checks passed".into())),
                    Some(Sfx::AllClear),
                ),
            }
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
        Event::TurnEnd { agent_id, .. } if agent_id == ROOT_AGENT => {
            let asked = session
                .agent(agent_id)
                .is_some_and(|a| a.last_said.trim_end().ends_with('?'));
            if asked {
                (
                    Some((ToastKind::Done, "💬 the commander asked you something".into())),
                    Some(Sfx::Question),
                )
            } else {
                (
                    Some((ToastKind::Done, "💬 turn complete: waiting for your orders".into())),
                    Some(Sfx::Waiting),
                )
            }
        }
        Event::Throttle { resets_at, .. } => (
            Some((
                ToastKind::Error,
                format!("⛔ plan limit reached · resets {}", clock_hm(*resets_at)),
            )),
            Some(Sfx::Ammo),
        ),
        Event::ToolEnd {
            call_id,
            is_error: false,
            ..
        } if session
            .agents
            .iter()
            .flat_map(|a| a.calls.iter())
            .any(|c| &c.id == call_id && c.command.as_deref().is_some_and(|cmd| cmd.contains("factory.mjs ship"))) =>
        {
            (Some((ToastKind::Done, "🏁 shipped".into())), Some(Sfx::Complete))
        }
        Event::ToolStart {
            agent_id,
            tool,
            command,
            ..
        } => {
            let kind = call_kind(tool, command.as_deref());
            let first = session
                .agent(agent_id)
                .map(|a| {
                    let range = a.turn_range(a.turn_log.len().saturating_sub(1));
                    !a.calls[range]
                        .iter()
                        .any(|c| call_kind(&c.tool, c.command.as_deref()) == kind)
                })
                .unwrap_or(true);
            let line = match kind {
                CallKind::Scout if first => Some(Sfx::Scout),
                CallKind::Dig if first => Some(Sfx::Dig),
                CallKind::Intel if first => Some(Sfx::Intel),
                CallKind::Test => Some(Sfx::Test),
                _ => None,
            };
            (None, line.or(Some(Sfx::Blip)))
        }
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
                Sfx::Supplies,
            );
        }
        _ => {}
    }
}

pub fn drain_bridge(mut store: ResMut<Store>, bridge: Res<BridgeRes>, settings: Res<Settings>) {
    let now = now_millis();
    if let Some(id) = store.pending_open.take() {
        if let Some(r) = store.session_list.iter().find(|s| s.id == id).cloned() {
            store.open(&r, &bridge.0);
        } else if store.sessions.contains_key(&id) {
            store.show(&id);
        }
        store.selected = store.pending_select.take();
        // Arriving at a constellation: the camera flies home to frame it.
        store.reset_view = true;
    }
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
                for ev in events {
                    let sid = ev.session_id().to_string();
                    let live = store.is_live_for(&sid);
                    store.seen.entry(sid.clone()).or_insert_with(Instant::now);
                    let is_current = store.current.as_deref() == Some(sid.as_str());
                    let (mut toast, mut sfx) = if live { react(&store, &ev) } else { (None, None) };
                    let project = store
                        .session_list
                        .iter()
                        .find(|s| s.id == sid)
                        .map(|s| s.project.clone())
                        .unwrap_or_else(|| sid.chars().take(8).collect());
                    let session = store
                        .sessions
                        .entry(sid.clone())
                        .or_insert_with(|| Session::new(sid.clone(), project.clone(), 0));
                    if session.project.is_empty() {
                        session.project = project.clone();
                    }
                    session.apply(ev.clone());
                    // Other sessions only speak up when they need the human.
                    if live && !is_current {
                        match &ev {
                            Event::TurnEnd { agent_id, .. } if agent_id == ROOT_AGENT => {
                                let asked = store
                                    .sessions
                                    .get(&sid)
                                    .and_then(|s| s.agent(agent_id))
                                    .is_some_and(|a| a.last_said.trim_end().ends_with('?'));
                                toast = Some((ToastKind::Done, format!("💬 {project}: awaiting your orders")));
                                sfx = Some(if asked { Sfx::Question } else { Sfx::Waiting });
                            }
                            Event::ToolEnd { is_error: true, .. } => {
                                toast = Some((ToastKind::Error, format!("✖ {project}: a call failed")));
                                sfx = Some(Sfx::Error);
                            }
                            Event::Throttle { resets_at, .. } => {
                                toast = Some((
                                    ToastKind::Error,
                                    format!("⛔ {project}: plan limit reached · resets {}", clock_hm(*resets_at)),
                                ));
                                sfx = Some(Sfx::Ammo);
                            }
                            _ => {}
                        }
                    }
                    if let Some((kind, text)) = toast {
                        store.toast(kind, text);
                    }
                    if let Some(s) = sfx
                        && (s != Sfx::Blip || settings.tool_blips)
                    {
                        store.play(s);
                    }
                    if live && is_current {
                        check_patterns(&mut store, &ev);
                    }
                }
                if store.current.is_none() {
                    let first = store.sessions.keys().next().cloned();
                    store.current = first;
                }
                store.layout_dirty = true;
            }
            ToApp::Approval(req) => {
                let label = req.command.clone().unwrap_or_else(|| req.summary.clone());
                let short: String = label.lines().next().unwrap_or("").chars().take(70).collect();
                store.toast(ToastKind::Warn, format!("🛡 authorization: {} wants {short}", req.tool));
                store.play(Sfx::Authorize);
                store.approvals.push(req);
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
    store.sync_watch(&bridge.0, now);
}

/// Seconds a text-only reply must stand alone before the turn counts as finished.
pub const SETTLE_GRACE_MS: i64 = 5_000;

/// Once a second: close turns the transcript never closed (see `Session::unsettled_turns`), with
/// the same toasts and radio lines a real turn end gets.
pub fn settle_turns(time: Res<Time>, mut store: ResMut<Store>, settings: Res<Settings>, mut next: Local<f32>) {
    let t = time.elapsed_secs();
    if t < *next {
        return;
    }
    *next = t + 1.0;
    let now = now_millis();
    let pending: Vec<Event> = store
        .sessions
        .values()
        .flat_map(|s| s.unsettled_turns(now, SETTLE_GRACE_MS))
        .collect();
    for ev in pending {
        let sid = ev.session_id().to_string();
        let live = store.is_live_for(&sid);
        let is_current = store.current.as_deref() == Some(sid.as_str());
        let (mut toast, mut sfx) = if live { react(&store, &ev) } else { (None, None) };
        if live && !is_current {
            let (project, asked) = store
                .sessions
                .get(&sid)
                .map(|s| (s.project.clone(), s.root().last_said.trim_end().ends_with('?')))
                .unwrap_or_default();
            toast = Some((ToastKind::Done, format!("💬 {project}: awaiting your orders")));
            sfx = Some(if asked { Sfx::Question } else { Sfx::Waiting });
        }
        if let Some(s) = store.sessions.get_mut(&sid) {
            s.apply(ev);
        }
        if let Some((kind, text)) = toast {
            store.toast(kind, text);
        }
        if let Some(s) = sfx
            && (s != Sfx::Blip || settings.tool_blips)
        {
            store.play(s);
        }
        store.layout_dirty = true;
    }
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
    for session in store.sessions.values() {
        let is_current = store.current.as_deref() == Some(session.id.as_str());
        for a in &session.agents {
            for c in a.calls.iter().filter(|c| c.ended.is_none() && c.started > 0) {
                open.insert(c.id.clone());
                if now - c.started > theme::STALL_AFTER_MS && !store.stalled.contains(&c.id) {
                    let tool = if is_current {
                        c.tool.clone()
                    } else {
                        format!("{}: {}", session.project, c.tool)
                    };
                    newly.push((c.id.clone(), tool, now - c.started));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn r(id: &str, modified: i64) -> SessionRef {
        SessionRef {
            id: id.into(),
            project: "p".into(),
            path: PathBuf::from(id),
            modified,
            size: 1,
        }
    }

    #[test]
    fn calls_are_classified_for_the_radio() {
        assert_eq!(call_kind("Read", None), CallKind::Scout);
        assert_eq!(call_kind("Grep", None), CallKind::Scout);
        assert_eq!(call_kind("Edit", None), CallKind::Dig);
        assert_eq!(call_kind("WebSearch", None), CallKind::Intel);
        assert_eq!(call_kind("Bash", Some("cargo test --release")), CallKind::Test);
        assert_eq!(call_kind("Bash", Some("node bin/factory.mjs check x")), CallKind::Test);
        assert_eq!(
            call_kind("Bash", Some("git add -A && git commit -m x")),
            CallKind::Commit
        );
        assert_eq!(call_kind("Bash", Some("ls -la")), CallKind::Other);
        assert!(output_says_failed("CHECK FAILED", ""));
        assert!(output_says_failed("", "error[E0308]: mismatched types"));
        assert!(!output_says_failed("test result: ok. 3 passed", ""));
    }

    #[test]
    fn pinned_sessions_stay_watched_after_you_leave_them() {
        let now = 10_000_000;
        let list = vec![r("stale", now - theme::WATCH_RECENT_MS - 1), r("cur", now - 1_000)];
        let w = desired_watch(&list, Some("cur"), &["stale".to_string()], now);
        let ids: Vec<&str> = w.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["cur", "stale"], "a stale session stays watched while pinned");
        let mut store = Store::new(None, PathBuf::from("."), None);
        store.current = Some("a".into());
        store.pin_current("b");
        assert_eq!(store.pinned, vec!["a".to_string()]);
        store.current = Some("b".into());
        store.pin_current("a");
        assert_eq!(store.pinned, vec!["b".to_string()], "going back unpins the destination");
    }

    #[test]
    fn watch_keeps_current_and_recent_sessions_only() {
        let now = 10_000_000;
        let list = vec![
            r("old", now - theme::WATCH_RECENT_MS - 1),
            r("a", now - 1_000),
            r("b", now - 2_000),
            r("c", now - 3_000),
            r("d", now - 4_000),
            r("e", now - 5_000),
            r("f", now - 6_000),
        ];
        let w = desired_watch(&list, Some("old"), &[], now);
        let ids: Vec<&str> = w.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            ["old", "a", "b", "c", "d", "e"],
            "current first, then newest, capped"
        );
        let w = desired_watch(&list, None, &[], now);
        assert_eq!(w.len(), theme::WATCH_MAX);
        assert!(!w.iter().any(|s| s.id == "old"));
    }
}
