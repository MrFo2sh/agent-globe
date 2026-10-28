//! Claude Code transcript source.
//!
//! Layout on disk (read-only, never modified):
//! `<claude_dir>/projects/<project>/<session>.jsonl` and
//! `<claude_dir>/projects/<project>/<session>/subagents/agent-<id>.jsonl` + `agent-<id>.meta.json`.
//! The meta's `toolUseId` is the `Agent` tool call in the parent that spawned the subagent.

use super::model::{Event, Millis, Progress, ROOT_AGENT, parse_millis};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const OUTPUT_CAP: usize = 16 * 1024;
const TEXT_CAP: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRef {
    pub id: String,
    pub project: String,
    pub path: PathBuf,
    pub modified: Millis,
    pub size: u64,
}

impl SessionRef {
    pub fn subagents_dir(&self) -> PathBuf {
        self.path.with_extension("").join("subagents")
    }
}

/// `$CLAUDE_CONFIG_DIR` or `~/.claude`.
pub fn default_claude_dir() -> PathBuf {
    if let Ok(d) = std::env::var("CLAUDE_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".claude")
}

fn file_millis(meta: &fs::Metadata) -> Millis {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as Millis)
        .unwrap_or(0)
}

/// All session transcripts under `<claude_dir>/projects`, newest first.
pub fn discover(claude_dir: &Path) -> Vec<SessionRef> {
    let mut out = Vec::new();
    let projects = claude_dir.join("projects");
    let Ok(dirs) = fs::read_dir(&projects) else { return out };
    for proj in dirs.flatten() {
        let Ok(ft) = proj.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let project = proj.file_name().to_string_lossy().to_string();
        let Ok(files) = fs::read_dir(proj.path()) else { continue };
        for f in files.flatten() {
            let path = f.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(meta) = f.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let id = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
            out.push(SessionRef {
                id,
                project: project.clone(),
                path,
                modified: file_millis(&meta),
                size: meta.len(),
            });
        }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.modified));
    out
}

struct FileState {
    offset: u64,
    partial: Vec<u8>,
    agent_id: String,
}

/// Incrementally reads a session and its subagents, emitting events for appended bytes only.
pub struct Tailer {
    session: SessionRef,
    files: HashMap<PathBuf, FileState>,
    started: bool,
    pub parse_errors: u32,
}

