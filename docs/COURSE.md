# Agent Globe: a quick course

A tour of this codebase for someone who has never written Rust. Every concept is shown with a
snippet taken from the project (trimmed for reading), so when you open the real file you will
recognise it. Read top to bottom once; later use the "Recipes" section when you want to change
something.

```
   you type a prompt in VS Code            Agent Globe on the other screen
   ┌──────────────────────────┐            ┌─────────────────────────────────┐
   │ Claude Code writes every │  tail the  │ constellation: commander in the │
   │ step to a .jsonl file    │ ────────▶  │ middle, subagents on rings,     │
   │ ~/.claude/projects/...   │   file     │ files on the outer ring, radio  │
   └──────────────────────────┘            └─────────────────────────────────┘
```

---

## 1. The 30-second mental model

Claude Code records everything it does into JSON-lines transcripts. Agent Globe reads those files
as they grow, turns each line into an **event**, folds events into a **session model** (who did
what, when), and two consumers draw that model every frame: the **3D scene** (Bevy) and the **2D
console** (egui). Sounds, badges and the attention queue are side effects of the same events.

```
 transcript lines ──▶ Event ──▶ Session::apply ──▶ Store ──▶ scene.rs (3D)
                                                        └──▶ ui.rs    (2D)
                                                        └──▶ sounds / badges / queue
```

If you remember one sentence: **everything is derived from `Store.sessions`**. Nothing in the UI
holds state of its own that is not recomputed from the store each frame.

---

## 2. The toolchain in five commands

Rust projects are built by **cargo**, which plays the role of npm plus a bundler plus a test runner.
`Cargo.toml` is `package.json`; `Cargo.lock` is `package-lock.json`; dependencies are called
**crates** and live on crates.io.

```
cargo build --release      # compile (the release exe is what we install)
cargo run --release        # compile and start the app
cargo test                 # run every #[test] function
cargo clippy -- -D warnings  # the linter; we treat every warning as an error
cargo fmt                  # the formatter; never argue with it
```

The factory wraps these: `node bin/factory.mjs check agent-globe` runs fmt, clippy, tests, the
release build and a dependency audit; `ship` tags a version and installs the exe.

Two things that surprise JavaScript people:

- A compile error is normal and expected. The compiler is the test you run most. Read the message
  from the top; it usually names the fix.
- There is no `undefined`, no `null`, no exceptions. The language replaces them with types
  (`Option`, `Result`), which section 3 explains.

---

## 3. Rust in ten minutes, using this project

### 3.1 Variables, functions, structs

```rust
// src/data/model.rs (trimmed)
pub struct ToolCall {
    pub id: String,
    pub tool: String,          // "Bash", "Read", "Edit"...
    pub command: Option<String>,   // only Bash calls have one
    pub started: Millis,       // a type alias for i64 (milliseconds)
    pub ended: Option<Millis>, // None while the call is still running
    pub is_error: bool,
}
```

- `let x = 5;` is immutable. `let mut x = 5;` can change. The compiler refuses a write to an
  immutable binding, which catches a whole family of bugs at compile time.
- `pub` means visible from other modules, like `export`.
- A `struct` is a typed object with fixed fields. Methods go in an `impl` block:

```rust
impl Agent {
    /// True while any call is still running.
    pub fn has_open_call(&self) -> bool {
        self.calls.iter().any(|c| c.ended.is_none())
    }
}
```

`&self` means "borrow me, read only". `&mut self` means "borrow me, I will change fields". A
method that takes `self` by value consumes the object.

### 3.2 Option and Result instead of null and throw

```rust
pub ended: Option<Millis>   // Some(1761700000000) or None
```

You cannot forget to check. The compiler forces a `match` or one of the helper methods:

```rust
let took = c.ended.map(|e| dur(e - c.started)).unwrap_or_else(|| "running".into());
```

Read it as: "if `ended` has a value, turn it into a duration string; otherwise the string
`running`". For things that can fail, functions return `Result<T, E>`:

