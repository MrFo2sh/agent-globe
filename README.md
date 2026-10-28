# Agent Globe

A GPU-rendered, game-engine-powered view of what your coding agents are doing, live. Each session
is a constellation: the root agent is a glowing core, every subagent depth is an orbit ring, links
are light beams with comets travelling along them while a tool call runs. Nodes pulse while busy,
dim when done, turn red on errors. Click a node and the camera flies to it; its panel shows the
prompt, every tool call in order, and each Bash command with real stdout and stderr.

Rust, Bevy (wgpu, Vulkan/DX12/Metal), egui. HDR bloom, MSAA, fog. Army Men theme: green plastic
soldiers on sand rings, a radio commander voice, stencil headings. No web view, no Electron.

Put VS Code with your agent on one screen and this on the other.

## What it shows (0.8)

- Every session modified in the last 30 minutes is tailed at once; other active sessions orbit as
  satellites, and a "Needs you" queue lists approvals, failures, waiting commanders (with the
  question they asked) and stalls across all of them.
- Unit panel: artifacts shelf (URLs, files, images, commits, test results), files changed, tool
  calls grouped under each of your prompts with calls, units and time per turn, and a history
  search. Images open in the app; every other file opens in VS Code.
- File ring: the session's hottest files on an outer orbit with beams from the units touching them.
- Radio: twenty spoken lines that let you follow a session by ear, from "Scouting ahead" to
  "We have casualties!" and "Need your input, commander". Settings list what each one means.
- Approvals from the globe (PreToolUse hook) with global Ctrl+Alt+Y / Ctrl+Alt+N hotkeys; 🖥 jumps
  to the session's VS Code window; radio voice lines, badges, progress rings, debrief coach.

## Quickstart (60 seconds)

```
cargo run --release
```

It finds Claude Code sessions under `~/.claude/projects` (or `$CLAUDE_CONFIG_DIR`), opens the most
recent one, and tails it. Start a subagent in your editor and watch a new node fly out to its orbit.

| Input | Action |
|---|---|
| left-drag / scroll / right-drag | orbit / zoom / pan (sensitivity in ⚙ settings) |
| hover a node | small card at the top right |
| click a node | full panel on the right; camera flies to it |
| Esc or ✕ | close the panel, camera flies home |
| R or ⌂ home | reset the view |
| idle 8 s with nothing selected | slow cinematic orbit (toggle in settings) |

Scrolling or dragging over a panel never moves the camera.

```
agent-globe --help
  --claude-dir <PATH>   Claude config dir (default: $CLAUDE_CONFIG_DIR or ~/.claude)
  --port <N>            HTTP ingest port (default 4711)
  --no-http             disable the ingest endpoint
  --session <ID>        open this session (prefix ok) on start
```

Settings (camera speeds, drift, bloom, UI scale) persist to `<config dir>/agent-globe/settings.json`.

## How agents feed it

| Source | Config | Latency |
|---|---|---|
| Claude Code transcripts (file tail) | none | ~300 ms |
| Claude Code hooks → `POST /hooks/claude` | one snippet, below | instant |
| Any agent → `POST /events` with the event schema | your hook or wrapper | instant |

### Claude Code hooks snippet

Add to `~/.claude/settings.json` (or the project `.claude/settings.json`). Every listed event is
POSTed to the app; if the app is not running the hook fails silently.

```json
{
  "hooks": {
    "PreToolUse":      [{ "hooks": [{ "type": "command", "command": "curl -s -m 1 -X POST http://127.0.0.1:4711/hooks/claude -H 'content-type: application/json' --data-binary @- >/dev/null 2>&1 || true" }] }],
    "PostToolUse":     [{ "hooks": [{ "type": "command", "command": "curl -s -m 1 -X POST http://127.0.0.1:4711/hooks/claude -H 'content-type: application/json' --data-binary @- >/dev/null 2>&1 || true" }] }],
    "SubagentStart":   [{ "hooks": [{ "type": "command", "command": "curl -s -m 1 -X POST http://127.0.0.1:4711/hooks/claude -H 'content-type: application/json' --data-binary @- >/dev/null 2>&1 || true" }] }],
    "SubagentStop":    [{ "hooks": [{ "type": "command", "command": "curl -s -m 1 -X POST http://127.0.0.1:4711/hooks/claude -H 'content-type: application/json' --data-binary @- >/dev/null 2>&1 || true" }] }],
    "UserPromptSubmit":[{ "hooks": [{ "type": "command", "command": "curl -s -m 1 -X POST http://127.0.0.1:4711/hooks/claude -H 'content-type: application/json' --data-binary @- >/dev/null 2>&1 || true" }] }]
  }
}
```

### Event schema (`POST /events`)

One JSON object, a JSON array, or newline-delimited objects. `at` is Unix millis (0 = unknown).