impl Tailer {
    pub fn new(session: SessionRef) -> Self {
        Tailer {
            session,
            files: HashMap::new(),
            started: false,
            parse_errors: 0,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session.id
    }

    /// Read everything new since the last poll. The first poll reads whole files.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        if !self.started {
            self.started = true;
            events.push(Event::SessionStart {
                session_id: self.session.id.clone(),
                project: self.session.project.clone(),
                at: 0,
            });
            self.files.insert(
                self.session.path.clone(),
                FileState {
                    offset: 0,
                    partial: Vec::new(),
                    agent_id: ROOT_AGENT.to_string(),
                },
            );
        }
        self.discover_subagents(&mut events);
        let paths: Vec<PathBuf> = self.files.keys().cloned().collect();
        for p in paths {
            self.read_appended(&p, &mut events);
        }
        events
    }

    fn discover_subagents(&mut self, events: &mut Vec<Event>) {
        let dir = self.session.subagents_dir();
        let Ok(entries) = fs::read_dir(&dir) else { return };
        let mut new_files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
            .filter(|p| !self.files.contains_key(p))
            .collect();
        new_files.sort();
        for p in new_files {
            let stem = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
            let agent_id = stem.strip_prefix("agent-").unwrap_or(&stem).to_string();
            let meta_path = p.with_file_name(format!("{stem}.meta.json"));
            let meta: Value = fs::read_to_string(&meta_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(Value::Null);
            let model = meta["model"]
                .as_str()
                .filter(|m| *m != "inherit")
                .unwrap_or("")
                .to_string();
            events.push(Event::AgentSpawn {
                session_id: self.session.id.clone(),
                agent_id: agent_id.clone(),
                parent_id: None,
                spawn_tool_use_id: meta["toolUseId"].as_str().map(String::from),
                name: meta["description"].as_str().unwrap_or("").to_string(),
                kind: meta["agentType"].as_str().unwrap_or("subagent").to_string(),
                model,
                prompt: String::new(),
                at: fs::metadata(&p).map(|m| file_millis(&m)).unwrap_or(0),
            });
            self.files.insert(
                p,
                FileState {
                    offset: 0,
                    partial: Vec::new(),
                    agent_id,
                },
            );
        }
    }

    fn read_appended(&mut self, path: &Path, events: &mut Vec<Event>) {
        let Some(state) = self.files.get_mut(path) else { return };
        let Ok(meta) = fs::metadata(path) else { return };
        let len = meta.len();
        if len < state.offset {
            // Truncated or rotated: start over for this file.
            state.offset = 0;
            state.partial.clear();
        }
        if len == state.offset {
            return;
        }
        let Ok(mut f) = fs::File::open(path) else { return };
        if f.seek(SeekFrom::Start(state.offset)).is_err() {
            return;
        }
        let mut buf = Vec::with_capacity((len - state.offset) as usize);
        if f.read_to_end(&mut buf).is_err() {
            return;
        }
        state.offset = len;
        let mut data = std::mem::take(&mut state.partial);
        data.extend_from_slice(&buf);
        let mut start = 0;
        while let Some(nl) = data[start..].iter().position(|&b| b == b'\n') {
            let line = &data[start..start + nl];
            start += nl + 1;
            if line.iter().all(|b| b.is_ascii_whitespace()) {
                continue;
            }
            match serde_json::from_slice::<Value>(line) {
                Ok(v) => parse_record(&v, &self.session.id, &state.agent_id, events),
                Err(_) => self.parse_errors += 1,
            }
        }
        state.partial = data[start..].to_vec();
    }
}

fn truncate(s: &str, cap: usize) -> String {
    if s.len() <= cap {
        return s.to_string();
    }
    let mut end = cap;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… [{} more bytes]", &s[..end], s.len() - end)
}

fn content_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Short human label for a tool call, from its name and input.
pub fn summarize_tool(name: &str, input: &Value) -> String {
    let pick = |keys: &[&str]| keys.iter().find_map(|k| input[k].as_str()).map(|s| s.to_string());
    let s = match name {
        "Bash" | "PowerShell" => pick(&["description", "command"]),
        "Read" | "Write" | "Edit" | "NotebookEdit" => pick(&["file_path", "notebook_path"]),
        "Agent" | "Task" => pick(&["description"]),
        "Grep" | "Glob" => pick(&["pattern"]),
        "WebFetch" => pick(&["url"]),
        "WebSearch" => pick(&["query"]),
        "Skill" => pick(&["skill"]),
        _ => input
            .as_object()
            .and_then(|o| o.values().find_map(|v| v.as_str()))
            .map(String::from),
    };
    let s = s.unwrap_or_else(|| input.to_string());
    truncate(s.lines().next().unwrap_or(""), 120)
}