```rust
// src/bridge.rs (trimmed)
pub fn parse_events(body: &str) -> Result<Vec<Event>, String> {
    if body.trim().is_empty() {
        return Err("empty body".into());
    }
    ...
    Ok(out)
}
```

The caller decides what to do with `Err`; nothing propagates invisibly. The `?` operator is the
shortcut: `let v = parse(x)?;` returns the error to the caller if there was one.

### 3.3 Enums with data, and `match`

This is the feature that makes the whole app readable. An `enum` is a type with several shapes,
each shape can carry different fields. The transcript becomes a stream of these:

```rust
// src/data/model.rs (trimmed)
pub enum Event {
    SessionStart { session_id: String, project: String, at: Millis },
    AgentSpawn   { session_id: String, agent_id: String, parent_id: Option<String>, name: String, ... },
    ToolStart    { session_id: String, agent_id: String, call_id: String, tool: String, command: Option<String>, at: Millis },
    ToolEnd      { session_id: String, agent_id: String, call_id: String, stdout: String, is_error: bool, at: Millis },
    Message      { session_id: String, agent_id: String, role: String, text: String, output_tokens: u64, ... },
    TurnEnd      { session_id: String, agent_id: String, at: Millis },
    Throttle     { session_id: String, agent_id: String, resets_at: Millis, kind: String, at: Millis },
}
```

And the reducer, the function that folds one event into the model, is one big `match`:

```rust
impl Session {
    pub fn apply(&mut self, ev: Event) {
        match ev {
            Event::ToolStart { agent_id, call_id, tool, command, at, .. } => {
                let a = self.agent_mut(&agent_id).expect("ensured");
                a.calls.push(ToolCall { id: call_id, tool, command, started: at, ended: None, ..Default::default() });
            }
            Event::ToolEnd { call_id, is_error, at, .. } => {
                if let Some(c) = self.find_call_mut(&call_id) {
                    c.ended = Some(at);
                    c.is_error = is_error;
                }
            }
            Event::TurnEnd { agent_id, .. } => { /* mark the commander as waiting for you */ }
            _ => {}
        }
    }
}
```

`match` must cover every shape, so adding a variant to `Event` makes the compiler point at every
place that needs a decision. That is how we added `Throttle` safely in one afternoon.

If you know Redux: `Event` is the action type, `Session::apply` is the reducer, `Store` is the
store. The difference is that the compiler checks the reducer is exhaustive.

### 3.4 Ownership and borrowing, the only hard part

Every value has exactly one owner. Passing a `String` to a function **moves** it; the caller cannot
use it afterwards. To let someone look without taking, you pass a reference: `&value` (read) or
`&mut value` (write). Rules the compiler enforces:

- any number of readers, or exactly one writer, never both at the same time;
- a reference cannot outlive the thing it points to.

What this feels like in practice:

```rust
let sid = ev.session_id().to_string();
let session = store.sessions.entry(sid.clone()).or_insert_with(|| Session::new(sid, ...));
//                                 ^^^^^^^^^^^ we clone because `sid` is also moved into the closure
```

When the compiler says "borrow of moved value", the fix is almost always `.clone()` (make a
copy) or restructuring so the read happens before the write. You will meet this error on day one;
it is not a sign you did something wrong, it is the compiler asking who owns what.

### 3.5 Closures and iterators

Rust iterators look like JavaScript array methods and are lazy until collected:

```rust
let hits: Vec<usize> = a.calls
    .iter()                                   // borrow each call
    .enumerate()                              // (index, call)
    .filter(|(_, c)| call_matches(c, &needle))
    .map(|(i, _)| i)
    .collect();                               // materialise into a Vec
```

`|x| expr` is an arrow function. `.iter()` borrows, `.into_iter()` moves, `.iter_mut()` lets you
change elements.

### 3.6 Traits and derive

