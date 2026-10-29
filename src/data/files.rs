//! Which files a session's agents are reading and editing: the "where is it digging" view.

use super::model::{Millis, Session};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Touch {
    pub agent_id: String,
    pub at: Millis,
    pub edit: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileStat {
    /// Path as the tool saw it, with forward slashes.
    pub path: String,
    /// Last two path components, for the label under the node.
    pub label: String,
    pub reads: u32,
    pub edits: u32,
    pub last_at: Millis,
    /// Recent touches only (within the window given to `hot_files`), for the beams.
    pub touches: Vec<Touch>,
}

impl FileStat {
    /// Edits weigh three reads.
    pub fn heat(&self) -> u32 {
        self.edits * 3 + self.reads
    }
}

pub fn is_edit_tool(tool: &str) -> bool {
    matches!(tool, "Edit" | "Write" | "MultiEdit" | "NotebookEdit")
}

pub fn is_read_tool(tool: &str) -> bool {
    tool == "Read"
}

/// Short label: the last two path components.
pub fn label_for(path: &str) -> String {
    let parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    let n = parts.len();
    parts[n.saturating_sub(2)..].join("/")
}

/// The hottest files of a session, most touched first, at most `max`. Touches older than
/// `recent_ms` still count towards heat but are not kept for beams.
pub fn hot_files(session: &Session, max: usize, now: Millis, recent_ms: Millis) -> Vec<FileStat> {
    let mut map: HashMap<String, FileStat> = HashMap::new();
    for a in &session.agents {
        for c in &a.calls {
            let edit = is_edit_tool(&c.tool);
            if !edit && !is_read_tool(&c.tool) {
                continue;
            }
            let path = c.summary.trim();
            if !(path.contains('/') || path.contains('\\')) {
                continue;
            }
            let key = path.replace('\\', "/");
            let e = map.entry(key.clone()).or_insert_with(|| FileStat {
                label: label_for(&key),
                path: key.clone(),
                reads: 0,
                edits: 0,
                last_at: 0,
                touches: Vec::new(),
            });
            if edit {
                e.edits += 1;
            } else {
                e.reads += 1;
            }
            e.last_at = e.last_at.max(c.started);
            if now - c.started <= recent_ms {
                e.touches.push(Touch {
                    agent_id: a.id.clone(),
                    at: c.started,
                    edit,
                });
            }
        }
    }
    let mut v: Vec<FileStat> = map.into_values().collect();
    v.sort_by(|a, b| {
        b.heat()
            .cmp(&a.heat())
            .then(b.last_at.cmp(&a.last_at))
            .then(a.path.cmp(&b.path))
    });
    v.truncate(max);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::model::{Event, ROOT_AGENT};

    fn call(agent: &str, id: &str, tool: &str, path: &str, at: Millis) -> Event {
        Event::ToolStart {
            session_id: "s".into(),
            agent_id: agent.into(),
            call_id: id.into(),
            tool: tool.into(),
            summary: path.into(),
            command: None,
            note: String::new(),
            todos: None,
            background: false,
            at,
        }
    }

    #[test]
    fn edits_outweigh_reads_and_labels_are_short() {
        let mut s = Session::new("s", "p", 1);
        s.apply(call(ROOT_AGENT, "1", "Read", "D:\\repo\\src\\app\\ui.rs", 10));
        s.apply(call(ROOT_AGENT, "2", "Read", "D:\\repo\\src\\app\\ui.rs", 20));
        s.apply(call(ROOT_AGENT, "3", "Read", "D:\\repo\\src\\app\\ui.rs", 30));
        s.apply(call(ROOT_AGENT, "4", "Edit", "D:/repo/src/data/model.rs", 40));
        s.apply(call(ROOT_AGENT, "5", "Bash", "cargo test", 50));
        s.apply(call(ROOT_AGENT, "6", "Grep", "fn main", 60));
        let hot = hot_files(&s, 10, 100, 65);
        assert_eq!(hot.len(), 2);
        assert_eq!(hot[0].label, "data/model.rs");
        assert_eq!((hot[0].edits, hot[0].reads, hot[0].heat()), (1, 0, 3));
        assert_eq!(hot[1].label, "app/ui.rs");
        assert_eq!(hot[1].reads, 3);
        assert_eq!(
            hot[1].touches.len(),
            0,
            "all three reads are older than the recent window"
        );
        assert_eq!(hot[0].touches.len(), 1);
        assert!(hot[0].touches[0].edit);
        assert_eq!(label_for("ui.rs"), "ui.rs");
    }
}
