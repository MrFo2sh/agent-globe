//! Session model and the single reducer that applies ingest events to it.

use super::artifacts::{self, Artifact};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Milliseconds since the Unix epoch.
pub type Millis = i64;

pub const ROOT_AGENT: &str = "root";

/// The one event schema every source produces (file tailers, HTTP ingest, hooks).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    SessionStart {
        session_id: String,
        #[serde(default)]
        project: String,
        #[serde(default)]
        at: Millis,
    },
    AgentSpawn {
        session_id: String,
        agent_id: String,
        #[serde(default)]
        parent_id: Option<String>,
        /// The tool-use id of the call that spawned this agent, used to resolve the parent.
        #[serde(default)]
        spawn_tool_use_id: Option<String>,
        #[serde(default)]
        name: String,
        #[serde(default)]
        kind: String,
        #[serde(default)]
        model: String,
        #[serde(default)]
        prompt: String,
        /// Launched in the background: its spawning call returns at launch, not when it is done.
        #[serde(default)]
        background: bool,
        #[serde(default)]
        at: Millis,
    },
    ToolStart {
        session_id: String,
        agent_id: String,
        call_id: String,
        tool: String,
        #[serde(default)]
        summary: String,
        #[serde(default)]
        command: Option<String>,
        /// What the agent said right before this call: the narrator line.
        #[serde(default)]
        note: String,
        /// Task-list snapshot when the call is a TodoWrite: real progress.
        #[serde(default)]
        todos: Option<Progress>,
        #[serde(default)]
        background: bool,
        #[serde(default)]
        at: Millis,
    },
    ToolEnd {
        session_id: String,
        agent_id: String,
        call_id: String,
        #[serde(default)]
        stdout: String,
        #[serde(default)]
        stderr: String,
        #[serde(default)]
        is_error: bool,
        #[serde(default)]
        at: Millis,
    },
    Message {
        session_id: String,
        agent_id: String,
        role: String,
        text: String,
        #[serde(default)]
        model: String,
        #[serde(default)]
        output_tokens: u64,
        /// Prompt size the model saw for this message (input + cache read + cache write).
        #[serde(default)]
        context_tokens: u64,
        #[serde(default)]
        input_tokens: u64,
        #[serde(default)]
        cache_read_tokens: u64,
        #[serde(default)]
        cache_write_tokens: u64,
        /// Working directory of the session, when the source knows it.
        #[serde(default)]
        cwd: String,
        #[serde(default)]
        at: Millis,
    },
    AgentEnd {
        session_id: String,
        agent_id: String,
        #[serde(default)]
        at: Millis,
    },
    /// The agent finished a turn and is waiting for the user.
    TurnEnd {
        session_id: String,
        agent_id: String,
        #[serde(default)]
        duration_ms: u64,
        #[serde(default)]
        at: Millis,
    },
    /// Claude Code was refused by the plan's rate limit; `resets_at` is when the window reopens.
    Throttle {
        session_id: String,
        agent_id: String,
        #[serde(default)]
        resets_at: Millis,
        #[serde(default)]
        kind: String,
        #[serde(default)]
        at: Millis,
    },
}

/// A rate-limit refusal recorded in a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Throttle {
    pub at: Millis,
    pub resets_at: Millis,
    pub kind: String,
}

/// First non-empty line of a text, cut to `max` characters with an ellipsis.
pub fn snippet(text: &str, max: usize) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let mut out: String = line.chars().take(max).collect();
    if line.chars().count() > max {
        out.push('…');
    }
    out
}

