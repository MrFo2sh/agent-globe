//! After-action report: what the session spent its time on, and what the human could do better
//! next time. Pure function over the session so it is testable.

use agent_globe::data::model::{ROOT_AGENT, Session};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
pub enum Category {
    Recon,
    Build,
    Ops,
    Command,
    Other,
}

impl Category {
    pub fn of(tool: &str) -> Category {
        match tool {
            "Read" | "Grep" | "Glob" | "LS" | "WebFetch" | "WebSearch" | "ToolSearch" => Category::Recon,
            "Edit" | "Write" | "NotebookEdit" | "MultiEdit" => Category::Build,
            "Bash" | "PowerShell" => Category::Ops,
            "Agent" | "Task" | "SendMessage" | "Skill" | "Workflow" => Category::Command,
            _ => Category::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Category::Recon => "recon (read, search)",
            Category::Build => "build (edit, write)",
            Category::Ops => "ops (commands)",
            Category::Command => "command (agents, skills)",
            Category::Other => "other",
        }
    }
}

#[derive(Debug, Default)]
pub struct Report {
    /// Label, value pairs for the stats table.
    pub rows: Vec<(String, String)>,
    /// Time share per category, 0..1, sorted descending.
    pub shares: Vec<(Category, f32, usize)>,
    /// Coaching lines, most important first.
    pub tips: Vec<String>,
}

fn dur(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m {}s", s / 60, s % 60)
    } else {
        format!("{}h {}m", s / 3600, (s % 3600) / 60)
    }
}

