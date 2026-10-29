# Changelog

## Unreleased

## 0.9.2 - 2026-10-29
- Navigating to another constellation keeps the one you came from as a satellite: the last three visited sessions stay watched and shown even when idle, so you can always go back.

## 0.9.1 - 2026-10-29
- Plan usage in the HUD, left of the debrief button: session (5 hr), weekly and per-model limits as percent with a small bar, coloured green, amber from two thirds, red from ninety percent; hover for the reset time, click or 📊 for the full panel with bars and reset times, refreshed every minute. The app reads the Claude Code login on this machine and asks Anthropic's usage endpoint, like the editor extension does; the token stays in memory and is never logged.

## 0.9.0 - 2026-10-29
- Satellites: every other constellation wears two slowly turning circles that brighten on hover; clicking a circle, a unit or the label navigates there and the camera flies home to frame it. A unit of another constellation is no longer selected from afar: go there first, then click it.

## 0.8.4 - 2026-10-29
- History search: matching prompt groups are a lit band with a border and their hit count; the other groups stay listed but dim and folded, so the matching ones stand out. Hits inside the prompt line are boxed.

## 0.8.3 - 2026-10-29
- History search: a prompt group that holds hits is drawn in the accent colour with its hit count in the header, and hits inside the prompt line itself are boxed too.

## 0.8.2 - 2026-10-29
- F11 toggles borderless full screen on the current monitor; the status strip mentions it.

## 0.8.1 - 2026-10-29
- History search highlights every hit in the accent colour: in call headers, narrator lines, commands and outputs. Capped outputs show the lines around the hits (with ⋯ where lines are skipped) instead of the first ten lines while a search is active.

## 0.8.0 - 2026-10-28
- Radio, eleven new lines so a session can be followed by ear: "Scouting ahead" (first read or search of a turn), "Digging in" (first edit), "Gathering intel" (web), "Testing the line" (a build, test or check starts), "All clear, sir" / "We have casualties!" (it passed or failed, including failures the exit code missed), "Position fortified" (git commit), "Need your input, commander" (the turn ended with a question) next to "Waiting for commands" (it just ended), "Requesting authorization" (approval), "Running low on supplies" (context near the limit), "We are out of ammo, sir" (plan limit). Settings list every line and when it plays, with a test button each.
- Cost is gone everywhere: no dollar figures, no budget alarm, no billing mode, no fuel gauge. Token counts stay. The only limit indicator left is the red "⛔ limit · resets hh:mm" from Claude Code's own throttle records.
- Documents open in VS Code: markdown, PDF and other file artifacts, and the files in the Changes list, go straight to the editor like the file ring does. The in-app viewer now shows images only; the PDF text extractor dependency is dropped.

## 0.7.1 - 2026-10-28
- Compact status hint no longer mentions the removed F shortcut.

## 0.7.0 - 2026-10-28
- The question in the bubble: when a commander waits for you, its badge, the Needs-You row, the hover card and the satellite card show what it actually said or asked, not just "awaiting your orders".
- Fuel gauge for Pro / Max plans: Claude Code's rate-limit refusals in the transcripts (status, reset time) become Throttle events. The app learns what a five-hour window holds from the spend at the moment of each throttle, then shows "⛽ 62% · empty ~16:40" from the current window's spend and the pace of the last half hour. While throttled it shows the reset time in red. A throttle raises a toast and the alarm, for any watched session. Settings show the learned size with a "forget" button.
- Global approval hotkeys: Ctrl+Alt+Y allows and Ctrl+Alt+N denies the oldest pending request from any window, with a toast confirming what was decided.
- File constellation: an outer ring with the session's 24 hottest files (edits weigh three reads), sized by heat and glowing for 20 s after an edit, with beams from the units that touched them in the last 45 s (bright for edits, faint for reads, fading with age). Labels under the nodes; click one to open the file in VS Code. Toggle in settings.
- The F shortcut for cinematic mode is gone; the 🎬 button remains and Esc brings the console back.

## 0.6.1 - 2026-10-28
- HUD: the session-length counter is gone (it is in the unit panel), so the cost and five-hour figures no longer run under the buttons.

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
