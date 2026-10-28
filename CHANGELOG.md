# Changelog

## Unreleased

## 0.6.0 - 2026-10-28
- Per-prompt grouping: the unit panel folds tool calls under each of your prompts, with calls, units spawned, wall time and cost per turn; the newest turn is open, older ones collapsed, and the history search keeps only turns with hits.
- Billing mode: "Pro / Max subscription" (default) marks every dollar figure with ≈ as API-equivalent and adds a five-hour window total across watched sessions; "API" shows list-price spend as before.
- Compact layout: on by itself under 1100 px of width or by setting. The sidebar collapses to a strip of session dots with an attention count, the HUD keeps only the key counters, and the unit panel takes a larger share of the window.

## 0.5.1 - 2026-10-28
- History search at the bottom of the unit panel: filters the tool calls by tool, summary, command, narrator note and output, case-insensitive, with a hit count; the list stops following the newest call while a search is active.

## 0.5.0 - 2026-10-28
- Control room: every session modified in the last 30 minutes is tailed at once (up to six), not just the open one.
- "Needs you" queue at the top of the sidebar: approvals, failed calls, commanders waiting for orders and stalls across all watched sessions; click a row to jump there.
- Other active sessions appear as satellite constellations around the current one; click a unit or its label to switch. Toggle in settings.
- Window jump: the 🖥 button next to a session, in the queue and in the HUD brings that session's VS Code window to the front (title match).
- Changes section in the commander panel: files changed against the last commit with +/- counts, how many edits this session made to each, an in-app diff viewer and an "open in VS Code" button.
- Artifact shelf folds repeats into one chip with a count (forty "check passed" become ×40).
- Fly-to frames the commander from further away so it no longer fills the view.
- Clicking a unit in cinematic mode brings the console back.
- Stalled calls get a ⏳ badge and appear in the queue; toasts and radio lines for other sessions only when they need you.

## 0.4.2 - 2026-10-28
- The red "failed call" badge and red unit colour now mean the latest finished call failed; a failure buried in history no longer marks the commander as failed every time a session opens.

## 0.4.1 - 2026-10-28
- Progress hint treats any open tool call as work in progress, so long builds no longer read "idle".

## 0.4.0 - 2026-10-28
- Progress rings under every unit: the task-list percentage when the agent keeps one (TodoWrite),
  a spinner while working, plus a hint of the current task or call, or "idle". Toggle in settings.
- App icon: embedded in the executable, shown in the title bar and the tray (status dot on top).
- Progress rings under every unit: the task-list percentage when the agent keeps one (TodoWrite),
  a spinner while working, plus a hint of the current task or call, or "idle". Toggle in settings.
- Viewer: movable and resizable window; drag to pan and scroll to zoom images; Markdown files are
  rendered; PDFs show extracted text by page; ◀ ▶ or arrow keys browse the unit's images, docs and
  PDFs together.
- Artifact shelf: search box when expanded.
- Desktop notifications are off by default (the radio is the alert) and never fire for badges
  that were already there at launch. Windows shows them under PowerShell's identity; that is a
  platform limitation for unsigned apps.

## 0.3.3 - 2026-10-28
- Viewer header shows the file name only; bubbles and badges hide while the viewer is open.

## 0.3.2 - 2026-10-28
- Artifact shelf: two rows by default, expand / collapse, scrollable; smaller thumbnails in the shelf.
- Integrated image viewer: click an image artifact to open it inside Agent Globe. Scroll to zoom,
  arrow keys or ◀ ▶ to browse the unit's images, Esc or ✕ to close, "open externally" if needed.

## 0.3.1 - 2026-10-28
- Artifact extraction rejects template URLs, shell fragments and sentence words; the chip strip
  shows the newest 18 with a "more" toggle.

## 0.3.0 - 2026-10-28 — Control tower
- Artifacts: served URLs, files written, images looked at, commits, pull requests and test results
  are extracted from every tool call. Chips in the unit panel; a speech bubble above the unit for
  90 s after a new one. Click opens the browser, VS Code at the file, the image, or the commit on
  GitHub. URL chips dim when the port stops answering.
