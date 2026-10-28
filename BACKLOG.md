# Backlog — Agent Globe

Everything cut from the MVP, one line each, most valuable first.

- Codex file source: tail `~/.codex/sessions/**/rollout-*.jsonl` into the same event schema.
- Time scrubber: replay a session from its first event at 1x–64x.
- Multiple sessions as multiple constellations side by side; pick which to focus.
- 3D labels (billboarded agent names) and a minimap.
- Sound: soft cues on spawn, finish, error (needs audio assets; keep opt-in).
- Achievements: first subagent, 100 calls, zero-error session, etc.
- Files-touched list per agent (from Read/Edit/Write calls) and conflict warnings across agents.
- Cost per agent from token usage and a model price table.
- Session search and filter in the sidebar; pin favorites.
- OpenTelemetry (OTLP/HTTP) receiver for agents that export traces.
- MCP adapter: a `report` tool for agents that have no hooks and no transcript files.
- Signed installers: `cargo wix` on Windows, `cargo bundle` on macOS.
- Hook-driven updates: the hooks snippet in README is best-effort until agent ids are confirmed in PreToolUse payloads.
- Re-check RUSTSEC-2026-0344 (ruzstd via bevy_image) and remove the audit.toml exception once Bevy bumps ruzstd >= 0.9.1.
- Verify 60 fps on the integrated AMD GPU (only the RTX 3090 has been measured: ~200 fps).
