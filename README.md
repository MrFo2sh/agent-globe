# Agent Globe

A GPU-rendered, game-engine-powered view of what your coding agents are doing, live. Each session
is a constellation: the root agent is a glowing core, every subagent depth is an orbit ring, links
are light beams with comets travelling along them while a tool call runs. Nodes pulse while busy,
dim when done, turn red on errors. Click a node and the camera flies to it; its panel shows the
prompt, every tool call in order, and each Bash command with real stdout and stderr.

Rust, Bevy (wgpu, Vulkan/DX12/Metal), egui. HDR bloom, MSAA, fog. Retro phosphor-green theme
with voxel nodes, scanlines and the VT323 terminal font. No web view, no Electron.

Put VS Code with your agent on one screen and this on the other.

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

## Gamification

The HUD shows a level and XP bar computed from real work: tool calls, output tokens, and agents
spawned. Toasts announce spawned agents, finished agents, and failed tools as they happen.

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