/// Turn one transcript record into zero or more events. `agent_id` is the file's owner.
pub fn parse_record(v: &Value, session_id: &str, agent_id: &str, events: &mut Vec<Event>) {
    let at = parse_millis(v["timestamp"].as_str());
    let agent = v["agentId"].as_str().unwrap_or(agent_id);
    match v["type"].as_str() {
        Some("assistant") => {
            let msg = &v["message"];
            let model = msg["model"].as_str().unwrap_or("").to_string();
            let usage = &msg["usage"];
            let output_tokens = usage["output_tokens"].as_u64().unwrap_or(0);
            let input_tokens = usage["input_tokens"].as_u64().unwrap_or(0);
            let cache_read_tokens = usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
            let cache_write_tokens = usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
            let context_tokens = input_tokens + cache_read_tokens + cache_write_tokens;
            // Text first: it is the narrator line for every tool call in the same message.
            let mut text = String::new();
            if let Some(blocks) = msg["content"].as_array() {
                for b in blocks {
                    if b["type"].as_str() == Some("text")
                        && let Some(t) = b["text"].as_str()
                    {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(t);
                    }
                }
            }
            let note = narrator_line(&text);
            if let Some(blocks) = msg["content"].as_array() {
                for b in blocks {
                    if b["type"].as_str() == Some("tool_use") {
                        let name = b["name"].as_str().unwrap_or("tool").to_string();
                        let input = &b["input"];
                        let command = match name.as_str() {
                            "Bash" | "PowerShell" => input["command"].as_str().map(String::from),
                            _ => None,
                        };
                        events.push(Event::ToolStart {
                            session_id: session_id.to_string(),
                            agent_id: agent.to_string(),
                            call_id: b["id"].as_str().unwrap_or("").to_string(),
                            tool: name.clone(),
                            summary: summarize_tool(&name, input),
                            command,
                            note: note.clone(),
                            todos: if name == "TodoWrite" {
                                todo_progress(input)
                            } else {
                                None
                            },
                            at,
                        });
                    }
                }
            }
            events.push(Event::Message {
                session_id: session_id.to_string(),
                agent_id: agent.to_string(),
                role: "assistant".into(),
                text: truncate(&text, TEXT_CAP),
                model,
                output_tokens,
                context_tokens,
                input_tokens,
                cache_read_tokens,
                cache_write_tokens,
                cwd: v["cwd"].as_str().unwrap_or("").to_string(),
                at,
            });
        }
        Some("system") => {
            if v["subtype"].as_str() == Some("turn_duration") {
                events.push(Event::TurnEnd {
                    session_id: session_id.to_string(),
                    agent_id: agent.to_string(),
                    duration_ms: v["durationMs"].as_u64().unwrap_or(0),
                    at,
                });
            }
        }
        Some("user") => {
            let content = &v["message"]["content"];
            let is_meta = v["isMeta"].as_bool().unwrap_or(false);
            if let Some(blocks) = content.as_array() {
                let mut text = String::new();
                for b in blocks {
                    match b["type"].as_str() {
                        Some("tool_result") => {
                            let r = &v["toolUseResult"];
                            let (stdout, stderr) =
                                if r.is_object() && (r.get("stdout").is_some() || r.get("stderr").is_some()) {
                                    (
                                        r["stdout"].as_str().unwrap_or("").to_string(),
                                        r["stderr"].as_str().unwrap_or("").to_string(),
                                    )
                                } else {
                                    (content_text(&b["content"]), String::new())
                                };
                            let is_error = match &b["is_error"] {
                                Value::Bool(x) => *x,
                                Value::String(s) => s.eq_ignore_ascii_case("true"),
                                _ => false,
                            };
                            events.push(Event::ToolEnd {
                                session_id: session_id.to_string(),
                                agent_id: agent.to_string(),
                                call_id: b["tool_use_id"].as_str().unwrap_or("").to_string(),
                                stdout: truncate(&stdout, OUTPUT_CAP),
                                stderr: truncate(&stderr, OUTPUT_CAP),
                                is_error,
                                at,
                            });
                        }
                        Some("text") if !is_meta => {
                            if let Some(t) = b["text"].as_str() {
                                text.push_str(&strip_tag_blocks(t));
                            }
                        }
                        _ => {}
                    }
                }
                if !text.trim().is_empty() {
                    events.push(user_message(
                        session_id,
                        agent,
                        &text,
                        v["cwd"].as_str().unwrap_or(""),
                        at,
                    ));
                }
            } else if let Some(s) = content.as_str()
                && !is_meta
                && !s.trim().is_empty()
            {
                events.push(user_message(session_id, agent, s, v["cwd"].as_str().unwrap_or(""), at));
            }
        }
        _ => {}
    }
}

