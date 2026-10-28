# Agent Globe

**One-liner:** A GPU-rendered 3D globe that shows every coding-agent session, its subagents, and their terminal executions live. Click any node to inspect it.
**Blueprint:** factory/blueprints/desktop-app.md
**Date:** 2026-10-28

## Problem
When a coding agent fans out into subagents and runs dozens of commands, the person watching has a
scrolling text log and no picture of what is running, what spawned what, and where the time goes.
They want the agent on one screen and a live map of its work on the other.

## MVP (ships today)
1. **Vertical slice:** open the app, it lists sessions found under the Claude config dir, pick one,
   the agent tree renders as a globe, click a node, see its tool calls and Bash output.
2. Globe layout: root at the north pole, depth = latitude ring, spawn order = longitude, arcs for
   spawn links. Orbit camera: drag rotate, scroll zoom. Hover highlights, click selects.
3. Node panel (egui): agent name/type, model, status, duration, prompt excerpt, tool-call timeline,
   terminal-style block per Bash call with command, stdout, stderr, exit flag.
4. Live tail: a watcher thread tails the session file and its `subagents/` folder, parses only
   appended bytes, sends deltas over a channel; the scene updates with no frame hitch.
5. HTTP ingest on `127.0.0.1:4711` accepting the event schema (JSON lines or one object per POST),
   feeding the same model as the file source. README ships a Claude Code hooks snippet.
6. Status strip: active agents, tool calls per minute, parse errors, fps.

## Not in MVP
Codex file source, OTLP receiver, MCP adapter, time scrubber, multi-globe, cost, file-conflict
warnings, session search, saving/exporting. All in BACKLOG.md.

## Interfaces
- Window: left sidebar (projects/sessions), center 3D viewport, right panel (selected node),
  bottom status strip.
- `POST http://127.0.0.1:4711/events` body: one JSON event or newline-delimited events.
- CLI flags: `--claude-dir <path>`, `--port <n>`, `--session <id>` (open directly), `--no-http`.

## Data
`data/model.rs`:
- `Session { id, project, path, root: AgentId, agents: Vec<Agent>, started, last_activity }`
- `Agent { id, parent: Option<AgentId>, spawn_tool_use_id, name, kind, model, depth, status, started, ended, prompt, calls: Vec<ToolCall>, tokens }`
- `ToolCall { id, name, input_summary, command, stdout, stderr, is_error, started, ended }`
- `Event` enum (ingest schema): `SessionStart, AgentSpawn, ToolStart, ToolEnd, Message, AgentEnd`.
Sources produce `Event`s; `model::apply(&mut Session, Event)` is the single reducer. Globe layout
is a pure function of the session.

## Risks
- Bevy compile time on first build (minutes). Mitigation: dev profile opt-level tweaks, build once.
- MSVC build tools still installing. Mitigation: data layer is pure Rust, tests run as soon as the
  linker exists.
- bevy_egui / bevy version mismatch. Mitigation: pin versions verified against crates.io deps.
- Picking on many entities. Mitigation: shared mesh, mesh picking backend, cap at 2000 nodes for MVP.

## Done when
- Launch with no flags on this machine: sidebar lists existing projects and sessions.
- Select the session of the current VS Code chat: globe shows the root and every subagent.
- Click a subagent: panel shows its Bash commands with real stdout.
- Start a new subagent in VS Code: a new node appears on the globe within 2 seconds.
- `curl -X POST 127.0.0.1:4711/events` with a sample event adds a node.
- `cargo test` passes for parsers, reducer, and layout. `check` is green.
- Frame time under 16 ms with the demo session on the integrated AMD GPU.

---
Source idea: ideas/inbox/001-agent-globe.md
