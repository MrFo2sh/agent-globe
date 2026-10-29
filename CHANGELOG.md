# Changelog

## Unreleased

## 0.27.2 - 2026-10-29
- Quick open shows its selection: the highlighted row and a ▸ marker follow ↑ ↓. The rows are frameless buttons, which ignore their fill, so the selection never painted before.

## 0.27.1 - 2026-10-29
- Right-click a prompt group in the unit panel to open it as a card, the same as a double-click.

## 0.27.0 - 2026-10-29
- Quick open: ↑ ↓ move the selection (the keys are taken before the search box can move its caret on them), hovering only moves it when the pointer moves, and on a wall of monitors the palette opens on the monitor under the pointer instead of on the seam.
- Light settings: key light, fill light, ambient and fog, applied the frame they change. The settings page is in sections now: radio, camera, look, light, scene, window. Reset keeps your docked panes and the agent panel's width.

## 0.26.1 - 2026-10-29
- Links between files are off by default; the ⛓ toggle in the top bar turns them on (focus or all).

## 0.26.0 - 2026-10-29
- One click picks a file, in the constellation, the tree or the finder: it lights up, the camera glides towards it, and its connections show with their direction, green to the files it uses, red from the files that use it, amber for files that change with it. A second click opens it in the editor; double-click in the tree opens it pinned; Esc drops the selection.
- Git repositories nested in the session folder are indexed with their own ignore rules, so a factory's products are in the constellation even though the factory's gitignore hides them. Connections and co-changes come from every repository.
- Connections to the files the units touch resolve again: those nodes carry the path as the tool wrote it, which no longer has to match letter for letter.

## 0.25.3 - 2026-10-29
- Text fields take the keyboard back. Once the editor surface (a native child window) has had the keyboard, Windows kept handing it back, so the history and artifact searches and every other box looked disabled. A click on the app's own side, or a field gaining focus, now returns the keyboard to the app.

## 0.25.2 - 2026-10-29
- Cards drag again. The motion check added for slow header clicks looked at egui's one-frame drag-start flag, which fires before the pointer has moved far enough, so no drag ever began. The check now watches the drag as it goes.

## 0.25.1 - 2026-10-29
- A ⛓ toggle in the top bar for the file connections: off, the hovered or open file only, or all. Remembered across runs.

## 0.25.0 - 2026-10-29
- Connections between files, from three sources: imports read from the source (Rust mod/use, TypeScript and JavaScript import/require, Python import) resolved against the file list; temporal coupling from the last 500 commits (files that change together, commits touching more than 30 files skipped, pairs seen twice or more); and files the same unit touched in the same stretch. Hover a file node, or open a file, and its connections light up: cyan for imports, amber for co-changes, with the connected files named. A settings switch draws every connection faint, always.
- File names: cold files show their names when the camera is within reach, when they are connected to the hovered or open file, and always on small projects.

## 0.24.2 - 2026-10-29
- Folder captions read `src/  16`: the stencil font has no arrow glyph, so the arrow showed as a question mark.

## 0.24.1 - 2026-10-29
- The file system reads as one: cold files are cool-tinted and a touch bigger, apart from the warm dust; each cold ring has a faint guide circle and each folder wedge a faint edge line; every folder carries a caption on its wedge (name and file count) that opens it in the files sidebar. All cold rings drift together so the wedges never shear. The home camera takes the outermost ring in.
- The agent panel opens at the width you last dragged it to, this run and the next.
- AGENT_GLOBE_SHOT=<png> photographs the window itself after a delay, a development aid.

## 0.24.0 - 2026-10-29
- The constellation carries the whole file system: beyond the ring of hot files, every file of the session folder (up to 1,500, gitignore respected) sits on the outer rings, alphabetical so folders stay together, small and dim, lit and labelled when hovered. Click a file node to open it in the editor card.
- A view toggle in the HUD: both, files only (the units, halos, rings and links step aside), or units only (no file rings).
- Folders are wedges of the outer rings: each top-level folder gets a slice of the circle sized by its file count, its files on the first cold ring and nested files one ring further out per level, a fan behind its name.
- A unit touching a file: the beam and the file's glow are cyan-green now, and read beams are brighter.
- Opening a file, from the tree, the finder, the palette or a node, glides the camera to its node.
- A press that has not moved never starts a card drag: a slow click on a header button used to undock the card and drop it as a small floating one.

