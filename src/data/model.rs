//! Session model and the single reducer that applies ingest events to it.

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
        #[serde(default)]
        at: Millis,
    },
    AgentEnd {
        session_id: String,
        agent_id: String,
        #[serde(default)]
        at: Millis,
    },
}

impl Event {
    pub fn session_id(&self) -> &str {
        match self {
            Event::SessionStart { session_id, .. }
            | Event::AgentSpawn { session_id, .. }
            | Event::ToolStart { session_id, .. }
            | Event::ToolEnd { session_id, .. }
            | Event::Message { session_id, .. }
            | Event::AgentEnd { session_id, .. } => session_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Active,
    Idle,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub tool: String,
    pub summary: String,
    pub command: Option<String>,
    pub stdout: String,
    pub stderr: String,
    pub is_error: bool,
    pub started: Millis,
    pub ended: Option<Millis>,
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
    /// Order in which this agent was spawned within the session (0 = root).
    pub order: u32,
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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub project: String,
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
            order,
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
                at,
                ..
            } => {
                self.ensure_agent(
                    &agent_id,
                    parent_id,
                    spawn_tool_use_id,
                    &name,
                    &kind,
                    &model,
                    &prompt,
                    at,
                );
                self.touch(&agent_id, at);
                self.resolve_tree();
            }
            Event::ToolStart {
                agent_id,
                call_id,
                tool,
                summary,
                command,
                at,
                ..
            } => {
                self.ensure_agent(&agent_id, None, None, "", "", "", "", at);
                self.call_owner.insert(call_id.clone(), agent_id.clone());
                let a = self.agent_mut(&agent_id).expect("ensured");
                if !a.calls.iter().any(|c| c.id == call_id) {
                    a.calls.push(ToolCall {
                        id: call_id,
                        tool,
                        summary,
                        command,
                        stdout: String::new(),
                        stderr: String::new(),
                        is_error: false,
                        started: at,
                        ended: None,
                    });
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
                if let Some(c) = a.calls.iter_mut().find(|c| c.id == call_id) {
                    c.stdout = stdout;
                    c.stderr = stderr;
                    c.is_error = is_error;
                    c.ended = Some(at);
                }
                // A finished spawn call means the child agent is done.
                let child: Option<String> = self
                    .agents
                    .iter()
                    .find(|x| x.spawn_tool_use_id.as_deref() == Some(call_id.as_str()))
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
                at,
                ..
            } => {
                self.ensure_agent(&agent_id, None, None, "", "", &model, "", at);
                let a = self.agent_mut(&agent_id).expect("ensured");
                a.messages += 1;
                a.output_tokens += output_tokens;
                if a.prompt.is_empty() && role == "user" && !text.trim().is_empty() {
                    a.prompt = text;
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

    #[test]
    fn event_json_roundtrip() {
        let e = Event::ToolStart {
            session_id: "s".into(),
            agent_id: "root".into(),
            call_id: "c".into(),
            tool: "Bash".into(),
            summary: "ls".into(),
            command: Some("ls -la".into()),
            at: 5,
        };
        let j = serde_json::to_string(&e).unwrap();
        assert!(j.contains("\"type\":\"tool_start\""));
        let back: Event = serde_json::from_str(&j).unwrap();
        assert_eq!(back, e);
    }
}