/// Done / total and the in-progress item from a TodoWrite input.
pub fn todo_progress(input: &Value) -> Option<Progress> {
    let todos = input["todos"].as_array()?;
    let total = todos.len() as u32;
    let done = todos
        .iter()
        .filter(|t| t["status"].as_str() == Some("completed"))
        .count() as u32;
    let current = todos
        .iter()
        .find(|t| t["status"].as_str() == Some("in_progress"))
        .and_then(|t| t["activeForm"].as_str().or_else(|| t["content"].as_str()))
        .unwrap_or("")
        .to_string();
    Some(Progress { done, total, current })
}

/// First meaningful sentence of what the agent said, for the narrator. Skips markdown noise.
pub fn narrator_line(text: &str) -> String {
    let cleaned: String = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("```") && !l.starts_with('|'))
        .collect::<Vec<_>>()
        .join(" ");
    let cleaned = cleaned.replace("**", "").replace('`', "");
    let end = cleaned
        .char_indices()
        .find(|(i, c)| matches!(c, '.' | '!' | '?') && *i > 20)
        .map(|(i, _)| i + 1)
        .unwrap_or(cleaned.len());
    truncate(cleaned[..end].trim(), 180)
}

/// Remove harness-injected `<tag>...</tag>` blocks (system reminders, IDE selection) so the
/// human's own words remain. Unclosed tags are left alone.
pub fn strip_tag_blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let name_end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(after.len());
        let name = &after[..name_end];
        let close = format!("</{name}>");
        if !name.is_empty()
            && after[name_end..].starts_with(['>', ' '])
            && let Some(end) = rest[start..].find(&close)
        {
            out.push_str(&rest[..start]);
            rest = &rest[start + end + close.len()..];
        } else {
            out.push_str(&rest[..=start]);
            rest = &rest[start + 1..];
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

fn user_message(session_id: &str, agent: &str, text: &str, cwd: &str, at: Millis) -> Event {
    Event::Message {
        session_id: session_id.to_string(),
        agent_id: agent.to_string(),
        role: "user".into(),
        text: truncate(text, TEXT_CAP),
        model: String::new(),
        output_tokens: 0,
        context_tokens: 0,
        input_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        cwd: cwd.to_string(),
        at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_common_tools() {
        let v = serde_json::json!({"command": "ls -la", "description": "List files"});
        assert_eq!(summarize_tool("Bash", &v), "List files");
        let v = serde_json::json!({"file_path": "C:/x/y.rs"});
        assert_eq!(summarize_tool("Read", &v), "C:/x/y.rs");
        let v = serde_json::json!({"weird": 1});
        assert!(summarize_tool("Other", &v).contains("weird"));
    }

    #[test]
    fn strips_injected_tag_blocks_but_keeps_the_prompt() {
        let t = "<system-reminder>
noise
</system-reminder>
test input <b>bold</b> 1 < 2";
        assert_eq!(strip_tag_blocks(t), "test input  1 < 2");
        assert_eq!(strip_tag_blocks("plain"), "plain");
    }

    #[test]
    fn todo_write_becomes_progress() {
        let v = serde_json::json!({"todos": [
            {"content": "Write parser", "status": "completed", "activeForm": "Writing parser"},
            {"content": "Add tests", "status": "in_progress", "activeForm": "Adding tests"},
            {"content": "Ship", "status": "pending", "activeForm": "Shipping"}
        ]});
        let p = todo_progress(&v).unwrap();
        assert_eq!((p.done, p.total), (1, 3));
        assert_eq!(p.current, "Adding tests");
        assert!((p.fraction() - 1.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn truncates_on_char_boundary() {
        let s = "ééééééééé";
        let t = truncate(s, 5);
        assert!(t.starts_with("éé"));
        assert!(t.contains("more bytes"));
    }
}