## 0.23.1 - 2026-10-29
- Dock columns follow the monitors: each monitor's free span (beside the sidebars and the agent panel) is split in two, so the seam between monitors is always a grid line and a pane docked on the left monitor never spills onto the right one. Pane positions are fractions of the whole window, clipped to the free area, so a sidebar opening pushes a pane and closing it gives the space back.
- Dock targets for four-wide shapes: a strip across both monitors and the whole wall (labelled 2), beside the quarters, halves and monitors.

## 0.23.0 - 2026-10-29
- Scoped zoom: Ctrl with + / - / 0 acts on the card under the pointer (the editor's font, the terminal's font, the text of the reply and turn cards). Only with no card under the pointer, and no text field typing, does it scale the whole UI. Inside the editor the page handles the keys itself.
- Ctrl+Shift+V flips Markdown between edit and preview, from the app or from inside the editor.
- Ctrl+P opens a quick-open palette at the top: type, ↑ ↓, Enter opens, Esc closes; an empty box lists the open files. It works from inside the editor too, which hands the keyboard back to the app.
- Preview tabs: a single click opens a file in a preview tab that the next single click reuses; double-click, or any edit, keeps the tab. Ctrl+W closes the active tab.

## 0.22.1 - 2026-10-29
- The dock area is exactly the free space: right of both left sidebars, left of the agent panel, under the top bar, above the status bar. Docked panes are fractions of it, so they give way when the agent panel or a sidebar opens and take the space back when it closes.

## 0.22.0 - 2026-10-29
- The files tree is a second left sidebar beside the sessions, not a card: a strip with 🗂 when collapsed, a resizable panel with the finder box on top when expanded. Expanding it pushes the dock area and every docked card to the right, like the sessions sidebar. Ctrl+P expands it and focuses the finder; ⟨ collapses it.

## 0.21.2 - 2026-10-29
- Docked panes share their edges like editor groups: drag the edge between two panes and both follow, so shrinking the files pane hands the space to the editor beside it. Every pane on that line moves, and none drops below a sliver.

## 0.21.1 - 2026-10-29
- The editor surface was never created: the host looked the window up through a registry this Bevy build does not expose. It now takes the window's raw handle from the window entity. The first open downloads Monaco, which can take a while; the page reports boot, libraries and errors into the log.
- Docked cards resize freely: drag any edge or the bottom-right corner; it snaps when within a few pixels of a grid line and lands wherever you leave it otherwise. Dock targets still give quarters, halves and whole monitors as starting shapes. Pane positions are saved as fractions of the dock area.
- AGENT_GLOBE_OPEN=<file> opens a file in the editor at start, a development aid.

## 0.21.0 - 2026-10-29
- Files card (🗂 in the HUD, or Ctrl+P anywhere): the session folder as a lazy tree, hidden and build folders skipped, and the card's search box as a fuzzy finder over an index built in the background with the gitignore respected. Click opens a file in the editor; right-click reveals it in VS Code; Enter opens the first match.
- Editor card: a WebView2 surface inside the card chrome, so it floats, docks and fills the screen like every card. Monaco for code with the phosphor theme and Ctrl+S saving through the app; Markdown rendered with Mermaid diagrams and GitHub styling, with an edit/preview switch; Word documents converted in the page; images; PDFs in Edge's own viewer. Tabs for open files with unsaved marks. Monaco and the Markdown tools load from the internet.

## 0.20.1 - 2026-10-29
- Dock targets for halves and whole monitors, not only quarters: ¼ at each region's centre, ½ between two regions, 1 at a monitor's centre. The block lights up before you drop.
- The dock area excludes the left sidebar and the top bar, like an editor's workbench: docked cards never cover them.