impl Event {
    pub fn session_id(&self) -> &str {
        match self {
            Event::SessionStart { session_id, .. }
            | Event::AgentSpawn { session_id, .. }
            | Event::ToolStart { session_id, .. }
            | Event::ToolEnd { session_id, .. }
            | Event::Message { session_id, .. }
            | Event::AgentEnd { session_id, .. }
            | Event::TurnEnd { session_id, .. }
            | Event::Throttle { session_id, .. } => session_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Active,
    Idle,
    Done,
}

/// Progress from the agent's own task list (TodoWrite): done over total, and the item in progress.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub done: u32,
    pub total: u32,
    #[serde(default)]
    pub current: String,
}

impl Progress {
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            self.done as f32 / self.total as f32
        }
    }
}

/// One of the human's prompts and everything the agent did in answer to it: the calls from
/// `first_call` up to the next turn's first call, and the tokens the answer cost.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub prompt: String,
    pub started: Millis,
    pub ended: Option<Millis>,
    pub first_call: usize,
    /// The commander's final text for this turn, what the editor shows as the answer.
    #[serde(default)]
    pub reply: String,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub tool: String,
    pub summary: String,
    pub command: Option<String>,
    /// Narrator line: what the agent said right before the call.
    #[serde(default)]
    pub note: String,
    pub stdout: String,
    pub stderr: String,
    pub is_error: bool,
    pub started: Millis,
    pub ended: Option<Millis>,
    /// Started with `run_in_background`: a process that outlives the call.
    #[serde(default)]
    pub background: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub parent: Option<String>,
    pub spawn_tool_use_id: Option<String>,
    pub name: String,
    pub kind: String,
    pub model: String,
    pub depth: u32,
    pub prompt: String,
    pub started: Millis,
    pub ended: Option<Millis>,
    pub last_activity: Millis,
    pub calls: Vec<ToolCall>,
    pub messages: u32,
    pub output_tokens: u64,
    /// Latest prompt size the model saw, in tokens (0 if unknown).
    #[serde(default)]
    pub context_tokens: u64,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
    /// URLs, files, images, commits, PRs, test results this agent produced, oldest first.
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    /// Latest task-list snapshot, if the agent keeps one.
    #[serde(default)]
    pub progress: Option<Progress>,
    /// The human's prompts in order, each with the calls and tokens that answered it.
    #[serde(default)]
    pub turn_log: Vec<Turn>,
    /// The agent's latest message text: what it said or asked last.
    #[serde(default)]
    pub last_said: String,
    /// When the agent last sent a text-only reply (no tool call in it). If nothing follows for a
    /// few seconds, that reply was the end of the turn: the transcript carries no end marker.
    #[serde(default)]
    pub final_text_at: Option<Millis>,
    /// Completed turns (root agent: times it handed control back to the user).
    #[serde(default)]
    pub turns: u32,
    /// True from a turn end until the agent does anything else: it is waiting for the human.
    #[serde(default)]
    pub waiting_for_user: bool,
    /// Order in which this agent was spawned within the session (0 = root).
    pub order: u32,
    /// Runs in the background: the call that spawned it returned at launch, so that call's end
    /// says nothing about this agent's.
    #[serde(default)]
    pub background: bool,
}

impl Agent {
    pub fn status(&self, now: Millis, active_window_ms: Millis) -> Status {
        if self.ended.is_some() {
            Status::Done
        } else if now - self.last_activity <= active_window_ms {
            Status::Active
        } else {
            Status::Idle
        }
    }

    pub fn has_open_call(&self) -> bool {
        self.calls.iter().any(|c| c.ended.is_none())
    }

    /// Indices of the calls made in answer to turn `i`.
    pub fn turn_range(&self, i: usize) -> std::ops::Range<usize> {
        let n = self.calls.len();
        let start = self.turn_log.get(i).map(|t| t.first_call).unwrap_or(0).min(n);
        let end = self.turn_log.get(i + 1).map(|t| t.first_call).unwrap_or(n).min(n);
        start..end.max(start)
    }

    /// True while the agent is "taking fire": its most recently finished call failed and nothing
    /// has succeeded since. A failure buried in history is not a current status.
    pub fn last_call_failed(&self) -> bool {
        self.calls
            .iter()
            .filter(|c| c.ended.is_some())
            .max_by_key(|c| c.ended.unwrap_or(0))
            .is_some_and(|c| c.is_error)
    }