pub fn report(session: &Session) -> Report {
    let mut r = Report::default();
    let calls: Vec<_> = session.agents.iter().flat_map(|a| a.calls.iter()).collect();
    let total_calls = calls.len();
    let errors = calls.iter().filter(|c| c.is_error).count();
    let subagents = session.agents.len().saturating_sub(1);
    let tokens_out: u64 = session.agents.iter().map(|a| a.output_tokens).sum();
    let ctx_peak = session.agents.iter().map(|a| a.context_tokens).max().unwrap_or(0);
    let duration = session.last_activity - session.started;

    // Time per category (only finished calls have a duration).
    let mut time: HashMap<Category, i64> = HashMap::new();
    let mut count: HashMap<Category, usize> = HashMap::new();
    for c in &calls {
        let cat = Category::of(&c.tool);
        *count.entry(cat).or_default() += 1;
        if let Some(e) = c.ended {
            *time.entry(cat).or_default() += (e - c.started).max(0);
        }
    }
    let total_time: i64 = time.values().sum::<i64>().max(1);
    let mut shares: Vec<(Category, f32, usize)> = count
        .keys()
        .map(|&k| {
            (
                k,
                time.get(&k).copied().unwrap_or(0) as f32 / total_time as f32,
                count[&k],
            )
        })
        .collect();
    shares.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    r.shares = shares;

    // Root-specific habits.
    let root = session.agent(ROOT_AGENT);
    let reads_before_edit = root
        .map(|a| {
            a.calls
                .iter()
                .take_while(|c| Category::of(&c.tool) != Category::Build)
                .filter(|c| Category::of(&c.tool) == Category::Recon)
                .count()
        })
        .unwrap_or(0);
    let mut repeats: HashMap<&str, usize> = HashMap::new();
    for c in calls.iter().filter_map(|c| c.command.as_deref()) {
        *repeats.entry(c).or_default() += 1;
    }
    let retried = repeats.values().filter(|&&n| n >= 2).count();
    let longest = calls
        .iter()
        .filter_map(|c| c.ended.map(|e| (e - c.started, c)))
        .max_by_key(|(d, _)| *d);
    let ops_share = r
        .shares
        .iter()
        .find(|s| s.0 == Category::Ops)
        .map(|s| s.1)
        .unwrap_or(0.0);
    let turns = root.map(|a| a.turns).unwrap_or(0);

    r.rows.push(("duration".into(), dur(duration)));
    r.rows.push(("turns".into(), turns.to_string()));
    r.rows.push(("tool calls".into(), total_calls.to_string()));
    r.rows.push(("failed calls".into(), errors.to_string()));
    r.rows.push(("subagents".into(), subagents.to_string()));
    r.rows.push(("output tokens".into(), format!("{}k", tokens_out / 1000)));
    r.rows.push(("context peak".into(), format!("{}k", ctx_peak / 1000)));
    r.rows
        .push(("reads before first edit".into(), reads_before_edit.to_string()));
    r.rows.push(("commands retried".into(), retried.to_string()));
    if let Some((d, c)) = longest {
        r.rows.push(("longest call".into(), format!("{} ({})", dur(d), c.tool)));
    }

    // Coaching. Each line states the evidence and the move.
    if reads_before_edit > 8 {
        r.tips.push(format!(
            "{reads_before_edit} reads before the first edit. Name the files and functions in your prompt; the agent spent its first minutes finding them."
        ));
    }
    if retried >= 2 {
        r.tips.push(format!(
            "{retried} commands were run more than once. Paste the failing output or the constraint into the prompt so the agent does not rediscover it."
        ));
    }
    if errors >= 3 {
        r.tips.push(format!(
            "{errors} tool calls failed. Open the red entries: a missing tool, a wrong path, or a permission is usually behind a cluster of failures."
        ));
    }
    if ops_share > 0.55 && total_calls > 20 {
        r.tips.push(
            "Over half the time went to running commands. Narrow the test or build scope in the prompt, or ask for a faster check first.".into(),
        );
    }
    if subagents == 0 && total_calls > 60 {
        r.tips.push(
            "A long single-threaded run. Independent parts (research, tests, docs) can go to parallel subagents; ask for it.".into(),
        );
    }
    if ctx_peak >= 150_000 {
        r.tips.push(format!(
            "Context peaked at {}k tokens. Past ~150k the agent forgets early instructions; split the task or start a fresh session with a summary.",
            ctx_peak / 1000
        ));
    }
    if subagents > 0 && errors == 0 && total_calls > 10 {
        r.tips
            .push("Clean run with delegation. This is the shape to repeat.".into());
    }
    if r.tips.is_empty() {
        r.tips
            .push("Nothing stands out yet. Debriefs get sharper after a few turns.".into());
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_globe::data::model::Event;

    fn call(s: &mut Session, agent: &str, id: &str, tool: &str, cmd: Option<&str>, at: i64) {
        s.apply(Event::ToolStart {
            session_id: "s".into(),
            agent_id: agent.into(),
            call_id: id.into(),
            tool: tool.into(),
            summary: String::new(),
            command: cmd.map(String::from),
            note: String::new(),
            todos: None,
            background: false,
            at,
        });
        s.apply(Event::ToolEnd {
            session_id: "s".into(),
            agent_id: agent.into(),
            call_id: id.into(),
            stdout: String::new(),
            stderr: String::new(),
            is_error: false,
            at: at + 1000,
        });
    }

    #[test]
    fn flags_reads_before_edit_and_retries() {
        let mut s = Session::new("s", "p", 1);
        for i in 0..10 {
            call(&mut s, ROOT_AGENT, &format!("r{i}"), "Read", None, 1000 + i * 2000);
        }
        call(&mut s, ROOT_AGENT, "e1", "Edit", None, 40_000);
        call(&mut s, ROOT_AGENT, "b1", "Bash", Some("cargo test"), 50_000);
        call(&mut s, ROOT_AGENT, "b2", "Bash", Some("cargo test"), 60_000);
        call(&mut s, ROOT_AGENT, "b3", "Bash", Some("cargo build"), 70_000);
        call(&mut s, ROOT_AGENT, "b4", "Bash", Some("cargo build"), 80_000);
        let r = report(&s);
        assert!(r.tips.iter().any(|t| t.contains("10 reads before the first edit")));
        assert!(r.tips.iter().any(|t| t.contains("2 commands were run more than once")));
        assert_eq!(r.shares[0].0, Category::Recon);
        assert!(r.rows.iter().any(|(k, v)| k == "tool calls" && v == "15"));
    }
}
