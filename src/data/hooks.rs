//! Maps Claude Code hook payloads (what a `command` hook receives on stdin) to events.
//! Best effort: hooks give instant signals; the file tailer remains the source of truth.

use super::model::{Event, ROOT_AGENT, now_millis};
use serde_json::Value;

pub fn from_claude_hook(v: &Value) -> Option<Event> {
    let session_id = v["session_id"].as_str()?.to_string();
    let agent_id = v["agent_id"].as_str().unwrap_or(ROOT_AGENT).to_string();
    let at = now_millis();
    let event = v["hook_event_name"].as_str()?;
    Some(match event {
        "SessionStart" => Event::SessionStart {
            session_id,
            project: v["cwd"].as_str().unwrap_or("").to_string(),
            at,
        },
        "UserPromptSubmit" => Event::Message {
            session_id,
            agent_id,
            role: "user".into(),
            text: v["prompt"].as_str().unwrap_or("").to_string(),
            model: String::new(),
            output_tokens: 0,
            context_tokens: 0,
            input_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cwd: v["cwd"].as_str().unwrap_or("").to_string(),
            at,
        },
        "PreToolUse" => {
            let tool = v["tool_name"].as_str().unwrap_or("tool").to_string();
            let input = &v["tool_input"];
            Event::ToolStart {
                session_id,
                agent_id,
                call_id: v["tool_use_id"].as_str().unwrap_or("").to_string(),
                summary: super::claude::summarize_tool(&tool, input),
                command: input["command"].as_str().map(String::from),
                tool,
                note: String::new(),
                todos: None,
                at,
            }
        }
        "PostToolUse" => {
            let r = &v["tool_response"];
            let (stdout, stderr) = match r {
                Value::Object(_) => (
                    r["stdout"].as_str().unwrap_or("").to_string(),
                    r["stderr"].as_str().unwrap_or("").to_string(),
                ),
                Value::String(s) => (s.clone(), String::new()),
                _ => (r.to_string(), String::new()),
            };
            Event::ToolEnd {
                session_id,
                agent_id,
                call_id: v["tool_use_id"].as_str().unwrap_or("").to_string(),
                stdout,
                stderr,
                is_error: r["is_error"].as_bool().unwrap_or(false),
                at,
            }
        }
        "SubagentStart" => Event::AgentSpawn {
            session_id,
            agent_id: v["agent_id"].as_str()?.to_string(),
            parent_id: None,
            spawn_tool_use_id: v["tool_use_id"].as_str().map(String::from),
            name: v["agent_type"].as_str().unwrap_or("").to_string(),
            kind: v["agent_type"].as_str().unwrap_or("subagent").to_string(),
            model: String::new(),
            prompt: String::new(),
            at,
        },
        "SubagentStop" => Event::AgentEnd {
            session_id,
            agent_id: v["agent_id"].as_str()?.to_string(),
            at,
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_pre_tool_use() {
        let v = serde_json::json!({
            "session_id": "s1", "hook_event_name": "PreToolUse", "tool_name": "Bash",
            "tool_use_id": "t1", "tool_input": {"command": "cargo test", "description": "Run tests"}
        });
        match from_claude_hook(&v).unwrap() {
            Event::ToolStart {
                call_id,
                tool,
                command,
                summary,
                ..
            } => {
                assert_eq!(call_id, "t1");
                assert_eq!(tool, "Bash");
                assert_eq!(command.as_deref(), Some("cargo test"));
                assert_eq!(summary, "Run tests");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn unknown_hook_is_none() {
        let v = serde_json::json!({"session_id": "s", "hook_event_name": "Notification"});
        assert!(from_claude_hook(&v).is_none());
    }
}
