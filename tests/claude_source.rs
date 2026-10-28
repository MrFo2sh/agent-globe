use agent_globe::data::claude::{Tailer, discover};
use agent_globe::data::layout::layout;
use agent_globe::data::model::{ROOT_AGENT, Session};
use std::fs;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for e in fs::read_dir(src).unwrap().flatten() {
        let d = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &d);
        } else {
            fs::copy(e.path(), d).unwrap();
        }
    }
}

#[test]
fn discovers_sessions_newest_first() {
    let found = discover(&fixtures());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].project, "demo-project");
    assert!(found[0].id.starts_with("11111111"));
}

#[test]
fn builds_tree_with_bash_output_and_subagent() {
    let r = discover(&fixtures()).remove(0);
    let mut t = Tailer::new(r.clone());
    let events = t.poll();
    let mut s = Session::new(&r.id, &r.project, 0);
    s.apply_all(events);

    assert_eq!(s.agents.len(), 2);
    let root = s.root();
    assert_eq!(root.prompt, "Add a README and run the tests");
    assert_eq!(root.calls.len(), 2);
    assert_eq!(root.calls[0].tool, "Bash");
    assert_eq!(root.calls[0].command.as_deref(), Some("ls -la"));
    assert_eq!(root.calls[0].stdout, "README.md\nsrc");
    assert_eq!(root.output_tokens, 72);

    let child = s.agent("abc123").unwrap();
    assert_eq!(child.parent.as_deref(), Some(ROOT_AGENT));
    assert_eq!(child.depth, 1);
    assert_eq!(child.name, "Run the test suite");
    assert_eq!(child.kind, "fork");
    assert_eq!(child.calls.len(), 1);
    assert_eq!(child.calls[0].stderr, "warning: unused import");
    assert!(child.ended.is_some(), "spawn tool_result should mark child done");

    let lay = layout(&s);
    assert_eq!(lay.nodes.len(), 2);
    assert!(t.poll().is_empty(), "second poll with no change yields nothing");
}

#[test]
fn tails_appended_lines_and_new_subagents() {
    let tmp = std::env::temp_dir().join(format!("agent-globe-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    copy_dir(&fixtures(), &tmp);
    let r = discover(&tmp).remove(0);
    let mut t = Tailer::new(r.clone());
    let mut s = Session::new(&r.id, &r.project, 0);
    s.apply_all(t.poll());
    assert_eq!(s.root().calls.len(), 2);

    // Append a new Bash call to the main transcript, written in two chunks to test partial lines.
    let line = r#"{"type":"assistant","message":{"model":"claude-opus-5-5","role":"assistant","content":[{"type":"tool_use","id":"toolu_bash3","name":"Bash","input":{"command":"git status","description":"Status"}}],"usage":{"output_tokens":5}},"uuid":"a3","timestamp":"2026-10-28T10:02:00.000Z"}"#;
    let (head, tail) = line.split_at(40);
    use std::io::Write;
    let mut f = fs::OpenOptions::new().append(true).open(&r.path).unwrap();
    f.write_all(head.as_bytes()).unwrap();
    f.flush().unwrap();
    s.apply_all(t.poll());
    assert_eq!(s.root().calls.len(), 2, "partial line must not be parsed yet");
    f.write_all(tail.as_bytes()).unwrap();
    f.write_all(b"\n").unwrap();
    f.flush().unwrap();
    s.apply_all(t.poll());
    assert_eq!(s.root().calls.len(), 3);
    assert_eq!(s.root().calls[2].command.as_deref(), Some("git status"));

    // A new subagent file appears.
    let sub = r.subagents_dir();
    fs::write(
        sub.join("agent-def456.meta.json"),
        r#"{"agentType":"explore","description":"Find callers","toolUseId":"toolu_bash3","spawnDepth":1}"#,
    )
    .unwrap();
    fs::write(sub.join("agent-def456.jsonl"), "").unwrap();
    s.apply_all(t.poll());
    assert_eq!(s.agents.len(), 3);
    assert_eq!(s.agent("def456").unwrap().parent.as_deref(), Some(ROOT_AGENT));
    assert_eq!(t.parse_errors, 0);
    let _ = fs::remove_dir_all(&tmp);
}