A **trait** is an interface. `#[derive(...)]` asks the compiler to write the boring
implementations for you:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn { pub prompt: String, pub started: Millis, ... }
```

`Debug` gives `{:?}` printing, `Clone` gives `.clone()`, `PartialEq` gives `==`, and the serde pair
gives JSON in and out. Bevy adds `Component` and `Resource` as derives too (section 5).

### 3.7 Modules

A file is a module. `mod scene;` in `src/app/mod.rs` includes `src/app/scene.rs`. `use` brings
names into scope. `crate::` is the root of this package; `super::` is the parent module.

```
src/
├── main.rs            ──▶ parses CLI args, calls app::run()
├── lib.rs             ──▶ exposes data/ and bridge for tests
├── bridge.rs          ──▶ threads + HTTP ingest
├── data/              ──▶ pure logic, no Bevy: testable in isolation
│   ├── model.rs       Event, Session, Agent, ToolCall, Turn
│   ├── claude.rs      transcript discovery, tailing, parsing
│   ├── artifacts.rs   URLs / files / commits found in tool output
│   ├── files.rs       hot files for the file ring
│   └── layout.rs      ring geometry
└── app/               ──▶ Bevy + egui
    ├── mod.rs         plugin wiring, the system schedule
    ├── state.rs       Store (the single source of truth), reactions, alarms
    ├── scene.rs       3D entities
    ├── camera.rs      orbit camera, fly-to
    ├── ui.rs          every egui panel
    ├── sounds.rs      radio lines
    ├── hotkeys.rs, winjump.rs, usage.rs, gitinfo.rs, probe.rs, debrief.rs, settings.rs, theme.rs
```

The split matters: `data/` has no idea Bevy exists, so its tests run in milliseconds and most
features start there.

---

## 4. Architecture: threads, a channel, and one store

```
 ┌─────────────── worker threads (src/bridge.rs) ────────────────┐
 │ scanner: lists sessions every 3 s                              │
 │ tailer:  polls up to six .jsonl files, parses new lines        │
 │ http:    127.0.0.1:4711  /events  /hooks/claude  /approvals    │
 │ usage:   asks Anthropic for plan usage once a minute           │
 │ hotkeys: Win32 Ctrl+Alt+Y / N                                  │
 └──────────────┬────────────────────────────────────────────────┘
                │ crossbeam channel: ToApp::{Sessions, Events, Approval, ...}
                ▼
 ┌─────────── main thread, once per frame (Bevy schedule) ───────┐
 │ drain_bridge  ─▶ Store.sessions[*].apply(event) + reactions    │
 │ watch_stalls  ─▶ open calls older than 2 min                    │
 │ sync_nodes    ─▶ spawn / despawn / recolour 3D units           │
 │ animate_*     ─▶ orbit positions, halos, dust, file ring       │
 │ camera::drive ─▶ fly-to, home, Esc                             │
 │ ui::draw      ─▶ every egui panel, reading the Store           │
 └───────────────────────────────────────────────────────────────┘
```

Threads never touch the store. They send messages; `drain_bridge` is the only writer. That rule is
what keeps a 2,500-line UI file from turning into a race condition.

```rust
// src/bridge.rs (trimmed): the message types
pub enum ToApp { Sessions(Vec<SessionRef>), Events(Vec<Event>), Approval(ApprovalRequest), Status(String), Error(String) }
pub enum ToWorker { Open(SessionRef), Watch(Vec<SessionRef>), Decide { id: u64, decision: String }, Rescan }
```

A channel in Rust is typed: the compiler knows exactly which messages can cross it.

---

## 5. Bevy: entities, components, resources, systems

Bevy is a game engine built around an **ECS**. Three nouns and one verb:

- **Entity**: an id, nothing more.
- **Component**: a plain struct attached to an entity. `Transform` (position), `Mesh3d`,
  `NodeAgent { session, id }`, `FileNode { path }`.
- **Resource**: a singleton available to every system. `Store`, `Settings`, `SceneAssets`.
- **System**: a function that runs every frame. Its parameters declare what it reads and writes;
  Bevy builds the schedule from that.

```rust
// src/app/scene.rs (trimmed)
#[derive(Component)]
pub struct NodeAgent { pub session: String, pub id: String }

