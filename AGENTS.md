# Agent Globe — notes for any coding agent

Rust + Bevy 0.20 (wgpu) + bevy_egui 0.43 (egui 0.36) desktop app that renders coding-agent
sessions as constellations. Product of the software factory at `D:\software-factory`; this
folder is `products/agent-globe` there and its own git repo (tags `vX.Y.Z`).

Read this, then `SPEC.md` (what it is) and `CHANGELOG.md` (what each version added). `docs/COURSE.md`
is the owner's introduction to the codebase; keep it true when the architecture changes. The owner
is on a Claude Max plan, works in VS Code on one screen with this app on the other, and tests the
running app themselves while you work: odd clicks in screenshots may be theirs.

## Map

| File | Owns |
|---|---|
| `src/data/claude.rs` | Reads Claude Code transcripts (`~/.claude/projects/<proj>/<session>.jsonl`, `<session>/subagents/agent-*.jsonl` + `.meta.json`). `discover`, `Tailer::poll`, `parse_record` → `Event`s. Also `throttle_info` (rate-limit records), `todo_progress`, `narrator_line`, `summarize_tool`. |
| `src/data/model.rs` | `Event` enum, `Session::apply` reducer, `Agent` (calls, tokens, artifacts, `turn_log`, `last_said`), `ToolCall`, `Turn`, `Throttle`, `snippet`. |
| `src/data/artifacts.rs` | URLs, files, images, commits, test results pulled out of tool output. |
| `src/data/files.rs` | Hot files of a session (`hot_files`) for the file ring. |
| `src/data/layout.rs` | Orbital layout: `ring_radius`, `orbit_point`, `layout`, `curve`. |
| `src/data/hooks.rs` | Events from Claude Code hooks posted to `/hooks/claude`. |
| `src/bridge.rs` | Worker threads: scanner, multi-session tailer (`ToWorker::Open/Watch`), HTTP ingest on 127.0.0.1:4711 (`/events`, `/hooks/claude`, `/approvals` blocking hook, `/health`). |
| `src/app/state.rs` | `Store` resource: sessions, watched set, attention queue, badges, approvals, viewer, queries; `drain_bridge`, `watch_stalls`, pattern alarms, radio triggers (`call_kind`). No cost anywhere: the owner asked for it to go. |
| `src/app/scene.rs` | 3D: units keyed by (session, agent), rings, satellites, halos, dust, file ring, beams, badge projection, click/hover observers. |
| `src/app/camera.rs` | HDR orbit camera, viewport between panels, fly-to, Esc handling. |
| `src/app/ui.rs` | All egui: HUD, sidebar (Needs-You queue, sessions tree, compact strip), unit panel (artifact shelf, changes, turns, history search), hover cards, settings, terminal, badges, bubbles, approvals, image viewer (images only; other files open in VS Code), file labels. ~2,500 lines; search for `fn draw_` and `fn agent_panel`. |
| `src/app/sounds.rs` | Radio voice lines (embedded WAVs), own rodio sink with device picker. |
| `src/app/alerts.rs` | Tray icon moods, optional Windows toasts. |
| `src/app/hotkeys.rs` | Global Ctrl+Alt+Y / Ctrl+Alt+N approval hotkeys (Win32 `RegisterHotKey` on its own thread). |
| `src/app/winjump.rs` | Bring a session's VS Code window to front by title (skips our own process: asking our window for its title deadlocks). |
| `src/app/raid.rs` | The cinematic-mode wave shooter: ships, turret beams, debris, waves and levels, pause persisted to `raid.json`. |
| `src/app/gitinfo.rs`, `probe.rs`, `terminal.rs`, `settings.rs`, `theme.rs` | git branch/dirty/changes, URL liveness, ConPTY consoles per session (alacritty_terminal behind an egui cell grid), persisted settings (`%APPDATA%/agent-globe/settings.json`), palette and tuning constants. |

Data flow: files/hooks → `bridge` threads → `ToApp` channel → `drain_bridge` applies events to
`Store.sessions` → `scene` syncs entities from the store each frame → `ui::draw` reads the store.
The app tails up to six recently modified sessions at once; `Store.current` is the one on screen.

## Rules that cost us time before

- Bevy system tuples hold 20 systems at most: `mod.rs` chains two halves.
- egui 0.36: `Panel::top/left/...` inside a root `Ui`, `Button::image`, `style_mut_of`; a font
  family installed this frame cannot be used this frame (`style_once`).
- Emissive strengths are in nits: 40–220 looks right, 1–5 is invisible.
- `world_to_viewport` returns window logical coordinates; do not add panel insets.
- Patch scripts must match the file as rustfmt leaves it; run `cargo fmt` first, then anchor.
- Never drive the owner's mouse or keyboard (no synthetic clicks, wheel, SendKeys). Verify with
  `tools/screenshot.ps1` (passive capture, DPI-aware) or unit tests, or ask the owner to click.
- The screenshot of a maximized window is 3868×2188 on this machine; the helper scales it down.
- Settings are saved on exit by the running app: edit `settings.json` only while it is closed.

## Loop

1. Change one thing. `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`.
2. Bump `version` in `Cargo.toml` and `factory.json`, add the entry under `## Unreleased` in
   `CHANGELOG.md`.
3. From `D:\software-factory`: `node bin/factory.mjs check agent-globe` then
   `node bin/factory.mjs ship agent-globe`. Ship commits and tags here and copies the release exe to
   `%LOCALAPPDATA%\Programs\agent-globe` (close the running app first). Then commit the factory's
   ledger. Pushing is the owner's job.
4. Relaunch `%LOCALAPPDATA%\Programs\agent-globe\agent-globe.exe` and take a passive screenshot.

Hard rules: defensive only, no secrets in the tree, stay inside this folder.