- Approvals from the globe: a PreToolUse hook posts to `/approvals` and blocks until you click
  Allow, Deny, or "Ask in VS Code" (Y / N keys). Badge and radio line on the unit while it waits.
- Windows notifications when the window is unfocused and a unit needs you; tray icon whose
  colour is the status (green clear, amber waiting, red fire) with Show / Mute / Quit.
- Cost per session in the HUD from token counts and list prices, per-unit cost in the panel,
  budget alarm in settings.
- Git awareness: branch and uncommitted-change count of the session's folder in the HUD.

## 0.2.4 - 2026-10-28
- Badges stay inside the scene area; they no longer draw over open panels.

## 0.2.3 - 2026-10-28
- Badges sit directly above their units (projection offset fix).

## 0.2.2 - 2026-10-28
- Notification badges above units: glowing "!" lamp when a unit finishes, chat bubble over the
  commander while it waits for your orders, red warning when a call failed. Click the unit to dismiss.
- New radio line on turn end: "Waiting for commands, sir." A successful `factory ship` plays
  "Construction complete."

## 0.2.1 - 2026-10-28
- Radio plays through its own audio sink with an output-device picker in settings. Bevy's audio
  only ever used the Windows default device, which on multi-device machines can be a monitor.
- User prompts wrapped in harness tags (system reminders, IDE selection) are no longer dropped, so
  the "consider it done" cue fires on every prompt.

## 0.2.0 - 2026-10-28 — Army Men edition
- Green plastic soldiers: capsule units with helmets on oval bases, a commander with twin halos,
  sand orbit rings, tracer rounds along busy beams, warm key light. Stencil headings (Black Ops One).
- Radio: synthesized commander voice lines and cues for prompt received, unit deployed, mission
  accomplished, taking fire, construction complete, stall, loop, ship. Mute, volume, test buttons,
  optional blip per tool call. Drop your own clips in `<config>/agent-globe/sounds/<event>.wav`.
- Narrator: the sentence the agent wrote right before each tool call, under the call.
- Alarms that teach: loop detection (same command 3x), stall detection (call open > 2 min, unit
  turns amber), context bloat (per-model limit; HUD gauge turns amber then red).
- Debrief window: duration, turns, calls, failures, subagents, tokens, context peak, reads before
  first edit, retried commands, longest call, time share per category, and coaching lines.
- Glossary tooltips on every HUD counter and panel field. Cinematic mode (F) hides the console.
- Turn-end detection from transcripts; context size parsed from usage.

- Retro phosphor theme: green CRT palette, VT323 terminal font (OFL, in assets/fonts), voxel
  cubes for nodes, core, stars and dust, tumbling nodes, scanline overlay (toggle in settings),
  slight chromatic aberration. Still fully 3D at 230+ fps on the RTX 3090.

## 0.1.0 - 2026-10-28
- First release. Discovers Claude Code sessions, builds the agent tree from transcripts and
  subagent files, and renders it as an orbital constellation on the GPU (Bevy/wgpu, HDR bloom):
  glowing core, orbit rings per depth, beams with comets while tool calls run, starfield.
- Camera: orbit, zoom, pan with user-tunable sensitivity; click-to-fly; Esc/✕ to return; R or
  ⌂ home to reset; idle cinematic orbit. Input over UI panels never moves the camera.
- Hover card at the top right; full agent panel on click with prompt, tool-call timeline, and
  terminal-style Bash output capped at 10 lines per block with expand toggles.
- HUD with live counters; toasts for spawned/finished agents and failed tools.
- Settings window (camera speeds, drift, bloom, UI scale) persisted to the OS config dir.
- Live tail of appended transcript bytes and new subagent files.
- Local HTTP ingest on 127.0.0.1:4711: `/events` (event schema) and `/hooks/claude` (hook payloads).
- Visual pass: MSAA 4x, distance fog, torus orbit rings, rotating halos on the core and on
  selected/hovered/busy nodes, drifting dust, 2 px beams. The 3D view renders only into the area
  the panels leave free (dedicated UI camera), so fly-to targets land in the visible centre.
- Sessions sidebar: collapsible per project, with the subagent tree under the open session.
- Each tool call shows its local wall-clock time (hh:mm:ss) and duration.
- 14 tests over parser, reducer, layout, hooks mapping and ingest parsing.
