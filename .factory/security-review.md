# Security review — agent-globe (2026-10-28)

1. Inputs: CLI flags (clap, typed); transcript files under the Claude dir (parsed as JSON, never executed); HTTP bodies on 127.0.0.1 (serde-validated into the Event enum, 4 MB cap). Validated in `data/claude.rs`, `bridge.rs`.
2. Files: `--claude-dir` is the only user path; reads are confined to `<dir>/projects/**/*.jsonl` and `*.meta.json`. No writes under the Claude dir, ever.
3. Shell: no subprocesses.
4. Network: inbound only, bound to 127.0.0.1 (loopback), no auth needed because it is not reachable off-host; `--no-http` disables it. No outbound calls.
5. Secrets: none used. Transcripts may contain secrets the agent printed; they are displayed locally only and never leave the process.
6. Dependencies: Cargo.lock committed; `cargo audit` runs in `check`.
7. Permissions: user-level file reads; no elevated rights.
8. Data: nothing stored by the app. In-memory only; cleared on exit.
9. DoS: tool output capped at 16 KB per call, messages at 2 KB, HTTP body at 4 MB, errors list at 50, polling at 300 ms.
10. Defensive scope: observation tool. It cannot act on agents or systems.