## 0.20.0 - 2026-10-29
- Docking. The window is a grid of regions (two columns per monitor by default, set in settings). Drag any card by its title: targets appear in every region; drop on one and the card fills that region. Drag a docked card's edge to grow or shrink it by whole regions; drag its title to float it again. Dropping anywhere else leaves the card floating. Where each card is docked is remembered across runs. Works for the reply, terminal, turn and settings cards, and every card to come.
- The window opens borderless across every monitor at start (one wall), instead of full screen on one. A settings switch restores the single-monitor full screen; F11 still toggles.

## 0.19.1 - 2026-10-29
- Terminal Ctrl+C works. Windows children inherit an "ignore Ctrl+C" flag from any parent started as its own process group (shells and cargo do that), so nothing in the console could be interrupted. The flag is cleared before each console is created; a test interrupts a running ping to prove it.
- The last terminal tab has a close button too: closing it closes the card. Tabs no longer show PowerShell's exe path as their title.

## 0.19.0 - 2026-10-29
- The raid: a wave shooter in cinematic mode. Saucers spawn at the edge of the scene and fly at the units; click one to fire the turret under the camera; a saucer that lands drains data integrity, at zero the raid is over. Waves grow (3 + 2n ships), every third wave is a new level with tougher, faster ships. Score, kills and a best score. Controls in the bottom-left console and with P: start, pause, resume, replay, stop (stop clears the sky). Pause writes the raid to disk, ships included, so it survives closing the app; leaving cinematic mode pauses it.

## 0.18.1 - 2026-10-29
- Terminal scrolling: the console's startup clear used to push a blank page into the history, so the wheel scrolled the prompt away into nothing. Blank-only history is dropped, a thin scrollbar shows where the screen sits, and a smoke test proves the console answers on this machine.

## 0.18.0 - 2026-10-29
- A real terminal: each tab is a Windows pseudo console (ConPTY) running PowerShell, with alacritty's VT state machine behind a cell grid, 10,000 lines of scrollback, 256 and true colour, bold, underline, inverse, the program's own cursor shape, bracketed paste and app-cursor keys. Click the grid to type; PSReadLine, tab completion, Ctrl+C, `claude`, vim and other full-screen programs work as in a native window. The wheel scrolls the history, the card's search box switches to a text view of the scrollback with the hits lit, ⧉ copies it all, 🗑 runs cls, ⟲ restarts the tab. Replaces the line-based console.
- Cinematic mode is for watching: clicks on units, files and other constellations do nothing there. Esc brings the console back.
- ARTIFACTS and CHANGES are collapsing sections like SERVERS & PROCESSES: counts in the header, fully hidden when closed.

## 0.17.0 - 2026-10-29
- Double-click a prompt group to open it as a card: a conversation segment (your prompt and the commander's reply) and a details segment (that turn's tool calls with the narrator lines).
- Every card has a search box (Ctrl+F): the reply and turn cards filter to matching lines with the hits lit, the terminal filters its scrollback, the turn details reuse the call-row highlighting.
- Card title rows drag the card even over their text.
- Terminal tabs: + or Ctrl+T opens a tab in the current folder, ✕, middle-click or Ctrl+W closes one; each tab is its own shell. Tab completion through PowerShell's own engine (Tab cycles, Shift+Tab back), a PS-style prompt, Ctrl+C ends the running command, cls clears.
- CHANGES: the expand button only appears when more than three files changed, and says how many are hidden.
- Fold all works: the collapse was written to ids inside a different ui scope than the headers. Groups now have absolute ids driven directly; fold sticks, and an unfold button opens every group again. Group headers toggle on click of the arrow or the title.

## 0.16.0 - 2026-10-29
- Integrated terminal: the HUD's ⌨ opens a persistent PowerShell in the session's folder, inside the app. Enter runs a line, ↑ ↓ walk the history, cd and variables stick between commands, errors show inline, ⟲ restarts the shell, 🗑 clears, ⧉ copies. Right-click ⌨ still opens a native window. One shell per session; closed shells end with the app.
- One card chrome for the reply, the terminal and settings: drag to move, drag the corner to resize, ⛶ to fill the screen, Esc to close. Cards sit above the badges and file labels; badges under a card are not drawn.
- No native title bar: the app is all chrome. A "native title bar" switch in settings brings Windows' bar back in windowed mode.
- "Yes, commander!" when you click the commander.
- Scene shortcuts (R, Esc) stay quiet while you type in a text field.
- Removed the debrief window and its button.