pub fn animate_nodes(
    time: Res<Time>,                 // read a resource
    store: Res<Store>,
    mut nodes: Query<(&NodeAgent, &Orbit, &mut Transform)>,   // iterate entities having all three
) {
    for (node, orbit, mut tf) in nodes.iter_mut() {
        let angle = orbit_angle(orbit.base_angle, orbit.depth, time.elapsed_secs());
        let target = Vec3::from(layout::orbit_point(orbit.depth, angle));
        tf.translation = tf.translation.lerp(target, 0.1);   // ease toward the orbit position
    }
}
```

A `Query<(&A, &B, &mut C)>` is a database query: "every entity that has A and B and C, give me
A and B to read and C to write". There is no scene graph to walk; you ask for shapes of data.

Spawning a unit:

```rust
commands.spawn((
    Mesh3d(assets.soldier_mesh.clone()),
    MeshMaterial3d(assets.materials[&MatKey::Idle].clone()),
    Transform::from_translation(Vec3::ZERO).with_scale(Vec3::splat(0.01)),
    NodeAgent { session: sid.clone(), id: n.agent_id.clone() },
    Orbit { depth: n.depth, base_angle: n.angle },
))
.observe(on_click)      // picking: a click on this mesh calls on_click
.observe(on_over)
.observe(on_out);
```

The tuple is the component list. Observers are callbacks for pointer events on that entity.

The schedule lives in `src/app/mod.rs`:

```rust
.add_systems(Update, (
    (state::drain_bridge, hotkeys::poll, state::watch_stalls, scene::sync_nodes, ...).chain(),
    (scene::draw_links, camera::drive, scene::project_badges, alerts::update, ...).chain(),
).chain())
```

`.chain()` means "in this order". Bevy tuples hold 20 systems, hence two halves.

### 5.1 Where the pixels come from

```
 Session model ──sync_nodes──▶ entities (mesh + material + NodeAgent)
                               │
          animate_nodes ──────▶ Transform (position/scale per frame)
          update_halos  ──────▶ halo visibility/material
          draw_links    ──────▶ gizmo lines (immediate, no entity)
          project_badges ─────▶ Store.badges / unit_screen (3D → 2D coordinates)
                                      │
                                      ▼
                           ui::draw paints badges, labels, bubbles at those 2D points
```

`project_badges` is the bridge between the worlds: it takes each unit's 3D position, asks the
camera for the screen point, and stores it so egui can draw a badge on top.

---

## 6. egui: immediate-mode UI

egui has no components, no state, no virtual DOM. Every frame you call functions that draw
widgets and return what happened:

```rust
// src/app/ui.rs (trimmed): the HUD
let hud = egui::Panel::top("hud").show(&mut root, |ui| {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("AGENT GLOBE").font(stencil(20.0)).color(ACCENT));
        tip(ui, format!("units {}", s.agents.len()), "Agents in this session...");
        if ui.button("⚙ settings").clicked() {
            toggle_settings = true;        // handled after the closure, on the Store
        }
    });
});
```

- `ui.button(...)` draws the button **and** returns whether it was clicked, in one call.
- Widgets are laid out as you call them: `horizontal`, `vertical`, `Panel::left`, `ScrollArea`.
- State you want to keep (open panels, search text) lives in the `Store`, not in the widget.

Pattern used everywhere in `ui.rs`: collect intentions into local flags inside the closures
(`toggle_settings`, `open`, `pick`), then apply them to the `Store` after the panel closes. This
avoids borrowing the store mutably while egui is reading it.

Text with highlighted search hits is built the same way, as a `LayoutJob` with sections:

```rust
let mut job = egui::text::LayoutJob::default();
for (a, b) in match_ranges(text, needle) {
    job.append(&text[last..a], 0.0, plain.clone());
    job.append(&text[a..b], 0.0, hit.clone());   // background: ACCENT
    last = b;
}
```

---

## 7. One line's journey, end to end

Claude Code appends this to the transcript:

```json
{"type":"assistant","timestamp":"2026-10-29T03:12:04.1Z","message":{"content":[
  {"type":"text","text":"Running the tests."},
  {"type":"tool_use","id":"toolu_01","name":"Bash","input":{"command":"cargo test"}}]}}
