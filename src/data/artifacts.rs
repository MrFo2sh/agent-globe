//! Artifacts: the things an agent produces that a human wants to reach in one click.
//! Served URLs, files it wrote, images it looked at, commits, pull requests, test results.
//! Extracted from tool calls by pattern; no model involved.

use super::model::{Millis, ToolCall};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArtifactKind {
    Url,
    Pr,
    File,
    Image,
    Commit,
    Test,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub kind: ArtifactKind,
    /// Short text for the chip.
    pub label: String,
    /// What a click opens: URL, path, commit hash.
    pub target: String,
    pub call_id: String,
    pub at: Millis,
    /// For tests: Some(true) = green, Some(false) = red.
    pub ok: Option<bool>,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

static URL: OnceLock<Regex> = OnceLock::new();
static LOCALHOST: OnceLock<Regex> = OnceLock::new();
static CREATED: OnceLock<Regex> = OnceLock::new();
static COMMIT: OnceLock<Regex> = OnceLock::new();
static RUST_TEST: OnceLock<Regex> = OnceLock::new();
static PYTEST: OnceLock<Regex> = OnceLock::new();
static JEST: OnceLock<Regex> = OnceLock::new();
static CHECK: OnceLock<Regex> = OnceLock::new();

const IMAGE_EXT: [&str; 6] = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"];
const FILE_TOOLS: [&str; 4] = ["Write", "Edit", "NotebookEdit", "MultiEdit"];

fn is_image(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    IMAGE_EXT.iter().any(|e| p.ends_with(e))
}

fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

/// Reject matches that are templates or shell fragments rather than addresses.
fn plausible_url(u: &str) -> bool {
    let Some(rest) = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://")) else {
        return false;
    };
    let host = rest.split(['/', '?', '#', ':']).next().unwrap_or("");
    let bad_chars = u
        .chars()
        .any(|c| matches!(c, '$' | '*' | '|' | '%' | '{' | '}' | '<' | '>' | '`'));
    let numeric = host.chars().all(|c| c.is_ascii_digit() || c == '.') && host.contains('.');
    let hosty = host == "localhost" || numeric || (host.contains('.') && host.len() > 3);
    !bad_chars && hosty && !host.ends_with('.')
}

/// A created-file path must look like a path, not a word from a sentence.
fn plausible_path(p: &str) -> bool {
    (p.contains('/') || p.contains('\\') || p.contains('.')) && !p.contains("://") && p.len() > 2
}

fn trim_url(u: &str) -> &str {
    u.trim_end_matches(['.', ',', ';', ':', ')', ']', '"', '\'', '>'])
}