## 0.15.0 - 2026-10-29
- Ctrl and + / - scale the whole UI in steps of ten percent, Ctrl 0 resets; the same setting as the ui scale slider, remembered across runs.
- The reply card can fill the screen (the full button, restore to go back) or be resized by dragging its bottom-right corner; the size is remembered.

## 0.14.0 - 2026-10-29
- Every prompt group now carries the commander's reply: a 💬 row with the first line, expanding to the full answer with light Markdown. Conversation turns with no tool calls used to render as an empty '0 calls' group; now they show what was said. The history search also matches reply text.
- Slash-command records (/model, /clear and friends) no longer appear as prompts.

## 0.13.1 - 2026-10-29
- '▴ fold all' now actually collapses every prompt group and the folding sticks; each group can be reopened individually afterwards. Before, the collapse lasted a single frame and sprang back open.

## 0.13.0 - 2026-10-29
- Docker servers get real controls: each one is matched to its container (docker ps), shown with its name and running/stopped state, with ⏸ stop when up, ▶ start when down, and ⛔ kill after a confirming second click. No more dead-end 'stop it from Docker' label for the user's own containers.
- The whole SERVERS & PROCESSES section is collapsible, with a count of servers and processes in its header.

## 0.12.0 - 2026-10-29
- ⌨ button next to the session name in the HUD opens a terminal in the session's root folder: Windows Terminal if installed, otherwise PowerShell.

## 0.11.2 - 2026-10-29
- The reply card is drawn on the top layer: badges and file labels no longer show through or over it. It is draggable, stays on screen, and scrolls.

## 0.11.1 - 2026-10-29
- The reply card has a solid opaque background with a drop shadow, so the constellation and file labels behind it no longer bleed through the text.

## 0.11.0 - 2026-10-29
- Servers list shows only what is listening now; Windows services and Agent Globe's own ports are hidden as noise, Docker-owned ports are labelled and left for Docker to stop. Background processes collapse into one expandable line.
- Click the 💬 badge to read the reply: a card next to the unit shows the commander's full last message, the text the editor shows as its answer, with light Markdown (headings, lists, code, bold), a copy button, and Esc, ✕ or the badge again to close. Works on satellites too. Replies are kept up to 12,000 characters.

## 0.10.1 - 2026-10-29
- Servers owned by Docker, Windows services or Agent Globe itself are listed with their owner but get no ✕: ending those would take Docker Desktop or the app down with the port.
- Usage readout keeps the last good numbers when a refresh fails, polls every 90 s, and waits five minutes after a 429 from the usage endpoint.

## 0.10.0 - 2026-10-29
- Servers & processes: the commander's panel lists the servers this session runs (from the local URLs it printed and the server-like or background commands it ran) with a live dot, the index page's title, the URL (click to open), the owning process and pid, plus the background processes it spawned with their command line. ✕ ends a process tree after a second confirming click. The HUD shows "⚡ n" while servers are listening; click it to open the panel. The app asks Windows every five seconds (netstat, the process list) and reads each index page once a minute.
- Tool calls remember whether they were started in the background.

## 0.9.5 - 2026-10-29
- Turns settle on their own: this Claude Code version writes no turn-end record, so the app now closes a commander's turn when its last reply was text only and nothing followed for five seconds. "Waiting for commands" (or "Need your input, commander" after a question) plays a few seconds after the answer, the waiting badge appears, and "thinking…" ends; a late real turn-end record is ignored.
- "▴ fold all" next to TOOL CALLS collapses every prompt group.
- The app starts full screen on its monitor; F11 switches to a window and back.

## 0.9.4 - 2026-10-29
- A commander whose turn is open with nothing answered yet reads "thinking…" with a spinner, under the unit and on the satellite label, instead of "idle". Claude Code writes nothing to the transcript while a long reply is being generated, so that state used to look like inactivity.

## 0.9.3 - 2026-10-29
- Clicking anywhere on a satellite constellation navigates to it: an invisible sphere wraps each satellite as the click target, with a faint glow while hovered; the two circles stay as the visual cue.

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
