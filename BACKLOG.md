# Backlog — Agent Globe

Not done yet, most valuable first. Shipped items live in CHANGELOG.md.

- Timeline scrubber: drag back through a session and replay it; stalls show as gaps.
- Keyboard navigation: j/k through calls, Enter expands, number keys pick units, / focuses search.
- Session export: one button writes the session as a Markdown report (prompts, decisions, files changed, cost).
- Conflict radar: red link between two units that touched the same file within a minute.
- Promote to rule: a button on a call or turn that appends a line to the project's CLAUDE.md.
- Spectator mode: read-only web view of a session on the LAN (phosphor look) for a classroom projector.
- Token weather: cache-write spikes (prompt cache invalidated) shown as a dust storm over the constellation.
- Ghost run: overlay the previous run of the same prompt in grey.
- Resume briefing: show the transcript's compaction summary on the commander card after a break.
- Prompt coach: one line per turn on what the prompt lacked when the agent looped or asked back.
- Codex transcript source (`~/.codex/sessions/**/rollout-*.jsonl`) into the same event schema.
- Remote sessions over SSH or a shared folder, shown as satellites.
- Approval policies: auto-allow safe commands, always ask for destructive ones.
- Unit nicknames per session instead of ids in the sidebar and HUD.
- Alert sound packs besides the Army Men voices.
- Signed installers: `cargo wix` on Windows, `cargo bundle` on macOS.
- Re-check RUSTSEC-2026-0344 (ruzstd via bevy_image) and drop the audit.toml exception once Bevy bumps it.
- Verify frame rate on the integrated AMD GPU (only the RTX 3090 is measured: 200–400 fps).