```

1. **Tailer** (`claude.rs`) sees new bytes, splits a complete line, parses JSON, and emits
   `Event::Message { role: "assistant", text: "Running the tests." }` and
   `Event::ToolStart { call_id: "toolu_01", tool: "Bash", command: Some("cargo test"), note: "Running the tests." }`.
2. The **channel** carries both to the main thread.
3. **drain_bridge** calls `react()` first (is this live? is it the current session?) which decides a
   toast and a radio line: `call_kind("Bash", "cargo test")` is `Test`, so the radio says
   "Testing the line." Then `session.apply(event)` pushes the `ToolCall`.
4. **sync_nodes** sees the agent is active again and swaps its material to the bright one.
5. **animate_nodes** bobs the unit while it has an open call.
6. **ui::draw** shows the call under the current turn, with the narrator line in italics, and the
   progress ring under the unit reads "Bash cargo test".
7. Later a `ToolEnd` arrives; `react()` sees the test-like command finished without error and the
   radio says "All clear, sir."

Every box in that chain is a plain function you can read in isolation.

---

## 8. Recipes

### 8.1 Add a radio line

1. `sounds.rs`: add a variant to `Sfx`, its file name in `name()`, its sentence in `line()`, when
   it plays in `when()`, the clip in `embedded()`, and a cooldown.
2. Generate the clip: say the sentence with the Windows voice, pass it through `radio_fx.py`,
   drop the WAV into `assets/sounds/`.
3. `state.rs`: trigger it in `react()` (current session) or in the other-sessions branch of
   `drain_bridge`.

```rust
Event::ToolStart { tool, .. } if tool == "WebSearch" => (None, Some(Sfx::Intel)),
```

### 8.2 Add a badge kind

1. `state.rs`: a variant in `BadgeKind`, its rank in `attention()`.
2. `scene.rs`: the condition in `project_badges` that decides a unit shows it.
3. `ui.rs`: glyph, colour and label in `badge_style()`.
4. `alerts.rs`: the Windows-toast text if it should notify.

### 8.3 Add a HUD counter

In `ui.rs`, inside the HUD closure, one `tip(ui, text, help)` call. Read the number from `store`
or `session`; never compute anything expensive there, the HUD runs every frame.

### 8.4 Add a field to the model

Add it to the struct with `#[serde(default)]` so older saved data still loads, set it in the
`Session::apply` arm that learns it, and write one test in `model.rs` that applies events and
asserts the field. The compiler will list every constructor you must update.

### 8.5 Verify a change

```
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
node bin/factory.mjs check agent-globe        # from D:\software-factory
powershell -File tools/screenshot.ps1 -Out shot.png   # passive capture of the running app
```

---

## 9. Reading the compiler

Three messages you will see most, and what they mean:

| Message | Meaning | Usual fix |
|---|---|---|
| `borrow of moved value` | you used something after giving it away | `.clone()` or reorder |
| `cannot borrow as mutable because it is also borrowed as immutable` | reading and writing the same thing in one expression | compute the read into a local first |
| `mismatched types: expected &str, found String` | a function wants a borrowed string | put `&` in front |

Clippy messages are style, but we treat them as errors so the code stays uniform. They always
include the suggested rewrite.

---

## 10. Glossary

- **crate**: a Rust package (library or binary). `bevy`, `egui`, `serde` are crates.
- **trait**: an interface. `derive` generates common ones.
- **Option / Result**: "maybe a value" and "value or error"; the language's replacement for null
  and exceptions.
- **borrow**: a temporary reference, `&x` to read, `&mut x` to write.
- **ECS**: entity, component, system; Bevy's way of organising a scene as data plus functions.
- **Resource**: a Bevy singleton. The `Store` is one.
- **system**: a function Bevy runs each frame; its parameters say what it needs.
- **immediate mode**: UI rebuilt every frame from state, no retained widget tree.
- **reducer**: `Session::apply`, the one place events change the model.
- **nits**: the unit Bevy uses for emissive light; 40–220 is bright here.

That is the whole shape. Next step: open `src/data/model.rs`, read `Session::apply` top to
bottom, then `src/app/state.rs` `react()`. Those two functions are where most behaviour lives.