/// Everything worth a chip in one finished (or just started, for files) tool call.
pub fn from_call(call: &ToolCall) -> Vec<Artifact> {
    let mut out: Vec<Artifact> = Vec::new();
    let at = call.ended.unwrap_or(call.started);
    let mut push = |kind: ArtifactKind, label: String, target: String, ok: Option<bool>| {
        if !out.iter().any(|a| a.kind == kind && a.target == target) {
            out.push(Artifact {
                kind,
                label,
                target,
                call_id: call.id.clone(),
                at,
                ok,
            });
        }
    };

    // Files the agent wrote or edited: the summary holds the path for these tools.
    if FILE_TOOLS.contains(&call.tool.as_str()) && !call.summary.is_empty() {
        let kind = if is_image(&call.summary) {
            ArtifactKind::Image
        } else {
            ArtifactKind::File
        };
        push(kind, file_name(&call.summary), call.summary.clone(), None);
    }
    // Images the agent looked at.
    if call.tool == "Read" && is_image(&call.summary) {
        push(
            ArtifactKind::Image,
            file_name(&call.summary),
            call.summary.clone(),
            None,
        );
    }

    let text = format!("{}\n{}", call.stdout, call.stderr);

    // "File created successfully at: <path>" and friends.
    for cap in re(
        &CREATED,
        r"(?m)(?:created successfully at|has been (?:updated|created) successfully at|written to):?\s*([^\s(]+)",
    )
    .captures_iter(&text)
    {
        let path = cap[1].trim_end_matches(['.', ',']).to_string();
        if !plausible_path(&path) {
            continue;
        }
        let kind = if is_image(&path) {
            ArtifactKind::Image
        } else {
            ArtifactKind::File
        };
        push(kind, file_name(&path), path, None);
    }

    // URLs, with pull requests classified on their own. Ignore license/doc boilerplate links.
    for m in re(&URL, r#"https?://[^\s"'<>)\]]+"#).find_iter(&text) {
        // A server bound to all interfaces is reachable on localhost; show it that way.
        let normalized = trim_url(m.as_str())
            .replace("0.0.0.0", "localhost")
            .replace("[::]", "localhost");
        let u = normalized.as_str();
        if !plausible_url(u) {
            continue;
        }
        if u.contains("rustsec.org") || u.contains("support.claude.com") || u.contains("platform.claude.com") {
            continue;
        }
        if u.contains("github.com") && u.contains("/pull/") {
            push(
                ArtifactKind::Pr,
                format!("PR {}", u.rsplit('/').next().unwrap_or("")),
                u.to_string(),
                None,
            );
        } else {
            let label = u.trim_start_matches("https://").trim_start_matches("http://");
            push(ArtifactKind::Url, label.chars().take(48).collect(), u.to_string(), None);
        }
    }
    for m in re(
        &LOCALHOST,
        r"(?:^|[\s:])(localhost|127\.0\.0\.1|0\.0\.0\.0):(\d{2,5})(/[^\s]*)?",
    )
    .captures_iter(&text)
    {
        let host = if &m[1] == "0.0.0.0" { "localhost" } else { &m[1] };
        let url = format!("http://{host}:{}{}", &m[2], m.get(3).map(|p| p.as_str()).unwrap_or(""));
        push(ArtifactKind::Url, format!("{host}:{}", &m[2]), url, None);
    }

    // Commits: "[main 9f17909] message".
    if call
        .command
        .as_deref()
        .is_some_and(|c| c.contains("git commit") || c.contains("git tag"))
    {
        for cap in re(&COMMIT, r"(?m)^\[[^\]\s]+ (?:\(root-commit\) )?([0-9a-f]{7,40})\]").captures_iter(&text) {
            push(ArtifactKind::Commit, cap[1][..7].to_string(), cap[1].to_string(), None);
        }
    }

    // Test and build results.
    let mut tests: Option<(u64, u64)> = None;
    for cap in re(&RUST_TEST, r"test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed").captures_iter(&text) {
        let (p, f): (u64, u64) = (cap[1].parse().unwrap_or(0), cap[2].parse().unwrap_or(0));
        let t = tests.get_or_insert((0, 0));
        t.0 += p;
        t.1 += f;
    }
    if tests.is_none()
        && let Some(cap) = re(&PYTEST, r"(?m)=+ (?:(\d+) failed, )?(\d+) passed").captures(&text)
    {
        tests = Some((
            cap[2].parse().unwrap_or(0),
            cap.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0),
        ));
    }
    if tests.is_none()
        && let Some(cap) = re(&JEST, r"Tests:\s+(?:(\d+) failed, )?(\d+) passed").captures(&text)
    {
        tests = Some((
            cap[2].parse().unwrap_or(0),
            cap.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0),
        ));
    }
    if let Some((p, f)) = tests {
        let ok = f == 0;
        let label = if ok {
            format!("{p} passed")
        } else {
            format!("{f} failed")
        };
        push(ArtifactKind::Test, label, format!("tests:{}", call.id), Some(ok));
    } else if let Some(cap) = re(&CHECK, r"CHECK (PASSED|FAILED)").captures(&text) {
        let ok = &cap[1] == "PASSED";
        push(
            ArtifactKind::Test,
            if ok {
                "check passed".into()
            } else {
                "check failed".into()
            },
            format!("check:{}", call.id),
            Some(ok),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(tool: &str, summary: &str, command: Option<&str>, stdout: &str) -> ToolCall {
        ToolCall {
            id: "c1".into(),
            tool: tool.into(),
            summary: summary.into(),
            command: command.map(String::from),
            note: String::new(),
            stdout: stdout.into(),
            stderr: String::new(),
            is_error: false,
            started: 1,
            ended: Some(2),
            background: false,
        }
    }

    #[test]
    fn finds_served_urls_and_localhost() {
        let a = from_call(&call(
            "Bash",
            "dev",
            Some("pnpm dev"),
            "  ▲ Next.js 16\n  - Local:   http://localhost:3000\n  - Network: http://192.168.1.5:3000\nready",
        ));
        let urls: Vec<_> = a
            .iter()
            .filter(|x| x.kind == ArtifactKind::Url)
            .map(|x| x.target.clone())
            .collect();
        assert!(urls.contains(&"http://localhost:3000".to_string()));
        assert!(urls.contains(&"http://192.168.1.5:3000".to_string()));
        let b = from_call(&call(
            "Bash",
            "serve",
            Some("python -m http.server 8000"),
            "Serving HTTP on 0.0.0.0 port 8000 (http://0.0.0.0:8000/) ...",
        ));
        assert!(
            b.iter()
                .any(|x| x.target == "http://localhost:8000/" || x.target == "http://localhost:8000")
        );
    }

    #[test]
    fn classifies_files_images_commits_prs_tests() {
        let f = from_call(&call(
            "Write",
            "D:/x/src/app.rs",
            None,
            "File created successfully at: D:/x/src/app.rs",
        ));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, ArtifactKind::File);
        assert_eq!(f[0].label, "app.rs");
        let i = from_call(&call("Read", "C:/shots/ui.png", None, ""));
        assert_eq!(i[0].kind, ArtifactKind::Image);
        let c = from_call(&call(
            "Bash",
            "commit",
            Some("git commit -m x"),
            "[main 9f17909a] release: v0.1.0\n 3 files changed",
        ));
        assert_eq!(c[0].kind, ArtifactKind::Commit);
        assert_eq!(c[0].label, "9f17909");
        let p = from_call(&call(
            "Bash",
            "pr",
            Some("gh pr create"),
            "https://github.com/MrFo2sh/agent-globe/pull/12\n",
        ));
        assert_eq!(p[0].kind, ArtifactKind::Pr);
        assert_eq!(p[0].label, "PR 12");
        let t = from_call(&call(
            "Bash",
            "test",
            Some("cargo test"),
            "test result: ok. 10 passed; 0 failed; 0 ignored\ntest result: ok. 3 passed; 0 failed; 0 ignored",
        ));
        let t = t.iter().find(|x| x.kind == ArtifactKind::Test).unwrap();
        assert_eq!(t.label, "13 passed");
        assert_eq!(t.ok, Some(true));
        let bad = from_call(&call(
            "Bash",
            "test",
            Some("pytest"),
            "=================== 2 failed, 40 passed in 3.2s ===================",
        ));
        let bad = bad.iter().find(|x| x.kind == ArtifactKind::Test).unwrap();
        assert_eq!(bad.label, "2 failed");
        assert_eq!(bad.ok, Some(false));
    }

    #[test]
    fn rejects_templates_and_sentence_words() {
        let a = from_call(&call(
            "Bash",
            "x",
            Some("cat README"),
            "use $ZROK_URL or http://host:port and *|https://* then http://the
written to the file
File created successfully at: D:/x/y.txt",
        ));
        let targets: Vec<_> = a.iter().map(|x| x.target.clone()).collect();
        assert_eq!(targets, vec!["D:/x/y.txt".to_string()]);
        assert!(plausible_url("http://localhost:3000"));
        assert!(plausible_url("https://github.com/MrFo2sh/agent-globe"));
        assert!(!plausible_url("http://host:port"));
        assert!(!plausible_url("http://$ZROK_URL"));
    }

    #[test]
    fn ignores_boilerplate_links_and_dedups() {
        let a = from_call(&call(
            "Bash",
            "audit",
            Some("cargo audit"),
            "URL: https://rustsec.org/advisories/RUSTSEC-2026-0344\nhttp://localhost:4711/health\nhttp://localhost:4711/health",
        ));
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].target, "http://localhost:4711/health");
    }
}