    fn add_artifacts(&mut self, found: Vec<Artifact>) {
        for a in found {
            if !self.artifacts.iter().any(|x| x.kind == a.kind && x.target == a.target) {
                self.artifacts.push(a);
            }
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub project: String,
    /// Working directory of the root agent, when known.
    #[serde(default)]
    pub cwd: String,
    pub started: Millis,
    pub last_activity: Millis,
    /// Agents in spawn order; index 0 is always the root.
    pub agents: Vec<Agent>,
    #[serde(skip)]
    index: HashMap<String, usize>,
    /// tool-use id -> agent that issued it. Used to resolve subagent parents.
    #[serde(skip)]
    call_owner: HashMap<String, String>,
    pub parse_errors: u32,
    /// Rate-limit refusals seen in this session, oldest first.
    #[serde(default)]
    pub throttles: Vec<Throttle>,
}

impl Session {
    pub fn new(id: impl Into<String>, project: impl Into<String>, at: Millis) -> Self {
        let mut s = Session {
            id: id.into(),
            project: project.into(),
            started: at,
            last_activity: at,
            ..Default::default()
        };
        s.ensure_agent(ROOT_AGENT, None, None, "session", "root", "", "", at);
        s
    }

    pub fn agent(&self, id: &str) -> Option<&Agent> {
        self.index.get(id).map(|&i| &self.agents[i])
    }

    pub fn agent_mut(&mut self, id: &str) -> Option<&mut Agent> {
        match self.index.get(id) {
            Some(&i) => Some(&mut self.agents[i]),
            None => None,
        }
    }

    pub fn root(&self) -> &Agent {
        &self.agents[0]
    }

    pub fn children(&self, id: &str) -> impl Iterator<Item = &Agent> {
        self.agents.iter().filter(move |a| a.parent.as_deref() == Some(id))
    }

    /// Turn-end events this session is owed: a commander whose last reply was text only, with
    /// nothing after it for `grace_ms`, has finished its turn even if the transcript never says so.
    pub fn unsettled_turns(&self, now: Millis, grace_ms: Millis) -> Vec<Event> {
        self.agents
            .iter()
            .filter(|a| a.depth == 0 && !a.waiting_for_user && a.ended.is_none() && !a.has_open_call())
            .filter_map(|a| {
                let at = a.final_text_at?;
                (now - at >= grace_ms).then(|| Event::TurnEnd {
                    session_id: self.id.clone(),
                    agent_id: a.id.clone(),
                    duration_ms: 0,
                    at,
                })
            })
            .collect()
    }

    pub fn total_calls(&self) -> usize {
        self.agents.iter().map(|a| a.calls.len()).sum()
    }

    pub fn calls_since(&self, since: Millis) -> usize {
        self.agents
            .iter()
            .flat_map(|a| a.calls.iter())
            .filter(|c| c.started >= since)
            .count()
    }

    #[allow(clippy::too_many_arguments)]
    fn ensure_agent(
        &mut self,
        id: &str,
        parent: Option<String>,
        spawn_tool_use_id: Option<String>,
        name: &str,
        kind: &str,
        model: &str,
        prompt: &str,
        at: Millis,
    ) -> usize {
        if let Some(&i) = self.index.get(id) {
            let a = &mut self.agents[i];
            if a.name.is_empty() && !name.is_empty() {
                a.name = name.to_string();
            }
            if a.kind.is_empty() && !kind.is_empty() {
                a.kind = kind.to_string();
            }
            if a.model.is_empty() && !model.is_empty() {
                a.model = model.to_string();
            }
            if a.prompt.is_empty() && !prompt.is_empty() {
                a.prompt = prompt.to_string();
            }
            if a.parent.is_none() && parent.is_some() {
                a.parent = parent;
            }
            if a.spawn_tool_use_id.is_none() && spawn_tool_use_id.is_some() {
                a.spawn_tool_use_id = spawn_tool_use_id;
            }
            return i;
        }
        let order = self.agents.len() as u32;
        let depth = if id == ROOT_AGENT { 0 } else { 1 };
        self.agents.push(Agent {
            id: id.to_string(),
            parent,
            spawn_tool_use_id,
            name: name.to_string(),
            kind: kind.to_string(),
            model: model.to_string(),
            depth,
            prompt: prompt.to_string(),
            started: at,
            ended: None,
            last_activity: at,
            calls: Vec::new(),
            messages: 0,
            output_tokens: 0,
            context_tokens: 0,
            input_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            artifacts: Vec::new(),
            progress: None,
            turn_log: Vec::new(),
            last_said: String::new(),
            final_text_at: None,
            turns: 0,
            waiting_for_user: false,
            order,
            background: false,
        });
        let i = self.agents.len() - 1;
        self.index.insert(id.to_string(), i);
        i
    }

    fn touch(&mut self, agent_id: &str, at: Millis) {
        if at <= 0 {
            return;
        }
        if let Some(a) = self.agent_mut(agent_id) {
            if at > a.last_activity {
                a.last_activity = at;
            }
            if a.started == 0 {
                a.started = at;
            }
        }
        if at > self.last_activity {
            self.last_activity = at;
        }
        if self.started == 0 {
            self.started = at;
        }
    }

    /// Resolve parents and depths for every agent whose parent is still unknown.
    fn resolve_tree(&mut self) {
        // Parents from spawn tool-use ids.
        let resolved: Vec<(usize, String)> = self
            .agents
            .iter()
            .enumerate()
            .filter(|(_, a)| a.parent.is_none() && a.id != ROOT_AGENT)
            .filter_map(|(i, a)| {
                a.spawn_tool_use_id
                    .as_ref()
                    .and_then(|t| self.call_owner.get(t))
                    .map(|owner| (i, owner.clone()))
            })
            .collect();
        for (i, owner) in resolved {
            self.agents[i].parent = Some(owner);
        }
        // Depths, iterated because children may precede parents in the vector.
        for _ in 0..8 {
            let mut changed = false;
            for i in 0..self.agents.len() {
                let depth = match self.agents[i].parent.as_deref() {
                    None => 0,
                    Some(p) => self.agent(p).map(|pa| pa.depth + 1).unwrap_or(1),
                };
                if self.agents[i].depth != depth {
                    self.agents[i].depth = depth;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// The single reducer. Every source goes through here.
    pub fn apply(&mut self, ev: Event) {
        match ev {
            Event::SessionStart { project, at, .. } => {
                if self.project.is_empty() {
                    self.project = project;
                }
                self.touch(ROOT_AGENT, at);
            }
            Event::AgentSpawn {
                agent_id,
                parent_id,
                spawn_tool_use_id,
                name,
                kind,
                model,
                prompt,
                background,
                at,
                ..
            } => {
                let i = self.ensure_agent(
                    &agent_id,
                    parent_id,
                    spawn_tool_use_id,
                    &name,
                    &kind,
                    &model,
                    &prompt,
                    at,
                );
                self.agents[i].background |= background;
                self.touch(&agent_id, at);
                self.resolve_tree();
            }
            Event::ToolStart {
                agent_id,
                call_id,
                tool,
                summary,
                command,
                note,
                todos,
                background,
                at,
                ..
            } => {
                self.ensure_agent(&agent_id, None, None, "", "", "", "", at);
                self.call_owner.insert(call_id.clone(), agent_id.clone());
                let a = self.agent_mut(&agent_id).expect("ensured");
                a.waiting_for_user = false;
                a.final_text_at = None;
                if let Some(p) = todos {
                    a.progress = Some(p);
                }
                if !a.calls.iter().any(|c| c.id == call_id) {
                    let call = ToolCall {
                        id: call_id,
                        tool,
                        summary,
                        command,
                        note,
                        stdout: String::new(),
                        stderr: String::new(),
                        is_error: false,
                        started: at,
                        ended: None,
                        background,
                    };
                    let found = artifacts::from_call(&call);
                    a.calls.push(call);
                    a.add_artifacts(found);
                }
                self.touch(&agent_id, at);
                self.resolve_tree();
            }
            Event::ToolEnd {
                agent_id,
                call_id,
                stdout,
                stderr,
                is_error,
                at,
                ..
            } => {
                // Prefer the agent that owns the call; sources may not know it.
                let owner = self.call_owner.get(&call_id).cloned().unwrap_or(agent_id);
                self.ensure_agent(&owner, None, None, "", "", "", "", at);
                let a = self.agent_mut(&owner).expect("ensured");
                let mut found = Vec::new();
                if let Some(c) = a.calls.iter_mut().find(|c| c.id == call_id) {
                    c.stdout = stdout;
                    c.stderr = stderr;
                    c.is_error = is_error;
                    c.ended = Some(at);
                    found = artifacts::from_call(c);
                }
                a.add_artifacts(found);
                // A finished spawn call means the child agent is done, unless the child runs in
                // the background: then the call returned at launch and the source reports the end.
                let child: Option<String> = self
                    .agents
                    .iter()
                    .find(|x| !x.background && x.spawn_tool_use_id.as_deref() == Some(call_id.as_str()))
                    .map(|x| x.id.clone());
                if let Some(child) = child
                    && let Some(c) = self.agent_mut(&child)
                    && c.ended.is_none()
                {
                    c.ended = Some(at);
                }
                self.touch(&owner, at);
            }
            Event::Message {
                agent_id,
                role,
                text,
                model,
                output_tokens,
                context_tokens,
                input_tokens,
                cache_read_tokens,
                cache_write_tokens,
                cwd,
                at,
                ..
            } => {
                if self.cwd.is_empty() && !cwd.is_empty() {
                    self.cwd = cwd;
                }
                self.ensure_agent(&agent_id, None, None, "", "", &model, "", at);
                let a = self.agent_mut(&agent_id).expect("ensured");
                // The model of the latest reply: a session switched with /model shows the new one.
                // Claude Code's own placeholder messages ("<synthetic>") are no model.
                if !model.is_empty() && !model.starts_with('<') {
                    a.model = model.clone();
                }
                a.waiting_for_user = false;
                a.messages += 1;
                if role == "assistant" && !text.trim().is_empty() {
                    a.last_said = text.chars().take(12_000).collect();
                }
                if role == "assistant" {
                    // Tool calls of the same record were applied just before, with this timestamp.
                    let had_tools = a.calls.last().is_some_and(|c| c.started == at);
                    a.final_text_at = if !had_tools && !text.trim().is_empty() {
                        Some(at)
                    } else {
                        None
                    };
                }
                a.output_tokens += output_tokens;
                a.input_tokens += input_tokens;
                a.cache_read_tokens += cache_read_tokens;
                a.cache_write_tokens += cache_write_tokens;
                if context_tokens > 0 {
                    a.context_tokens = context_tokens;
                }
                if role == "user" && !text.trim().is_empty() {
                    // A new prompt closes the previous turn if the agent never signalled its end.
                    if let Some(t) = a.turn_log.last_mut()
                        && t.ended.is_none()
                    {
                        t.ended = Some(at);
                    }
                    a.turn_log.push(Turn {
                        prompt: text.chars().take(2000).collect(),
                        started: at,
                        ended: None,
                        first_call: a.calls.len(),
                        ..Default::default()
                    });
                    if a.prompt.is_empty() {
                        a.prompt = text;
                    }
                } else if role == "assistant"
                    && let Some(t) = a.turn_log.last_mut()
                {
                    if !text.trim().is_empty() {
                        t.reply = text.chars().take(12_000).collect();
                    }
                    t.output_tokens += output_tokens;
                    t.input_tokens += input_tokens;
                    t.cache_read_tokens += cache_read_tokens;
                    t.cache_write_tokens += cache_write_tokens;
                }
                self.touch(&agent_id, at);
            }
            Event::AgentEnd { agent_id, at, .. } => {
                if let Some(a) = self.agent_mut(&agent_id)
                    && a.ended.is_none()
                {
                    a.ended = Some(if at > 0 { at } else { a.last_activity });
                }
                self.touch(&agent_id, at);
            }
            Event::Throttle {
                agent_id,
                resets_at,
                kind,
                at,
                ..
            } => {
                self.ensure_agent(&agent_id, None, None, "", "", "", "", at);
                self.throttles.push(Throttle { at, resets_at, kind });
                self.touch(&agent_id, at);
            }
            Event::TurnEnd { agent_id, at, .. } => {
                self.ensure_agent(&agent_id, None, None, "", "", "", "", at);
                if let Some(a) = self.agent_mut(&agent_id) {
                    if !a.waiting_for_user {
                        a.turns += 1;
                    }
                    a.waiting_for_user = true;
                    a.final_text_at = None;
                    if let Some(t) = a.turn_log.last_mut()
                        && t.ended.is_none()
                    {
                        t.ended = Some(at);
                    }
                }
                self.touch(&agent_id, at);
            }
        }
    }

    pub fn apply_all(&mut self, events: impl IntoIterator<Item = Event>) {
        for e in events {
            self.apply(e);
        }
    }
}

/// Parse an RFC 3339 timestamp into millis. Returns 0 when missing or malformed.
pub fn parse_millis(ts: Option<&str>) -> Millis {
    ts.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp_millis())
        .unwrap_or(0)
}

pub fn now_millis() -> Millis {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn(id: &str, tool: &str, at: Millis) -> Event {
        Event::AgentSpawn {
            session_id: "s".into(),
            agent_id: id.into(),
            parent_id: None,
            spawn_tool_use_id: Some(tool.into()),
            name: id.into(),
            kind: "fork".into(),
            model: String::new(),
            prompt: String::new(),
            background: false,
            at,
        }
    }

    fn tool(agent: &str, call: &str, at: Millis) -> Event {
        Event::ToolStart {
            session_id: "s".into(),
            agent_id: agent.into(),
            call_id: call.into(),
            tool: "Agent".into(),
            summary: String::new(),
            command: None,
            note: String::new(),
            todos: None,
            background: false,
            at,
        }
    }

    #[test]
    fn resolves_parent_from_spawn_tool_use_even_when_child_arrives_first() {
        let mut s = Session::new("s", "p", 1);
        s.apply(spawn("a", "t1", 2));
        assert_eq!(s.agent("a").unwrap().parent, None);
        s.apply(tool(ROOT_AGENT, "t1", 2));
        assert_eq!(s.agent("a").unwrap().parent.as_deref(), Some(ROOT_AGENT));
        assert_eq!(s.agent("a").unwrap().depth, 1);
        // grandchild
        s.apply(tool("a", "t2", 3));
        s.apply(spawn("b", "t2", 3));
        assert_eq!(s.agent("b").unwrap().parent.as_deref(), Some("a"));
        assert_eq!(s.agent("b").unwrap().depth, 2);
    }

    #[test]
    fn tool_end_marks_spawned_child_done() {
        let mut s = Session::new("s", "p", 1);
        s.apply(tool(ROOT_AGENT, "t1", 2));
        s.apply(spawn("a", "t1", 2));
        s.apply(Event::ToolEnd {
            session_id: "s".into(),
            agent_id: ROOT_AGENT.into(),
            call_id: "t1".into(),
            stdout: "ok".into(),
            stderr: String::new(),
            is_error: false,
            at: 9,
        });
        assert_eq!(s.agent("a").unwrap().ended, Some(9));
        assert_eq!(s.root().calls[0].stdout, "ok");
        assert_eq!(s.root().status(10, 100), Status::Active);
        assert_eq!(s.root().status(1000, 100), Status::Idle);
        assert_eq!(s.agent("a").unwrap().status(10, 100), Status::Done);
    }

    /// A background launch returns at once: the child keeps working until its own end arrives.
    #[test]
    fn background_child_outlives_its_spawning_call() {
        let mut s = Session::new("s", "p", 1);
        s.apply(tool(ROOT_AGENT, "t1", 2));
        let mut ev = spawn("a", "t1", 2);
        if let Event::AgentSpawn { background, .. } = &mut ev {
            *background = true;
        }
        s.apply(ev);
        s.apply(end("t1", false, 3));
        assert_eq!(s.agent("a").unwrap().ended, None);
        s.apply(Event::AgentEnd {
            session_id: "s".into(),
            agent_id: "a".into(),
            at: 0,
        });
        assert!(s.agent("a").unwrap().ended.is_some());
    }

    fn end(call: &str, is_error: bool, at: Millis) -> Event {
        Event::ToolEnd {
            session_id: "s".into(),
            agent_id: ROOT_AGENT.into(),
            call_id: call.into(),
            stdout: String::new(),
            stderr: String::new(),
            is_error,
            at,
        }
    }

    #[test]
    fn only_the_latest_finished_call_counts_as_failed() {
        let mut s = Session::new("s", "p", 1);
        s.apply(tool(ROOT_AGENT, "t1", 2));
        s.apply(end("t1", true, 3));
        assert!(s.root().last_call_failed());
        // A later success clears the status even though the old failure stays in history.
        s.apply(tool(ROOT_AGENT, "t2", 4));
        assert!(s.root().last_call_failed(), "still failed while the next call is open");
        s.apply(end("t2", false, 5));
        assert!(!s.root().last_call_failed());
    }

    fn msg(role: &str, text: &str, at: Millis, out: u64) -> Event {
        serde_json::from_value(serde_json::json!({
            "type": "message", "session_id": "s", "agent_id": ROOT_AGENT, "role": role, "text": text,
            "model": "claude-opus-5-5", "output_tokens": out, "at": at
        }))
        .expect("message event")
    }

    #[test]
    fn prompts_group_calls_and_tokens_into_turns() {
        let mut s = Session::new("s", "p", 1);
        s.apply(tool(ROOT_AGENT, "t0", 2)); // before any prompt was seen
        s.apply(msg("user", "build it", 10, 0));
        s.apply(tool(ROOT_AGENT, "t1", 11));
        s.apply(msg("assistant", "done", 12, 500));
        s.apply(tool(ROOT_AGENT, "t2", 13));
        s.apply(Event::TurnEnd {
            session_id: "s".into(),
            agent_id: ROOT_AGENT.into(),
            duration_ms: 0,
            at: 20,
        });
        s.apply(msg("user", "now test it", 30, 0));
        s.apply(tool(ROOT_AGENT, "t3", 31));
        let a = s.root();
        assert_eq!(a.turn_log.len(), 2);
        assert_eq!((a.turn_log[0].first_call, a.turn_log[0].ended), (1, Some(20)));
        assert_eq!(a.turn_log[0].output_tokens, 500);
        assert_eq!(a.turn_log[0].reply, "done", "each turn keeps the commander's reply");
        assert_eq!(a.turn_log[1].reply, "", "a text-only turn with no answer yet is empty");
        assert_eq!(a.turn_range(0), 1..3);
        assert_eq!(a.turn_range(1), 3..4);
        assert!(a.turn_log[1].ended.is_none(), "the second turn is still open");
        // a third prompt without a TurnEnd closes the second turn
        s.apply(msg("user", "again", 40, 0));
        assert_eq!(s.root().turn_log[1].ended, Some(40));
    }

    #[test]
    fn throttles_and_last_words_are_kept() {
        let mut s = Session::new("s", "p", 1);
        s.apply(msg("assistant", "Should I delete the old records?", 5, 10));
        assert_eq!(s.root().last_said, "Should I delete the old records?");
        s.apply(Event::Throttle {
            session_id: "s".into(),
            agent_id: ROOT_AGENT.into(),
            resets_at: 1_000_000,
            kind: "five_hour".into(),
            at: 900_000,
        });
        assert_eq!(s.throttles.len(), 1);
        assert_eq!(s.throttles[0].resets_at, 1_000_000);
        assert_eq!(snippet("  \n first line here\nsecond", 8), "first li…");
        assert_eq!(snippet("short", 10), "short");
    }

    /// `/model` mid-session: the panel shows the model of the latest reply, not the first.
    #[test]
    fn the_model_follows_the_latest_reply() {
        let mut s = Session::new("s", "p", 1);
        s.apply(msg("assistant", "hi", 5, 1));
        assert_eq!(s.root().model, "claude-opus-5-5");
        let reply = |model: &str| -> Event {
            serde_json::from_value(serde_json::json!({
                "type": "message", "session_id": "s", "agent_id": ROOT_AGENT, "role": "assistant",
                "text": "x", "model": model, "at": 9
            }))
            .expect("message event")
        };
        s.apply(reply("claude-opus-4-8"));
        assert_eq!(s.root().model, "claude-opus-4-8");
        s.apply(reply("<synthetic>"));
        assert_eq!(s.root().model, "claude-opus-4-8", "placeholders are no model");
    }

    #[test]
    fn a_text_only_reply_settles_the_turn_after_the_grace() {
        let mut s = Session::new("s", "p", 1);
        s.apply(msg("user", "do it", 10, 0));
        s.apply(tool(ROOT_AGENT, "t1", 20));
        s.apply(msg("assistant", "on it", 20, 5)); // same timestamp as the call: not final
        assert_eq!(s.root().final_text_at, None);
        s.apply(end("t1", false, 25));
        s.apply(msg("assistant", "Done. Anything else?", 30, 5));
        assert_eq!(s.root().final_text_at, Some(30));
        assert!(s.unsettled_turns(3_000, 5_000).is_empty(), "too soon");
        let evs = s.unsettled_turns(6_000, 5_000);
        assert_eq!(evs.len(), 1);
        for e in evs {
            s.apply(e);
        }
        assert!(s.root().waiting_for_user);
        assert_eq!(s.root().turn_log[0].ended, Some(30));
        assert_eq!(s.root().turns, 1);
        // a late real TurnEnd does not count the turn twice
        s.apply(Event::TurnEnd {
            session_id: "s".into(),
            agent_id: ROOT_AGENT.into(),
            duration_ms: 0,
            at: 31,
        });
        assert_eq!(s.root().turns, 1);
        // a text reply followed by a tool call is not final
        s.apply(msg("user", "more", 40, 0));
        s.apply(msg("assistant", "Let me look.", 41, 5));
        s.apply(tool(ROOT_AGENT, "t2", 43));
        assert_eq!(s.root().final_text_at, None);
        assert!(s.unsettled_turns(60_000, 5_000).is_empty());
    }

    #[test]
    fn event_json_roundtrip() {
        let e = Event::ToolStart {
            session_id: "s".into(),
            agent_id: "root".into(),
            call_id: "c".into(),
            tool: "Bash".into(),
            summary: "ls".into(),
            command: Some("ls -la".into()),
            note: "checking the tree".into(),
            todos: None,
            background: false,
            at: 5,
        };
        let j = serde_json::to_string(&e).unwrap();
        assert!(j.contains("\"type\":\"tool_start\""));
        let back: Event = serde_json::from_str(&j).unwrap();
        assert_eq!(back, e);
    }
}
