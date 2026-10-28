# Changelog

## Unreleased

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