```json
{"type":"session_start","session_id":"s1","project":"my-repo","at":0}
{"type":"agent_spawn","session_id":"s1","agent_id":"a1","parent_id":"root","name":"Run tests","kind":"fork","model":"claude-opus-5-5","prompt":"...","at":0}
{"type":"tool_start","session_id":"s1","agent_id":"a1","call_id":"c1","tool":"Bash","summary":"Run tests","command":"cargo test","at":0}
{"type":"tool_end","session_id":"s1","agent_id":"a1","call_id":"c1","stdout":"ok","stderr":"","is_error":false,"at":0}
{"type":"message","session_id":"s1","agent_id":"a1","role":"assistant","text":"done","model":"claude-opus-5-5","output_tokens":120,"at":0}
{"type":"agent_end","session_id":"s1","agent_id":"a1","at":0}
```

`parent_id` may be omitted if `spawn_tool_use_id` names the parent's tool call; the app resolves it.
The root agent of every session has id `root`.

```
curl -X POST 127.0.0.1:4711/events -d '{"type":"agent_spawn","session_id":"demo","agent_id":"x1","parent_id":"root","name":"hello"}'
```

## Artifacts, approvals, alerts

- **Artifacts.** Every served URL, written file, viewed image, commit, pull request and test
  result an agent produces becomes a chip in its two-row shelf (expand to see all, with search) and,
  for 90 seconds, a speech bubble above the unit. Click: browser, VS Code at the file, the commit on
  GitHub, or the built-in viewer for images, Markdown and PDFs (drag to pan, scroll to zoom, resize,
  ◀ ▶ to browse). A URL chip dims when its port stops answering.
- **Approvals from the globe.** Add this hook and tool permission prompts show up as a card with
  Allow / Deny (Y / N) on the second screen. "Ask in VS Code" or a 15-minute timeout falls back to
  the normal prompt. Scope the matcher to the tools you want routed here.

  ```json
  "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "timeout": 900,
    "command": "curl -s -m 900 -X POST http://127.0.0.1:4711/approvals -H 'content-type: application/json' --data-binary @- || true" }] }]
  ```
- **Alerts.** The radio is the alert. Windows notifications are an opt-in in settings; when on, they
  fire only while the window is unfocused and appear under PowerShell's identity (unsigned-app limit). The tray icon colour is the one-glance status; double-click it
  to bring the window back; its menu mutes the radio or quits.
- **Cost.** Session spend in the HUD from token counts and list prices; per-unit cost in the
  panel; a budget alarm in settings.
- **Git.** Branch and uncommitted-change count of the session's folder in the HUD.

## Radio, narrator, alarms, debrief

- **Radio.** Voice lines on prompt received, unit deployed, mission accomplished, taking fire,
  construction complete, stall and loop. Mute and volume in ⚙ settings. Replace any line with your
  own clip at `<config dir>/agent-globe/sounds/<event>.wav` (events: prompt, spawn, done, error,
  complete, stall, loop, ship, blip, alarm, fanfare).
- **Progress rings.** Under every unit: percentage from the agent's own task list, a spinner while
  it works, and a one-line hint of what it is doing, or "idle".
- **Progress rings.** Under every unit: percentage from the agent's own task list, a spinner while
  it works, and a one-line hint of what it is doing, or "idle".
- **Narrator.** Under every tool call: the sentence the agent wrote right before it. Turn it off in
  settings.
- **Alarms.** Same command three times, a call open for two minutes, or context near the model's
  limit: toast, sound, and the unit turns amber.
- **Debrief (🎖).** Where the session's time went, what failed, and coaching lines based on the
  evidence: reads before the first edit, retried commands, context peaks, missing delegation.
- **Glossary.** Hover any counter for what it means.

## Layout

```
src/data/     pure Rust, no Bevy: model + reducer, Claude parser, hooks mapper, orbital layout
src/bridge.rs worker threads: session scanner, file tailer, HTTP ingest
src/app/      Bevy: camera (HDR, bloom, fly-to), scene (core, rings, beams, comets, stars),
              egui (HUD, sessions, agent panel, hover card, toasts, settings), state, theme
tests/        integration tests over fixtures/ (a trimmed real-shaped session)
```

Rules: `data/` never imports Bevy. `app/` never parses files. The tailer reads only appended
bytes and never writes under `~/.claude`.

## Config

No environment variables beyond the optional `CLAUDE_CONFIG_DIR`. Everything else is a flag or a
setting in the ⚙ window.

## Development

```
node ../../bin/factory.mjs check agent-globe     # fmt, clippy, check, test, release build, audit
cargo run                                       # dev build
```

The audit step carries one documented exception in `.cargo/audit.toml`.

## Backlog

See `BACKLOG.md`.

## License

MIT
