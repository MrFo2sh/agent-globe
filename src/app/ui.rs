//! egui layer: HUD (top), sessions (left), selected agent (right), status (bottom), toasts,
//! hover card, settings and terminal cards. Reports whether the pointer is over any of these so
//! the camera can ignore input that belongs to the UI.

use super::editor;
use super::files;
use super::gitinfo::{FileChange, GitInfo, GitWatch};
use super::probe::Probe;
use super::raid;
use super::servers::{self, ServerWatch, Snapshot};
use super::settings::Pane;
use super::settings::Settings;
use super::sounds::{Sfx, Sounds};
use super::state::CardState;
use super::state::{BadgeKind, BridgeRes, EditorCmd, SceneView, Store, ToastKind, Viewer, clock_hm};
use super::terminal::{self, Terminals};
use super::theme;
use super::usage::{self, UsageState, UsageWatch};
use agent_globe::bridge::Bridge;
use agent_globe::data::artifacts::{Artifact, ArtifactKind};
use agent_globe::data::claude::SessionRef;
use agent_globe::data::model::{Agent, ROOT_AGENT, Session, Status, now_millis, snippet};
use agent_globe::data::servers::session_targets;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use bevy_panorbit_camera::PanOrbitCamera;
use std::collections::{HashMap, HashSet};

// Army palette.
pub(super) const ACCENT: egui::Color32 = egui::Color32::from_rgb(160, 232, 110); // light plastic
pub(super) const DIM: egui::Color32 = egui::Color32::from_rgb(120, 140, 95);
const GREEN: egui::Color32 = egui::Color32::from_rgb(190, 240, 140);
pub(super) const RED: egui::Color32 = egui::Color32::from_rgb(255, 100, 80);
pub(super) const AMBER: egui::Color32 = egui::Color32::from_rgb(255, 185, 70);
pub(super) const SAND: egui::Color32 = egui::Color32::from_rgb(222, 200, 135);
pub(super) const TEXT: egui::Color32 = egui::Color32::from_rgb(222, 226, 200);
const PANEL: egui::Color32 = egui::Color32::from_rgba_premultiplied(8, 14, 5, 228);
pub(super) const KHAKI_DIM: egui::Color32 = egui::Color32::from_rgb(150, 138, 96);
pub(super) const INK: egui::Color32 = egui::Color32::from_rgb(4, 8, 3);
const SCANLINE: egui::Color32 = egui::Color32::from_rgba_premultiplied(0, 0, 0, 60);
const MAX_LINES: usize = 10;

pub(super) fn stencil(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("stencil".into()))
}

/// Returns true on the frame it installed fonts; egui applies them next frame, so skip drawing then.
fn style_once(ctx: &egui::Context, done: &mut bool) -> bool {
    if *done {
        return false;
    }
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "vt323".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/VT323-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "stencil".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/BlackOpsOne-Regular.ttf"
        ))),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "vt323".to_owned());
    fonts.families.insert(
        egui::FontFamily::Name("stencil".into()),
        vec!["stencil".to_owned(), "vt323".to_owned()],
    );
    ctx.set_fonts(fonts);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        for font in style.text_styles.values_mut() {
            if font.family == egui::FontFamily::Proportional {
                font.size *= 1.25;
            }
        }
    });

    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.override_text_color = Some(TEXT);
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(45, 70, 30));
    v.widgets.inactive.bg_fill = egui::Color32::from_rgb(20, 34, 14);
    v.widgets.hovered.bg_fill = egui::Color32::from_rgb(36, 60, 24);
    v.widgets.active.bg_fill = egui::Color32::from_rgb(50, 84, 32);
    v.selection.bg_fill = egui::Color32::from_rgb(40, 72, 28);
    v.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    ctx.set_visuals(v);
    // Zoom is `ui_scale`, set by the app's own keys. egui's built-in Ctrl +/- would zoom on top
    // of it from the same keypress, and everything projected from 3D (labels, spinners, badges)
    // divides by `ui_scale`: they drifted away from their figures, more the further from the
    // window's corner.
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
    *done = true;
    true
}

fn age(ms: i64) -> String {
    let d = (now_millis() - ms).max(0) / 1000;
    match d {
        0..=59 => format!("{d}s"),
        60..=3599 => format!("{}m", d / 60),
        3600..=86_399 => format!("{}h", d / 3600),
        _ => format!("{}d", d / 86_400),
    }
}

fn dur(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m {}s", s / 60, s % 60)
    } else {
        format!("{}h {}m", s / 3600, (s % 3600) / 60)
    }
}

/// Local wall-clock time of an event, hh:mm:ss.
fn clock(ms: i64) -> String {
    if ms <= 0 {
        return "--:--:--".into();
    }
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| t.with_timezone(&chrono::Local).format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "--:--:--".into())
}

fn short(id: &str) -> &str {
    &id[..id.len().min(8)]
}

fn tip(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>, help: &str) {
    ui.label(text).on_hover_text(help);
}

/// Bevy allows sixteen parameters per system; the newer resources ride together here.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Side<'w, 's> {
    pub terms: ResMut<'w, Terminals>,
    pub raid: ResMut<'w, raid::Raid>,
    pub index: ResMut<'w, files::FileIndex>,
    /// AGENT_GLOBE_OPEN=<file> was honoured once (a dev aid: open a file without a click).
    pub env_opened: Local<'s, bool>,
    pub env_toggled: Local<'s, bool>,
    pub was_typing: Local<'s, bool>,
    pub monitors: Query<'w, 's, &'static bevy::window::Monitor>,
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut contexts: EguiContexts,
    mut store: ResMut<Store>,
    mut settings: ResMut<Settings>,
    bridge: Res<BridgeRes>,
    diagnostics: Res<DiagnosticsStore>,
    cams: Query<&PanOrbitCamera>,
    sounds: Res<Sounds>,
    probe: Res<Probe>,
    gitw: Res<GitWatch>,
    usage: Res<UsageWatch>,
    serverw: Res<ServerWatch>,
    mut side: Side,
    mut images: Local<HashMap<String, Option<egui::TextureHandle>>>,
    mut styled: Local<bool>,
    mut last_panel: Local<Option<String>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    if style_once(ctx, &mut styled) {
        return Ok(());
    }
    // Checked against egui's own value every frame, so nothing else can leave the two apart.
    if (ctx.zoom_factor() - settings.ui_scale).abs() > 1e-3 {
        ctx.set_zoom_factor(settings.ui_scale);
    }
    let now = now_millis();
    store.typing = ctx.memory(|m| m.focused().is_some());
    // Files asked to open from chips and lists land in the editor card.
    let queued: Vec<String> = OPEN_QUEUE
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default();
    for p in queued {
        let full = if std::path::Path::new(&p).is_absolute() {
            p
        } else {
            match store.root_dir() {
                Some(r) => std::path::Path::new(&r).join(&p).display().to_string(),
                None => p,
            }
        };
        store.open_doc_pinned(&full);
    }
    // Ctrl+` opens or closes the terminal, from anywhere (the terminal itself included).
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Backtick))
        && let Some(sid) = store.current.clone()
    {
        store.terminal_open = if store.terminal_open.is_some() { None } else { Some(sid) };
    }
    side.index
        .configure(settings.files_show_hidden, settings.files_show_ignored);
    // The editor surface is a native child window: once it has had the keyboard, Windows hands
    // the keyboard back to it whenever the window is activated, and egui's text fields look
    // disabled. A press on the app's own side, or a text field gaining focus, takes it back.
    {
        let ppp = ctx.pixels_per_point();
        let surface = store
            .webview_rect
            .map(|r| egui::Rect::from_min_size(egui::pos2(r[0] / ppp, r[1] / ppp), egui::vec2(r[2] / ppp, r[3] / ppp)));
        let pressed_outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !surface.is_some_and(|r| r.contains(p)))
        });
        if pressed_outside || (store.typing && !*side.was_typing) {
            store.focus_app = true;
        }
        *side.was_typing = store.typing;
    }
    // Where the monitors meet, in window points: the dock grid puts a column line on each seam.
    {
        let ppp = ctx.pixels_per_point();
        let origin = side.monitors.iter().map(|m| m.physical_position.x).min().unwrap_or(0);
        let mut seams: Vec<f32> = side
            .monitors
            .iter()
            .map(|m| (m.physical_position.x - origin) as f32 / ppp)
            .filter(|x| *x > 1.0)
            .collect();
        seams.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        store.monitor_seams = seams;
    }
    if !*side.env_opened
        && let Ok(p) = std::env::var("AGENT_GLOBE_OPEN")
    {
        *side.env_opened = true;
        if std::env::var("AGENT_GLOBE_MD_EDIT").is_ok() {
            store.md_preview = false;
        }
        if std::env::var("AGENT_GLOBE_LINKS_ALL").is_ok() {
            settings.file_links_all = true;
        }
        store.open_doc(&p);
    }
    // AGENT_GLOBE_MD_TOGGLE=<secs>: flip the Markdown mode once after that many seconds (a dev aid).
    if let Ok(secs) = std::env::var("AGENT_GLOBE_MD_TOGGLE")
        && let Ok(secs) = secs.parse::<f64>()
        && !*side.env_toggled
        && ctx.input(|i| i.time) > secs
    {
        *side.env_toggled = true;
        store.md_preview = !store.md_preview;
        let preview = store.md_preview;
        bevy::log::info!("dev: markdown mode toggled, preview={preview}");
        store.editor_cmds.push(EditorCmd::Mode { preview });
    }
    if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::P)) && !store.hide_ui && !store.quick_open {
        store.quick_open = true;
        store.quick_focus = true;
        store.quick_query.clear();
        store.quick_sel = 0;
    }
    // Card-scoped shortcuts, by the card under the pointer.
    if let Some((id, step)) = store.zoom_request.take() {
        match id.as_str() {
            "editor-card" => store.editor_cmds.push(EditorCmd::Zoom(step)),
            "terminal-card" => {
                store.term_font = if step == 0 {
                    13.0
                } else {
                    (store.term_font + step as f32).clamp(9.0, 28.0)
                };
            }
            _ => {
                let cs = store.cards.entry(id).or_default();
                cs.zoom = if step == 0 {
                    1.0
                } else {
                    (cs.zoom * 1.1_f32.powi(step)).clamp(0.6, 2.5)
                };
            }
        }
    }
    if store.hot_card.as_deref() == Some("editor-card") && !store.typing {
        let (md_switch, close_tab) = ctx.input(|i| {
            (
                i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::V),
                i.modifiers.command && i.key_pressed(egui::Key::W),
            )
        });
        if md_switch {
            store.md_preview = !store.md_preview;
            let preview = store.md_preview;
            store.editor_cmds.push(EditorCmd::Mode { preview });
        }
        if close_tab && let Some(i) = store.active_doc {
            store.close_doc(i);
        }
    }
    // The compact layout is the layout: denser HUD, collapsible sessions sidebar.
    let compact = true;
    if let Some(s) = store.current_session() {
        gitw.watch(&s.cwd);
        probe.watch(
            s.agents
                .iter()
                .flat_map(|a| a.artifacts.iter())
                .filter(|a| a.kind == ArtifactKind::Url)
                .map(|a| a.target.clone())
                .collect(),
        );
    }
    let git = gitw.info();
    serverw.set_wanted(store.current_session().map(session_targets).unwrap_or_default());
    let servers_snap: Snapshot = serverw.snapshot();
    let alive_servers = servers_snap.servers.iter().filter(|x| x.alive).count();

    // ---- cinematic mode: scene only ----
    if store.hide_ui {
        store.scene_insets = Some((0.0, 0.0, 0.0, 0.0));
        store.pointer_over_ui = false;
        store.webview_rect = None;
        egui::Area::new(egui::Id::new("cinematic-hint"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -12.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("Esc: back to the console · P: raid pause / resume")
                        .small()
                        .color(DIM),
                );
            });
        draw_raid(ctx, &mut side.raid);
        draw_badges(ctx, &mut store);
        draw_progress(ctx, &store, &settings, now);
        draw_satellites(ctx, &mut store, now);
        draw_files(ctx, &mut store);
        draw_bubbles(ctx, &mut store, &probe, git.as_ref(), &mut images, now);
        draw_viewer(ctx, &mut store, &mut settings, &mut images);
        let mut cards: Vec<(&str, egui::Rect)> = Vec::new();
        if let Some(r) = draw_reply(ctx, &mut store, &mut settings) {
            cards.push(("reply-card", r));
        }
        if let Some(r) = draw_terminal(ctx, &mut store, &mut settings, &mut side.terms) {
            cards.push(("terminal-card", r));
        }
        let narrator = settings.narrator;
        if let Some(r) = draw_turn_card(ctx, &mut store, &mut settings, narrator) {
            cards.push(("turn-card", r));
        }
        store.card_rects = cards
            .iter()
            .map(|(_, r)| [r.min.x, r.min.y, r.max.x, r.max.y])
            .collect();
        let pointer = ctx.pointer_latest_pos();
        store.hot_card = cards
            .iter()
            .find(|(_, r)| pointer.is_some_and(|p| r.contains(p)))
            .map(|(id, _)| id.to_string());
        draw_toasts(ctx, &store);
        return Ok(());
    }

    let mut ui_rects: Vec<egui::Rect> = Vec::new();
    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );

    // ---- top: HUD ----
    let mut toggle_settings = false;
    let mut go_home = false;
    let mut cinematic = false;
    let mut toggle_sound = false;
    let mut toggle_usage = false;
    let mut pick_root = false;
    let mut open_term: Option<String> = None;
    let mut open_files = false;
    let mut cycle_view = false;
    let mut cycle_links = false;
    let usage_state = usage.snapshot();
    let hud = egui::Panel::top("hud").show(&mut root, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(if compact { "AG" } else { "AGENT GLOBE" }).font(stencil(20.0)).color(ACCENT));
            ui.separator();
            match store.current_session() {
                None => {
                    ui.label(egui::RichText::new("no session").color(DIM));
                }
                Some(s) => {
                    let active = s
                        .agents
                        .iter()
                        .filter(|a| a.status(now, theme::ACTIVE_WINDOW_MS) == Status::Active)
                        .count();
                    let root = s.agent(ROOT_AGENT);
                    let ctx_k = root.map(|a| a.context_tokens / 1000).unwrap_or(0);
                    let limit_k = root.map(|a| theme::context_alarm_tokens(&a.model) / 1000).unwrap_or(160);
                    let ctx_color = if ctx_k >= limit_k {
                        RED
                    } else if ctx_k * 3 >= limit_k * 2 {
                        AMBER
                    } else {
                        TEXT
                    };
                    ui.label(
                        egui::RichText::new(format!("{}  {}", s.project, short(&s.id)))
                            .color(SAND)
                            .monospace(),
                    );
                    if !s.cwd.is_empty()
                        && ui
                            .small_button("🗂")
                            .on_hover_text("Expand or collapse the files sidebar (Ctrl+P)")
                            .clicked()
                    {
                        open_files = true;
                    }
                    {
                        let (label, tip) = match store.scene_view {
                            SceneView::Both => ("⚭ both", "The constellation shows the units and the whole file system. Click: files only."),
                            SceneView::Files => ("🗂 files", "The constellation shows the file system only. Click: units only."),
                            SceneView::Agents => ("⚑ units", "The constellation shows the units only. Click: both."),
                        };
                        if ui.small_button(label).on_hover_text(tip).clicked() {
                            cycle_view = true;
                        }
                    }
                    {
                        let (label, tip) = if !settings.file_links {
                            ("⛓ off", "Links between files are off. Click: the hovered or open file's links.")
                        } else if settings.file_links_all {
                            ("⛓ all", "Every connection between files, faint. Click: off.")
                        } else {
                            ("⛓ focus", "The hovered or open file's imports, co-changes and same-turn touches. Click: every connection.")
                        };
                        if ui.small_button(label).on_hover_text(tip).clicked() {
                            cycle_links = true;
                        }
                    }
                    if !s.cwd.is_empty() {
                        let r = ui.small_button("⌨").on_hover_text(format!(
                            "Terminal in {}\nright-click: a native window instead",
                            s.cwd
                        ));
                        if r.clicked() {
                            open_term = Some(s.id.clone());
                        } else if r.secondary_clicked() {
                            open_terminal(&s.cwd);
                        }
                    }
                    ui.separator();
                    tip(
                        ui,
                        egui::RichText::new(format!("units {}", s.agents.len())).monospace(),
                        "Agents in this session: the commander (your main agent) plus every subagent it spawned.\nSubagents are separate model runs with their own context, launched through the Agent tool.",
                    );
                    tip(
                        ui,
                        egui::RichText::new(format!("active {active}")).monospace().color(ACCENT),
                        "Agents that did something in the last 20 seconds.",
                    );
                    if !compact {
                    tip(
                        ui,
                        egui::RichText::new(format!("calls {}", s.total_calls())).monospace(),
                        "Tool calls: every time an agent used a tool (read a file, ran a command, spawned a unit).\nEach call is one round trip between the model and the harness.",
                    );
                    tip(
                        ui,
                        egui::RichText::new(format!("{}/min", s.calls_since(now - 60_000))).monospace(),
                        "Tool calls in the last minute. Zero while the model is thinking or waiting for you.",
                    );
                    }
                    tip(
                        ui,
                        egui::RichText::new(format!("ctx {ctx_k}k")).monospace().color(ctx_color),
                        "Context size: how many tokens the commander's last request carried (prompt + history + tool results).\nAmber at two thirds of this model's window, red near the limit: that is where the agent starts forgetting early instructions.",
                    );
                    if let Some(until) = store.throttled_until(now) {
                        tip(
                            ui,
                            egui::RichText::new(format!("⛔ limit · resets {}", clock_hm(until))).monospace().color(RED),
                            "Claude Code reported the five-hour limit. Nothing runs until the reset time.",
                        );
                    }
                    if !compact && let Some(g) = git.as_ref() {
                        tip(
                            ui,
                            egui::RichText::new(format!("⎇ {} · {}Δ", g.branch, g.dirty)).monospace().color(DIM),
                            "Git branch of the session's folder and how many files are uncommitted right now.",
                        );
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⚙ settings").clicked() {
                    toggle_settings = true;
                }
                if ui.button("⌂ home").on_hover_text("Reset the camera (R)").clicked() {
                    go_home = true;
                }
                if ui.button("🎬").on_hover_text("Cinematic mode: hide the console (F)").clicked() {
                    cinematic = true;
                }
                let icon = if settings.sound { "🔊" } else { "🔇" };
                if ui.button(icon).on_hover_text("Mute / unmute the radio").clicked() {
                    toggle_sound = true;
                }
                // Servers this session is running right now.
                if alive_servers > 0 {
                    ui.separator();
                    let list: Vec<String> = servers_snap
                        .servers
                        .iter()
                        .filter(|x| x.alive)
                        .map(|x| format!("{} {}", x.title.clone().unwrap_or_default(), x.url))
                        .collect();
                    if ui
                        .button(egui::RichText::new(format!("⚡ {alive_servers}")).color(ACCENT))
                        .on_hover_text(format!("Servers listening:\n{}\nClick to open the commander's panel.", list.join("\n")))
                        .clicked()
                    {
                        pick_root = true;
                    }
                }
                // Plan usage: the same numbers the editor shows.
                ui.separator();
                match &usage_state {
                    Some(UsageState::Ready(u)) => {
                        let shown = if compact { 1 } else { u.limits.len().min(3) };
                        for l in u.limits.iter().take(shown).rev() {
                            let frac = l.utilization / 100.0;
                            let color = usage_color(l.utilization);
                            let resp = ui
                                .horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{} {:.0}%", usage::short_label(&l.key), l.utilization))
                                            .monospace()
                                            .color(color),
                                    );
                                    ui.add(
                                        egui::ProgressBar::new(frac)
                                            .desired_width(54.0)
                                            .desired_height(7.0)
                                            .fill(color),
                                    );
                                })
                                .response;
                            let tip_text = format!("{}: {:.0}% used. {}", l.label, l.utilization, usage::resets_text(l.resets_at, now));
                            if resp.on_hover_text(tip_text).interact(egui::Sense::click()).clicked() {
                                toggle_usage = true;
                            }
                        }
                        if ui.small_button("📊").on_hover_text("Usage: session, weekly and per-model limits").clicked() {
                            toggle_usage = true;
                        }
                    }
                    Some(UsageState::NoLogin) => {
                        ui.label(egui::RichText::new("usage: no login").small().color(DIM))
                            .on_hover_text("No Claude Code login found on this machine. Run `claude` once and sign in; the usage shows here within a minute.");
                    }
                    Some(UsageState::Error(e)) => {
                        ui.label(egui::RichText::new("usage: ?").small().color(AMBER)).on_hover_text(e);
                    }
                    None => {
                        ui.label(egui::RichText::new("usage…").small().color(DIM));
                    }
                }
            });
        });
        ui.add_space(4.0);
    });
    ui_rects.push(hud.response.rect);
    let inset_top = hud.response.rect.height();
    if toggle_settings {
        store.show_settings = !store.show_settings;
    }
    if toggle_usage {
        store.show_usage = !store.show_usage;
    }
    if pick_root {
        store.selected = Some(ROOT_AGENT.to_string());
    }
    if open_files {
        store.files_open = !store.files_open;
    }
    if cycle_view {
        store.scene_view = store.scene_view.next();
    }
    if cycle_links {
        // off → focus → all → off
        if !settings.file_links {
            settings.file_links = true;
            settings.file_links_all = false;
        } else if !settings.file_links_all {
            settings.file_links_all = true;
        } else {
            settings.file_links = false;
            settings.file_links_all = false;
        }
    }
    if let Some(sid) = open_term {
        store.terminal_open = if store.terminal_open.as_deref() == Some(sid.as_str()) {
            None
        } else {
            Some(sid)
        };
    }
    if go_home {
        store.selected = None;
        store.reset_view = true;
    }
    if cinematic {
        store.hide_ui = true;
    }
    if toggle_sound {
        settings.sound = !settings.sound;
        if settings.sound {
            store.play(Sfx::Spawn);
        }
    }

    // ---- left: sessions ----
    let mut open: Option<SessionRef> = None;
    let mut show: Option<String> = None;
    let mut pick: Option<String> = None;
    let mut go: Option<(String, String)> = None;
    let attention = store.attention(now);
    let mut needy: HashMap<String, &'static str> = HashMap::new();
    for a in &attention {
        needy.entry(a.session_id.clone()).or_insert(badge_style(a.kind).0);
    }
    let collapsed = compact && !store.sidebar_open;
    let mut sidebar_toggle = false;
    let (left_rect, inset_left) = if collapsed {
        // A strip of session dots: enough to see who needs you and to switch, in a half-screen window.
        let strip = egui::Panel::left("sessions-strip")
            .resizable(false)
            .default_size(44.0)
            .show(&mut root, |ui| {
                ui.add_space(6.0);
                ui.vertical_centered(|ui| {
                    if ui.small_button("☰").on_hover_text("Show the sessions list").clicked() {
                        sidebar_toggle = true;
                    }
                    if !attention.is_empty() {
                        ui.label(
                            egui::RichText::new(format!("{}", attention.len()))
                                .color(AMBER)
                                .strong(),
                        )
                        .on_hover_text("Things that need you: approvals, failures, waiting commanders, stalls");
                    }
                    ui.separator();
                    for s in store
                        .session_list
                        .iter()
                        .filter(|s| store.watched.contains(&s.id))
                        .take(12)
                    {
                        let is_current = store.current.as_deref() == Some(s.id.as_str());
                        let flag = needy.get(&s.id).copied();
                        let glyph = flag.unwrap_or(if now - s.modified < theme::ACTIVE_WINDOW_MS {
                            "●"
                        } else {
                            "○"
                        });
                        let color = if flag.is_some() {
                            AMBER
                        } else if is_current {
                            ACCENT
                        } else {
                            DIM
                        };
                        if ui
                            .add(egui::Button::new(egui::RichText::new(glyph).color(color)).frame(is_current))
                            .on_hover_text(format!("{}  {}  {} ago", s.project, short(&s.id), age(s.modified)))
                            .clicked()
                        {
                            open = Some(s.clone());
                        }
                    }
                });
            });
        (strip.response.rect, strip.response.rect.width())
    } else {
        let left = egui::Panel::left("sessions")
        .resizable(true)
        .default_size(230.0)
        .show(&mut root, |ui| {
            ui.add_space(6.0);
            if compact && ui.small_button("« collapse").clicked() {
                sidebar_toggle = true;
            }
            ui.label(
                egui::RichText::new(store.claude_dir.display().to_string())
                    .small()
                    .color(DIM),
            );
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                if !attention.is_empty() {
                    ui.label(egui::RichText::new("NEEDS YOU").small().color(AMBER))
                        .on_hover_text("Across every session being watched: approvals, failures, commanders waiting for orders, stalls.");
                    for a in attention.iter().take(8) {
                        let (glyph, color, _) = badge_style(a.kind);
                        ui.horizontal(|ui| {
                            let text = format!("{glyph} {}  {}", a.project, a.label);
                            let row = ui
                                .add(
                                    egui::Button::new(egui::RichText::new(text).small().color(color))
                                        .frame(false)
                                        .truncate(),
                                )
                                .on_hover_text(format!("{} · {} ago · click to look", short(&a.session_id), age(a.since)));
                            if row.clicked() {
                                go = Some((a.session_id.clone(), a.agent_id.clone()));
                            }
                        });
                    }
                    ui.separator();
                }
                let mut live: Vec<&str> = store
                    .sessions
                    .keys()
                    .filter(|k| !store.session_list.iter().any(|s| &s.id == *k))
                    .map(String::as_str)
                    .collect();
                live.sort();
                if !live.is_empty() {
                    ui.label(egui::RichText::new("LIVE (ingest)").small().color(DIM));
                    for id in live {
                        let sel = store.current.as_deref() == Some(id);
                        if ui.selectable_label(sel, format!("● {}", short(id))).clicked() {
                            show = Some(id.to_string());
                        }
                    }
                    ui.separator();
                }
                ui.label(egui::RichText::new("SESSIONS").small().color(DIM));
                let mut projects: Vec<&str> = Vec::new();
                for s in &store.session_list {
                    if !projects.contains(&s.project.as_str()) {
                        projects.push(&s.project);
                    }
                }
                for project in projects {
                    let has_current = store
                        .session_list
                        .iter()
                        .any(|s| s.project == project && store.current.as_deref() == Some(s.id.as_str()));
                    let header = egui::RichText::new(project).color(SAND).small();
                    egui::CollapsingHeader::new(header)
                        .id_salt(project)
                        .default_open(has_current)
                        .show(ui, |ui| {
                            for s in store.session_list.iter().filter(|s| s.project == project) {
                                let is_current = store.current.as_deref() == Some(s.id.as_str());
                                let active = now - s.modified < theme::ACTIVE_WINDOW_MS;
                                let dot = if active { "●" } else { "○" };
                                let flag = needy.get(&s.id).copied().unwrap_or("");
                                let text = format!(
                                    "{dot} {}  {:>3}  {:>6}k {flag}",
                                    short(&s.id),
                                    age(s.modified),
                                    s.size / 1024
                                );
                                let rt = egui::RichText::new(text).monospace();
                                let rt = if active { rt.color(ACCENT) } else { rt };
                                ui.horizontal(|ui| {
                                    if ui.selectable_label(is_current, rt).clicked() {
                                        open = Some(s.clone());
                                    }
                                });
                                if is_current && let Some(session) = store.sessions.get(&s.id) {
                                    agent_tree(ui, session, ROOT_AGENT, 0, now, store.selected.as_deref(), &mut pick);
                                }
                            }
                        });
                }
            });
        });
        (left.response.rect, left.response.rect.width())
    };
    ui_rects.push(left_rect);
    if sidebar_toggle {
        store.sidebar_open = !store.sidebar_open;
    }
    // ---- files sidebar: beside the sessions, collapsible to a strip, pushing the dock area ----
    if store.workspace.is_none() && !settings.workspace.is_empty() {
        store.workspace = Some(settings.workspace.clone());
    }
    let files_ctx = store.root_dir().map(|cwd| {
        let project = store
            .current_session()
            .filter(|s| !s.cwd.is_empty())
            .map(|s| s.project.clone())
            .unwrap_or_else(|| {
                cwd.rsplit(['\\', '/'])
                    .find(|p| !p.is_empty())
                    .unwrap_or("folder")
                    .to_string()
            });
        (cwd, project)
    });
    let mut files_w = 0.0_f32;
    let mut files_toggle = false;
    let mut pick_folder = false;
    if files_ctx.is_none() {
        // No session on screen and no folder opened: the strip offers the picker.
        let strip = egui::Panel::left("files-strip")
            .resizable(false)
            .default_size(34.0)
            .show(&mut root, |ui| {
                ui.add_space(6.0);
                ui.vertical_centered(|ui| {
                    if ui
                        .small_button("📁")
                        .on_hover_text("Open a folder: its files fill the rings, the tree and the finder")
                        .clicked()
                    {
                        pick_folder = true;
                    }
                });
            });
        files_w = strip.response.rect.width();
        ui_rects.push(strip.response.rect);
    }
    if let Some((cwd, project)) = files_ctx {
        if store.files_open {
            let panel = egui::Panel::left("files")
                .resizable(true)
                .default_size(330.0)
                .show(&mut root, |ui| {
                    editor::files_body(ui, ctx, &mut store, &mut side.index, &cwd, &project)
                });
            files_w = panel.response.rect.width();
            ui_rects.push(panel.response.rect);
            if panel.inner.collapse {
                files_toggle = true;
            }
            if panel.inner.pick_folder {
                pick_folder = true;
            }
        } else {
            let strip = egui::Panel::left("files-strip")
                .resizable(false)
                .default_size(34.0)
                .show(&mut root, |ui| {
                    ui.add_space(6.0);
                    ui.vertical_centered(|ui| {
                        if ui
                            .small_button("🗂")
                            .on_hover_text("Files: the session folder as a tree, with a finder (Ctrl+P)")
                            .clicked()
                        {
                            files_toggle = true;
                        }
                    });
                });
            files_w = strip.response.rect.width();
            ui_rects.push(strip.response.rect);
        }
    }
    if files_toggle {
        store.files_open = !store.files_open;
    }
    if pick_folder && let Some(p) = rfd::FileDialog::new().set_title("Open folder").pick_folder() {
        let chosen = p.to_string_lossy().to_string();
        settings.workspace = chosen.clone();
        store.workspace = Some(chosen);
        store.files_open = true;
        store.files_open_dirs.clear();
    }
    let inset_left = inset_left + files_w;
    if let Some(s) = open {
        store.open(&s, &bridge.0);
    }
    if let Some(id) = show {
        store.show(&id);
    }
    if let Some(id) = pick {
        store.selected = Some(id);
    }
    if let Some((sid, aid)) = go {
        store.acked.insert(format!("err:{sid}:{aid}"));
        store.acked.insert(format!("done:{sid}:{aid}"));
        if store.current.as_deref() == Some(sid.as_str()) {
            store.selected = Some(aid);
        } else {
            store.pending_open = Some(sid);
            store.pending_select = Some(aid);
        }
    }

    // ---- bottom: status ----
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let cam_dist = cams.iter().next().and_then(|c| c.radius).unwrap_or(0.0);
    let bottom = egui::Panel::bottom("status").show(&mut root, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{fps:5.0} fps")).monospace().color(DIM));
            ui.label(
                egui::RichText::new(format!("cam {cam_dist:4.1}"))
                    .monospace()
                    .color(DIM),
            );
            ui.separator();
            ui.label(egui::RichText::new(&store.status).monospace().color(DIM));
            if let Some(p) = store.http_port {
                ui.label(egui::RichText::new(format!("ingest :{p}")).monospace().color(DIM));
            }
            ui.separator();
            ui.label(
                egui::RichText::new(if compact {
                    "drag · scroll · R home · F11"
                } else {
                    "drag rotate · scroll zoom · right-drag pan · click unit or file · R home · F11 full screen · Ctrl +/- UI size"
                })
                .small()
                .color(DIM),
            );
            if let Some(e) = store.errors.last() {
                ui.separator();
                ui.label(egui::RichText::new(e).monospace().color(RED));
            }
        });
    });
    ui_rects.push(bottom.response.rect);
    let inset_bottom = bottom.response.rect.height();

    // ---- right: selected agent ----
    let selected = store
        .selected
        .as_deref()
        .and_then(|id| store.current_session().and_then(|s| s.agent(id)))
        .cloned();
    let hovered = store
        .hovered
        .as_deref()
        .and_then(|id| store.current_session().and_then(|s| s.agent(id)))
        .cloned();
    let mut close = false;
    let mut view_image: Option<String> = None;
    let mut query = std::mem::take(&mut store.artifact_query);
    let mut history = std::mem::take(&mut store.history_query);
    let mut fold_groups = std::mem::take(&mut store.fold_groups);
    let mut kill_armed = store.kill_armed.take();
    let mut docker_armed = store.docker_armed.take();
    let mut kill_request: Option<u32> = None;
    let mut docker_request: Option<(String, String, String)> = None;
    let mut open_turn: Option<usize> = None;
    let mut inset_right = 0.0_f32;
    let just_opened = selected.as_ref().map(|a| &a.id) != last_panel.as_ref();
    *last_panel = selected.as_ref().map(|a| a.id.clone());
    let mut reconnect = false;
    // The commander's session id is what `claude --resume` takes: with it, a session whose
    // terminal was closed can be picked up again.
    let resume_cmd = selected
        .as_ref()
        .filter(|a| a.depth == 0)
        .and(store.current.as_ref())
        .map(|sid| format!("claude --resume {sid}"));
    if let Some(agent) = selected.as_ref() {
        let narrator = settings.narrator;
        let stalled = store.stalled.clone();
        // Which changed files this session's agents edited, by repo-relative path.
        let changes: Vec<FileChange> = if agent.depth == 0 {
            git.as_ref().map(|g| g.changes.clone()).unwrap_or_default()
        } else {
            Vec::new()
        };
        let cwd = git.as_ref().map(|g| g.cwd.clone()).unwrap_or_default();
        let touched: HashMap<String, usize> = if changes.is_empty() {
            HashMap::new()
        } else {
            let edits: Vec<String> = store
                .current_session()
                .map(|s| {
                    s.agents
                        .iter()
                        .flat_map(|a| a.calls.iter())
                        .filter(|c| matches!(c.tool.as_str(), "Edit" | "Write" | "NotebookEdit" | "MultiEdit"))
                        .map(|c| c.summary.replace('\\', "/").to_ascii_lowercase())
                        .collect()
                })
                .unwrap_or_default();
            changes
                .iter()
                .map(|c| {
                    let needle = c.path.to_ascii_lowercase();
                    (c.path.clone(), edits.iter().filter(|e| e.ends_with(&needle)).count())
                })
                .filter(|(_, n)| *n > 0)
                .collect()
        };
        let vw = ctx.viewport_rect().width();
        let remembered = settings.agent_panel_width;
        let right = egui::Panel::right("agent")
            .resizable(true)
            .default_size(if remembered > 100.0 {
                remembered
            } else if compact {
                vw * 0.55
            } else {
                520.0
            })
            .max_size(vw * if compact { 0.8 } else { 0.6 })
            .show(&mut root, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.small_button("✕ close  (Esc)").clicked() {
                        close = true;
                    }
                    if let Some(cmd) = &resume_cmd {
                        if ui
                            .small_button("⟲ reconnect")
                            .on_hover_text(format!("Pick this session up again in a new terminal tab:\n{cmd}"))
                            .clicked()
                        {
                            reconnect = true;
                        }
                        if ui.small_button("⧉").on_hover_text(format!("Copy  {cmd}")).clicked() {
                            ui.ctx().copy_text(cmd.clone());
                        }
                    }
                });
                // Clipped: the width you drag the panel to holds, whatever the agent's rows are.
                clipped(ui, |ui| {
                    agent_panel(
                        ui,
                        agent,
                        now,
                        &mut store.expanded_calls,
                        just_opened,
                        narrator,
                        &stalled,
                        &probe,
                        git.as_ref(),
                        &mut images,
                        &mut view_image,
                        &mut query,
                        &changes,
                        &cwd,
                        &touched,
                        &mut history,
                        &mut fold_groups,
                        &servers_snap,
                        &mut kill_armed,
                        &mut docker_armed,
                        &mut kill_request,
                        &mut docker_request,
                        &mut open_turn,
                    )
                });
            });
        inset_right = right.response.rect.width();
        if reconnect && let (Some(cmd), Some(sid)) = (resume_cmd.clone(), store.current.clone()) {
            store.term_run = Some(cmd);
            store.terminal_open = Some(sid);
        }
        // The width you dragged it to is the width it opens at, this run and the next.
        if (inset_right - settings.agent_panel_width).abs() > 1.0 && inset_right > 100.0 {
            settings.agent_panel_width = inset_right;
        }
        ui_rects.push(right.response.rect);
    }
    store.artifact_query = query;
    if let Some(ti) = open_turn
        && let Some(sid) = store.current.clone()
        && let Some(aid) = selected.as_ref().map(|a| a.id.clone())
    {
        store.turn_open = Some((sid, aid, ti));
        store.turn_tab = 0;
    }
    store.history_query = history;
    store.fold_groups = fold_groups;
    store.kill_armed = kill_armed.filter(|(_, at)| at.elapsed().as_secs_f32() < 3.0);
    store.docker_armed = docker_armed.filter(|(_, at)| at.elapsed().as_secs_f32() < 3.0);
    if let Some(pid) = kill_request {
        let msg = servers::kill(pid);
        store.toast(
            if msg.starts_with('✖') {
                ToastKind::Warn
            } else {
                ToastKind::Error
            },
            msg,
        );
    }
    if let Some((verb, id, name)) = docker_request {
        let ok = matches!(verb.as_str(), "start" | "stop");
        let msg = servers::docker_action(&verb, &id, &name);
        let clean = msg.starts_with('▶') || msg.starts_with('⏸') || msg.starts_with('✖');
        store.toast(
            if clean && ok {
                ToastKind::Done
            } else if clean {
                ToastKind::Warn
            } else {
                ToastKind::Error
            },
            msg,
        );
    }
    if close {
        store.selected = None;
    }
    if let Some(path) = view_image.take() {
        open_viewer(&mut store, &path);
    }
    // Hover over a satellite: a card about that session.
    let hovered_session = store
        .hovered_session
        .as_ref()
        .and_then(|id| store.sessions.get(id))
        .cloned();
    if let Some(sess) = hovered_session.as_ref() {
        egui::Area::new(egui::Id::new("hover-session"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 16.0 + inset_top))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(PANEL)
                    .stroke(egui::Stroke::new(1.0, SAND))
                    .inner_margin(10.0)
                    .corner_radius(6.0)
                    .show(ui, |ui| {
                        ui.set_max_width(340.0);
                        session_card(ui, sess, now);
                    });
            });
    }
    // Hover: a small card at the top right that never steals the pointer.
    if let Some(agent) = hovered
        .as_ref()
        .filter(|h| selected.as_ref().map(|s| &s.id) != Some(&h.id))
        .filter(|_| hovered_session.is_none())
    {
        egui::Area::new(egui::Id::new("hover-card"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 16.0 + inset_top))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(PANEL)
                    .stroke(egui::Stroke::new(1.0, ACCENT))
                    .inner_margin(10.0)
                    .corner_radius(6.0)
                    .show(ui, |ui| {
                        ui.set_max_width(340.0);
                        hover_card(ui, agent, now);
                    });
            });
    }

    // ---- settings card ----
    let mut settings_rect: Option<egui::Rect> = None;
    if store.show_settings {
        let maxed = store.settings_max;
        let (mut cs, mut dock, grid) = begin_card(&mut store, &mut settings, "settings-card", ctx);
        let out = floating_card(
            ctx,
            "settings-card",
            maxed,
            &mut cs,
            &mut dock,
            grid,
            egui::pos2(260.0, 80.0),
            egui::vec2(440.0, 600.0),
            None,
            |ui| {
                ui.label(egui::RichText::new("⚙").size(16.0));
                ui.label(egui::RichText::new("settings").font(stencil(14.0)).color(SAND));
            },
            |ui, _q| {
                egui::ScrollArea::vertical()
                    .id_salt("settings-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                ui.set_min_width(ui.available_width() - 8.0);
                ui.label(egui::RichText::new("RADIO").small().color(DIM));
                ui.checkbox(&mut settings.sound, "sound on");
                ui.add(egui::Slider::new(&mut settings.volume, 0.0..=1.0).text("volume"));
                let current = if settings.audio_device.is_empty() {
                    "system default".to_string()
                } else {
                    settings.audio_device.clone()
                };
                egui::ComboBox::from_label("output device")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(settings.audio_device.is_empty(), "system default")
                            .clicked()
                        {
                            settings.audio_device.clear();
                        }
                        for name in sounds.device_names() {
                            if ui.selectable_label(settings.audio_device == name, &name).clicked() {
                                settings.audio_device = name;
                            }
                        }
                    });
                ui.label(egui::RichText::new(sounds.status()).small().color(DIM));
                ui.checkbox(&mut settings.tool_blips, "blip on every tool call");
                ui.collapsing(egui::RichText::new("what the radio means").small().color(DIM), |ui| {
                    egui::Grid::new("radio-glossary")
                        .num_columns(2)
                        .spacing([14.0, 2.0])
                        .show(ui, |ui| {
                            for s in Sfx::ALL.iter().copied().filter(|s| s.is_voice()) {
                                ui.label(egui::RichText::new(format!("“{}”", s.line())).small().color(SAND));
                                ui.label(egui::RichText::new(s.when()).small().color(DIM));
                                ui.end_row();
                            }
                        });
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("test:").small().color(DIM));
                    for s in Sfx::ALL.iter().copied().filter(|s| s.is_voice()) {
                        if ui
                            .small_button(s.name())
                            .on_hover_text(format!("“{}” — {}", s.line(), s.when()))
                            .clicked()
                        {
                            store.play(s);
                        }
                    }
                });
                let overrides = if sounds.overrides.is_empty() {
                    "none yet".to_string()
                } else {
                    sounds.overrides.join(", ")
                };
                ui.label(
                    egui::RichText::new(format!(
                        "your own clips go in {}/sounds/<name>.wav (loaded: {overrides})",
                        super::settings::config_dir().display()
                    ))
                    .small()
                    .color(DIM),
                );
                ui.separator();
                ui.label(egui::RichText::new("CAMERA").small().color(DIM));
                ui.add(egui::Slider::new(&mut settings.orbit_sensitivity, 0.05..=2.0).text("orbit speed"));
                ui.add(egui::Slider::new(&mut settings.zoom_sensitivity, 0.05..=2.0).text("zoom speed"));
                ui.add(egui::Slider::new(&mut settings.pan_sensitivity, 0.05..=2.0).text("pan speed"));
                ui.checkbox(&mut settings.idle_orbit, "cinematic orbit when idle");
                ui.separator();
                ui.label(egui::RichText::new("LOOK").small().color(DIM));
                ui.add(egui::Slider::new(&mut settings.drift_speed, 0.0..=4.0).text("orbit drift"));
                ui.add(egui::Slider::new(&mut settings.bloom, 0.0..=0.6).text("bloom"));
                ui.add(egui::Slider::new(&mut settings.ui_scale, 0.6..=1.8).text("ui scale"));
                ui.checkbox(&mut settings.scanlines, "CRT scanlines");
                ui.separator();
                ui.label(egui::RichText::new("LIGHT").small().color(DIM));
                ui.add(egui::Slider::new(&mut settings.key_light, 0.0..=8000.0).text("key light"))
                    .on_hover_text("The warm light from above right, in lux.");
                ui.add(egui::Slider::new(&mut settings.fill_light, 0.0..=3000.0).text("fill light"))
                    .on_hover_text("The cool light from the sky side, in lux.");
                ui.add(egui::Slider::new(&mut settings.ambient_light, 0.0..=800.0).text("ambient"))
                    .on_hover_text("Light from everywhere, in nits. Lifts the shadow side of the plastic.");
                ui.add(egui::Slider::new(&mut settings.fog, 0.2..=3.0).text("fog"))
                    .on_hover_text("Above one the fog closes in; below one it pulls back. One is the original.");
                ui.separator();
                ui.label(egui::RichText::new("SCENE").small().color(DIM));
                ui.checkbox(&mut settings.narrator, "narrator line under each call");
                ui.checkbox(&mut settings.progress_rings, "progress rings and hints under units");
                ui.checkbox(
                    &mut settings.satellites,
                    "other active sessions as satellites around this one",
                );
                ui.checkbox(
                    &mut settings.files_ring,
                    "file ring: the session's hottest files with beams from the units touching them",
                );
                ui.checkbox(&mut settings.file_links, "links between files")
                    .on_hover_text("Lines from the hovered or open file to the files it imports, the files that change with it in git, and the files the same unit touched in the same stretch.");
                ui.checkbox(&mut settings.file_links_all, "all links, always")
                    .on_hover_text("Every connection, faint. Busy on a big project.");
                ui.checkbox(&mut settings.files_show_hidden, "show hidden files")
                    .on_hover_text("Dotfiles and dot-folders (.github, .vscode, .claude) in the rings, the tree and the finder.");
                ui.checkbox(&mut settings.files_show_ignored, "show ignored files")
                    .on_hover_text("Files the gitignore excludes. Build and dependency folders (target, node_modules, dist) stay out regardless: they run to tens of thousands of files.");
                ui.label(
                    egui::RichText::new(
                        "approvals from any window: Ctrl+Alt+Y allows, Ctrl+Alt+N denies the oldest pending request",
                    )
                    .small()
                    .color(DIM),
                );
                ui.separator();
                ui.label(egui::RichText::new("ALERTS").small().color(DIM));
                ui.checkbox(
                    &mut settings.desktop_notifications,
                    "Windows notifications when unfocused",
                );
                ui.label(
                    egui::RichText::new("approvals from the globe: add the PreToolUse hook from the README")
                        .small()
                        .color(DIM),
                );
                ui.separator();
                ui.label(egui::RichText::new("WINDOW").small().color(DIM));
                ui.checkbox(&mut settings.title_bar, "native title bar")
                    .on_hover_text("Windows' own title bar in windowed mode (F11 leaves full screen). Off: the app is all chrome.");
                ui.checkbox(&mut settings.span_monitors, "span all monitors at start")
                    .on_hover_text("One borderless window across every monitor. Off: full screen on one monitor. Takes effect at the next start.");
                ui.label(egui::RichText::new("DOCK GRID").small().color(DIM))
                    .on_hover_text("Drag a card by its title and drop it on a target to dock it. Drag a docked card's edge to resize it; neighbours follow.");
                ui.add(egui::Slider::new(&mut settings.grid_rows, 1..=4).text("rows"))
                    .on_hover_text("Columns follow the monitors: each monitor's free span is split in two.");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .button("reset defaults")
                        .on_hover_text("Everything back to the original values. Docked panes and the agent panel's width stay.")
                        .clicked()
                    {
                        let keep = (settings.panes.clone(), settings.agent_panel_width);
                        *settings = Settings::default();
                        settings.panes = keep.0;
                        settings.agent_panel_width = keep.1;
                    }
                    ui.label(
                        egui::RichText::new(super::settings::path().display().to_string())
                            .small()
                            .color(DIM),
                    );
                });
                    });
            },
        );
        end_card(&mut store, &mut settings, "settings-card", cs, dock, &out.sash);
        settings_rect = out.rect;
        if let Some(r) = out.rect {
            ui_rects.push(r);
        }
        if out.toggle_max {
            store.settings_max = !store.settings_max;
        }
        if out.close {
            store.show_settings = false;
        }
    }

    // ---- usage window ----
    if store.show_usage {
        let mut open = true;
        let win = egui::Window::new("📊 usage")
            .open(&mut open)
            .resizable(false)
            .default_width(300.0)
            .default_pos(egui::pos2(ctx.viewport_rect().width() - 360.0, 80.0))
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("USAGE").font(stencil(16.0)).color(SAND));
                ui.add_space(4.0);
                match &usage_state {
                    Some(UsageState::Ready(u)) => {
                        for l in &u.limits {
                            let color = usage_color(l.utilization);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(&l.label).size(16.0).color(TEXT));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{:.0}%", l.utilization))
                                            .size(16.0)
                                            .color(color),
                                    );
                                });
                            });
                            ui.add(
                                egui::ProgressBar::new(l.utilization / 100.0)
                                    .desired_width(280.0)
                                    .desired_height(10.0)
                                    .fill(color),
                            );
                            ui.label(
                                egui::RichText::new(usage::resets_text(l.resets_at, now))
                                    .small()
                                    .color(DIM),
                            );
                            ui.add_space(8.0);
                        }
                        ui.label(
                            egui::RichText::new(format!("fetched {} ago · refreshes every minute", age(u.fetched_at)))
                                .small()
                                .color(DIM),
                        );
                    }
                    Some(UsageState::NoLogin) => {
                        ui.label(
                            egui::RichText::new(
                                "No Claude Code login found on this machine. Run `claude` once and sign in.",
                            )
                            .color(DIM),
                        );
                    }
                    Some(UsageState::Error(e)) => {
                        ui.label(egui::RichText::new(e).color(AMBER));
                    }
                    None => {
                        ui.label(egui::RichText::new("fetching…").color(DIM));
                    }
                }
            });
        if let Some(w) = win {
            ui_rects.push(w.response.rect);
        }
        if !open {
            store.show_usage = false;
        }
    }

    draw_badges(ctx, &mut store);
    draw_progress(ctx, &store, &settings, now);
    draw_satellites(ctx, &mut store, now);
    draw_files(ctx, &mut store);
    draw_bubbles(ctx, &mut store, &probe, git.as_ref(), &mut images, now);
    if let Some(path) = store.pending_open_file.take() {
        // First click: pick the file, light it and its connections, glide towards it. Second: open.
        let again = store
            .selected_file
            .as_deref()
            .is_some_and(|s| super::scene::same_file(s, &path));
        if again {
            store.open_doc(&path);
        } else {
            store.selected_file = Some(path.clone());
            store.focus_file = Some(path);
        }
    }
    if let Some(rect) = draw_viewer(ctx, &mut store, &mut settings, &mut images) {
        ui_rects.push(rect);
    }
    if let Some(rect) = draw_approvals(ctx, &mut store, &bridge.0, inset_top) {
        ui_rects.push(rect);
    }
    let mut cards: Vec<(&str, egui::Rect)> = Vec::new();
    if let Some(r) = draw_reply(ctx, &mut store, &mut settings) {
        cards.push(("reply-card", r));
    }
    if let Some(r) = draw_terminal(ctx, &mut store, &mut settings, &mut side.terms) {
        cards.push(("terminal-card", r));
    }
    let narrator = settings.narrator;
    if let Some(r) = draw_turn_card(ctx, &mut store, &mut settings, narrator) {
        cards.push(("turn-card", r));
    }
    if let Some(r) = editor::draw_editor_card(ctx, &mut store, &mut settings) {
        cards.push(("editor-card", r));
    }
    if let Some(r) = settings_rect {
        cards.push(("settings-card", r));
    }
    editor::draw_quick_open(ctx, &mut store, &mut side.index);
    store.card_rects = cards
        .iter()
        .map(|(_, r)| [r.min.x, r.min.y, r.max.x, r.max.y])
        .collect();
    let pointer = ctx.pointer_latest_pos();
    store.hot_card = cards
        .iter()
        .find(|(_, r)| pointer.is_some_and(|p| r.contains(p)))
        .map(|(id, _)| id.to_string());
    ui_rects.extend(cards.iter().map(|(_, r)| *r));
    draw_toasts(ctx, &store);

    store.scene_insets = Some((inset_left, inset_top, inset_right, inset_bottom));
    if settings.scanlines {
        let vr = ctx.viewport_rect();
        let area = egui::Rect::from_min_max(
            egui::pos2(vr.min.x + inset_left, vr.min.y + inset_top),
            egui::pos2(vr.max.x - inset_right, vr.max.y - inset_bottom),
        );
        let painter = ctx.layer_painter(egui::LayerId::background());
        let mut y = area.min.y;
        while y < area.max.y {
            let line = egui::Rect::from_min_max(egui::pos2(area.min.x, y), egui::pos2(area.max.x, y + 1.0));
            painter.rect_filled(line, 0.0, SCANLINE);
            y += 3.0;
        }
    }
    store.pointer_over_ui = ctx
        .pointer_latest_pos()
        .map(|p| ui_rects.iter().any(|r| r.contains(p)))
        .unwrap_or(false);
    Ok(())
}

/// The raid's console, bottom-left in cinematic mode: phase, wave, level, score, data integrity,
/// and the controls. A big wave banner flashes mid-screen when a message is live.
fn draw_raid(ctx: &egui::Context, raid: &mut raid::Raid) {
    use raid::{Phase, Request};
    if let Some((msg, ttl)) = raid.message.clone() {
        let alpha = (ttl / 0.6).clamp(0.0, 1.0);
        egui::Area::new(egui::Id::new("raid-banner"))
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 90.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(msg)
                        .font(stencil(30.0))
                        .color(AMBER.gamma_multiply(alpha)),
                );
            });
    }
    egui::Area::new(egui::Id::new("raid-hud"))
        .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(16.0, -12.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let border = if raid.flash > 0.0 { RED } else { ACCENT };
            egui::Frame::NONE
                .fill(egui::Color32::from_rgba_unmultiplied(6, 12, 5, 215))
                .stroke(egui::Stroke::new(1.0, border))
                .inner_margin(10.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.set_width(250.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("🛸 RAID").font(stencil(15.0)).color(SAND));
                        let (tag, color) = match raid.phase {
                            Phase::Idle => ("idle", DIM),
                            Phase::Running => ("live", ACCENT),
                            Phase::Paused => ("paused", AMBER),
                            Phase::Over => ("over", RED),
                        };
                        ui.label(egui::RichText::new(tag).small().color(color));
                    });
                    match raid.phase {
                        Phase::Idle => {
                            ui.label(
                                egui::RichText::new("Saucers come for the data. Click them before they land.")
                                    .small()
                                    .color(DIM),
                            );
                            if raid.best > 0 {
                                ui.label(egui::RichText::new(format!("best {}", raid.best)).small().color(SAND));
                            }
                            if ui.button("▶ start raid  (P)").clicked() {
                                raid.request = Some(Request::Start);
                            }
                        }
                        Phase::Running | Phase::Paused => {
                            ui.label(
                                egui::RichText::new(format!(
                                    "wave {} · level {} · score {} · kills {}",
                                    raid.wave, raid.level, raid.score, raid.kills
                                ))
                                .small()
                                .monospace()
                                .color(TEXT),
                            );
                            let pct = (raid.integrity * 100.0).round() as i32;
                            let fill = if raid.integrity > 0.6 {
                                egui::Color32::from_rgb(70, 130, 45)
                            } else if raid.integrity > 0.3 {
                                AMBER
                            } else {
                                RED
                            };
                            ui.add(
                                egui::ProgressBar::new(raid.integrity)
                                    .fill(fill)
                                    .text(egui::RichText::new(format!("data integrity {pct}%")).small()),
                            );
                            let inbound = raid.alive + raid.to_spawn as usize;
                            ui.label(
                                egui::RichText::new(format!("{inbound} inbound · {} in the air", raid.alive))
                                    .small()
                                    .color(DIM),
                            );
                            ui.horizontal(|ui| {
                                if raid.phase == Phase::Running {
                                    if ui.button("⏸ pause").clicked() {
                                        raid.request = Some(Request::Pause);
                                    }
                                } else {
                                    if ui.button("▶ resume").clicked() {
                                        raid.request = Some(Request::Resume);
                                    }
                                    if ui.button("⟲ replay").clicked() {
                                        raid.request = Some(Request::Replay);
                                    }
                                }
                                if ui
                                    .button("■ stop")
                                    .on_hover_text("End the raid and clear the sky")
                                    .clicked()
                                {
                                    raid.request = Some(Request::Stop);
                                }
                            });
                        }
                        Phase::Over => {
                            ui.label(egui::RichText::new("DATA LOST").font(stencil(14.0)).color(RED));
                            ui.label(
                                egui::RichText::new(format!(
                                    "wave {} · score {} · kills {} · best {}",
                                    raid.wave, raid.score, raid.kills, raid.best
                                ))
                                .small()
                                .monospace()
                                .color(TEXT),
                            );
                            ui.horizontal(|ui| {
                                if ui.button("⟲ replay").clicked() {
                                    raid.request = Some(Request::Replay);
                                }
                                if ui.button("■ stop").clicked() {
                                    raid.request = Some(Request::Stop);
                                }
                            });
                        }
                    }
                });
        });
}

/// Notification badges floating above units: lamp for done, chat bubble for waiting, red for error.
fn badge_style(kind: BadgeKind) -> (&'static str, egui::Color32, &'static str) {
    match kind {
        BadgeKind::Done => ("!", egui::Color32::from_rgb(255, 215, 90), "done"),
        BadgeKind::Waiting => ("💬", ACCENT, "awaiting your orders"),
        BadgeKind::Error => ("⚠", RED, "failed call"),
        BadgeKind::Approval => ("🛡", AMBER, "authorization needed"),
        BadgeKind::Stalled => ("⏳", AMBER, "stalled"),
    }
}

/// Green until two thirds, amber until ninety percent, red after.
fn usage_color(pct: f32) -> egui::Color32 {
    if pct >= 90.0 {
        RED
    } else if pct >= 66.0 {
        AMBER
    } else {
        ACCENT
    }
}

/// Open a terminal in the session's root folder: Windows Terminal if present, else PowerShell.
fn open_terminal(cwd: &str) {
    let cwd = cwd.to_string();
    std::thread::spawn(move || {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // Windows Terminal opens a tab in the folder; -NoExit keeps the shell after `cd`.
            let wt = std::process::Command::new("wt")
                .args(["-d", &cwd])
                .creation_flags(0x0800_0000)
                .spawn();
            if wt.is_ok() {
                return;
            }
            let _ = std::process::Command::new("cmd")
                .args([
                    "/C",
                    "start",
                    "powershell",
                    "-NoExit",
                    "-Command",
                    &format!("Set-Location -LiteralPath '{}'", cwd.replace('\'', "''")),
                ])
                .creation_flags(0x0800_0000)
                .spawn();
        }
        #[cfg(not(windows))]
        {
            let _ = std::process::Command::new("x-terminal-emulator")
                .current_dir(&cwd)
                .spawn()
                .or_else(|_| std::process::Command::new("gnome-terminal").current_dir(&cwd).spawn());
        }
    });
}

/// Floating label under every satellite commander: project, id, state. Click to switch.
fn draw_satellites(ctx: &egui::Context, store: &mut Store, now: i64) {
    if store.viewer.is_some() || store.satellites.is_empty() {
        return;
    }
    let vr = ctx.viewport_rect();
    let (l, t, r, b) = store.scene_insets.unwrap_or((0.0, 0.0, 0.0, 0.0));
    let scene = egui::Rect::from_min_max(
        egui::pos2(vr.min.x + l, vr.min.y + t),
        egui::pos2(vr.max.x - r, vr.max.y - b),
    );
    let mut go: Option<String> = None;
    for sat in store.satellites.clone() {
        let Some(s) = store.sessions.get(&sat.session_id) else {
            continue;
        };
        let pos = egui::pos2(sat.x, sat.y + 34.0);
        if !scene.contains(pos) {
            continue;
        }
        let root = s.root();
        let active = s
            .agents
            .iter()
            .filter(|a| a.status(now, theme::ACTIVE_WINDOW_MS) == Status::Active)
            .count();
        let in_turn = root.turn_log.last().is_some_and(|t| t.ended.is_none()) && !root.waiting_for_user;
        let (state, color) = if root.waiting_for_user {
            ("💬 awaiting orders".to_string(), ACCENT)
        } else if active > 0 {
            (format!("active · {active} unit(s) moving"), ACCENT)
        } else if in_turn {
            ("thinking…".to_string(), ACCENT)
        } else {
            ("idle".to_string(), DIM)
        };
        egui::Area::new(egui::Id::new(("satellite", &sat.session_id)))
            .fixed_pos(pos)
            .pivot(egui::Align2::CENTER_TOP)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(PANEL)
                    .stroke(egui::Stroke::new(1.0, DIM))
                    .inner_margin(6.0)
                    .corner_radius(6.0)
                    .show(ui, |ui| {
                        let title = format!("{}  {}", s.project, short(&s.id));
                        if ui
                            .add(egui::Button::new(egui::RichText::new(title).small().color(SAND)).frame(false))
                            .on_hover_text("click to switch to this session")
                            .clicked()
                        {
                            go = Some(s.id.clone());
                        }
                        ui.label(egui::RichText::new(state).small().color(color));
                    });
            });
    }
    if let Some(id) = go {
        store.pending_open = Some(id);
    }
}

/// Labels under the file nodes: name, counts on hover, click: select, click again: open in the editor.
fn draw_files(ctx: &egui::Context, store: &mut Store) {
    if store.viewer.is_some() || store.file_screen.is_empty() {
        return;
    }
    let vr = ctx.viewport_rect();
    let (l, t, r, b) = store.scene_insets.unwrap_or((0.0, 0.0, 0.0, 0.0));
    let scene = egui::Rect::from_min_max(
        egui::pos2(vr.min.x + l, vr.min.y + t),
        egui::pos2(vr.max.x - r, vr.max.y - b),
    );
    let mut open: Option<String> = None;
    let mut reveal_dir: Option<String> = None;
    // Labels that would land on one already drawn are skipped, the busiest files first: folder
    // captions, the hovered file, then by edits and reads. Sizes are last frame's.
    let mut labels = store.file_screen.clone();
    let hovered = store.hovered_file.clone();
    labels.sort_by_key(|(path, _, _, _, reads, edits)| {
        (
            !path.starts_with("dir:"),
            hovered.as_deref() != Some(path.as_str()),
            std::cmp::Reverse((*edits, *reads)),
        )
    });
    let mut taken: Vec<egui::Rect> = Vec::new();
    for (path, label, x, y, reads, edits) in labels {
        let pos = egui::pos2(x, y + 10.0);
        if !scene.contains(pos) {
            continue;
        }
        let id = if let Some(dir) = path.strip_prefix("dir:") {
            egui::Id::new(("dir", dir))
        } else {
            egui::Id::new(("file", &path))
        };
        if let Some(size) = ctx.memory(|m| m.area_rect(id)).map(|r| r.size()) {
            let rect = if path.starts_with("dir:") {
                egui::Rect::from_center_size(pos, size)
            } else {
                egui::Rect::from_min_size(pos - egui::vec2(size.x * 0.5, 0.0), size)
            };
            if taken.iter().any(|t| t.intersects(rect)) {
                continue;
            }
            taken.push(rect);
        }
        if let Some(dir) = path.strip_prefix("dir:") {
            egui::Area::new(egui::Id::new(("dir", dir)))
                .fixed_pos(pos)
                .pivot(egui::Align2::CENTER_CENTER)
                .order(egui::Order::Foreground)
                .interactable(true)
                .show(ctx, |ui| {
                    let text = egui::RichText::new(&label).font(stencil(12.0)).color(KHAKI_DIM);
                    if ui
                        .add(egui::Button::new(text).frame(false))
                        .on_hover_text(format!("{dir}\nclick: show this folder in the files sidebar"))
                        .clicked()
                    {
                        reveal_dir = Some(dir.to_string());
                    }
                });
            continue;
        }
        egui::Area::new(egui::Id::new(("file", &path)))
            .fixed_pos(pos)
            .pivot(egui::Align2::CENTER_TOP)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let color = if edits > 0 { SAND } else { DIM };
                if ui
                    .add(egui::Button::new(egui::RichText::new(&label).small().color(color)).frame(false))
                    .on_hover_text(format!(
                        "{path}\n{reads} reads · {edits} edits · click to open in the editor"
                    ))
                    .clicked()
                {
                    open = Some(path.clone());
                }
            });
    }
    if open.is_some() {
        store.pending_open_file = open;
    }
    if let Some(dir) = reveal_dir {
        store.files_open = true;
        store.files_open_dirs.insert(dir.replace('/', "\\"));
    }
}

/// Hover card for a satellite: what that session is and whether it needs you.
fn session_card(ui: &mut egui::Ui, s: &Session, now: i64) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(&s.project).font(stencil(15.0)).color(SAND));
        ui.label(egui::RichText::new(short(&s.id)).monospace().small().color(DIM));
    });
    let active = s
        .agents
        .iter()
        .filter(|a| a.status(now, theme::ACTIVE_WINDOW_MS) == Status::Active)
        .count();
    ui.label(
        egui::RichText::new(format!(
            "{} units · {active} active · {} calls · {}",
            s.agents.len(),
            s.total_calls(),
            dur(s.last_activity - s.started)
        ))
        .small()
        .monospace(),
    );
    let root = s.root();
    if root.waiting_for_user {
        let what = if root.last_said.trim().is_empty() {
            "commander is waiting for your orders".to_string()
        } else {
            snippet(&root.last_said, 140)
        };
        ui.add(egui::Label::new(egui::RichText::new(format!("💬 {what}")).small().color(ACCENT)).wrap());
    } else if let Some(c) = root
        .calls
        .iter()
        .rev()
        .find(|c| c.ended.is_none())
        .or(root.calls.last())
    {
        ui.label(
            egui::RichText::new(format!("▸ {} {}", c.tool, c.summary))
                .small()
                .color(DIM),
        );
    }
    ui.label(egui::RichText::new("click to switch").small().color(DIM));
}

fn draw_badges(ctx: &egui::Context, store: &mut Store) {
    if store.badges.is_empty() || store.viewer.is_some() {
        return;
    }
    let t = ctx.input(|i| i.time) as f32;
    let pulse = 0.55 + 0.45 * (t * 3.0).sin().abs();
    let vr = ctx.viewport_rect();
    let (l, t, r, btm) = store.scene_insets.unwrap_or((0.0, 0.0, 0.0, 0.0));
    let scene = egui::Rect::from_min_max(
        egui::pos2(vr.min.x + l, vr.min.y + t),
        egui::pos2(vr.max.x - r, vr.max.y - btm),
    );
    let painter = ctx
        .layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("badges")))
        .with_clip_rect(scene);
    let mut toggle_reply: Option<(String, String)> = None;
    let badges = store.badges.clone();
    for b in &badges {
        if !scene.contains(egui::pos2(b.x, b.y)) {
            continue;
        }
        if store
            .card_rects
            .iter()
            .any(|r| b.x >= r[0] && b.x <= r[2] && b.y >= r[1] && b.y <= r[3])
        {
            continue;
        }
        let (glyph, color, label) = badge_style(b.kind);
        let question = if b.kind == BadgeKind::Waiting {
            store
                .sessions
                .get(&b.session_id)
                .and_then(|s| s.agent(&b.agent_id))
                .map(|a| snippet(&a.last_said, 70))
                .filter(|q| !q.is_empty())
        } else {
            None
        };
        let label: &str = question.as_deref().unwrap_or(label);
        let center = egui::pos2(b.x, b.y - 10.0 - 4.0 * (t * 2.0).sin());
        if b.kind == BadgeKind::Waiting {
            let key = (b.session_id.clone(), b.agent_id.clone());
            let open = store.reply_open.as_ref() == Some(&key);
            let resp = egui::Area::new(egui::Id::new(("reply-badge", &key)))
                .fixed_pos(center - egui::vec2(16.0, 16.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    ui.allocate_response(egui::vec2(32.0, 32.0), egui::Sense::click())
                })
                .inner;
            if resp
                .on_hover_text(if open { "close the reply" } else { "read the reply" })
                .clicked()
            {
                toggle_reply = Some(key);
            }
        }
        let glow = color.gamma_multiply(0.25 * pulse);
        painter.circle_filled(center, 22.0, glow);
        painter.circle_filled(center, 14.0, PANEL);
        painter.circle_stroke(center, 14.0, egui::Stroke::new(1.5, color.gamma_multiply(pulse)));
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            glyph,
            egui::FontId::proportional(17.0),
            color,
        );
        painter.text(
            center + egui::vec2(0.0, 24.0),
            egui::Align2::CENTER_TOP,
            label,
            egui::FontId::proportional(12.0),
            color.gamma_multiply(0.9),
        );
    }
    if let Some(key) = toggle_reply {
        store.reply_open = if store.reply_open.as_ref() == Some(&key) {
            None
        } else {
            Some(key)
        };
    }
    ctx.request_repaint();
}

/// The reply card: the commander's last message, the one the editor shows as its answer, with
/// What a floating card reports back: header buttons pressed, where it ended up, and what the
/// pointer did to its title row and edges this frame.
#[derive(Default)]
pub(super) struct CardOut {
    pub close: bool,
    pub toggle_max: bool,
    pub rect: Option<egui::Rect>,
    /// The title is being dragged this frame (egui's one-frame start flag is not used: it fires
    /// before the pointer has moved far enough to tell a drag from a slow click).
    dragging: bool,
    drag_delta: egui::Vec2,
    drag_stopped: bool,
    /// An edge of a docked card being dragged: 0 left, 1 right, 2 top, 3 bottom, 4 corner.
    edge: Option<u8>,
    /// Edges this frame moved: (axis 0 = x / 1 = y, from, to) as fractions. Neighbouring panes
    /// sharing that line follow, so shrinking one pane grows the next.
    pub sash: Vec<(u8, f32, f32)>,
}

/// The dock grid. Columns follow the monitors: each monitor's free span (what is left of it
/// beside the sidebars and the agent panel) is split in two, so the seam between monitors is
/// always a grid line and a pane docked on one monitor never spills onto the next. Rows split
/// the free height evenly. Panes are fractions of the whole window, clipped to the free area
/// when drawn, so opening a panel pushes them and closing it gives the space back.
#[derive(Clone)]
pub(super) struct DockGrid {
    vp: egui::Rect,
    area: egui::Rect,
    /// Column lines as fractions of the window width, ascending; first and last are the area's edges.
    xs: Vec<f32>,
    ys: Vec<f32>,
}

impl DockGrid {
    const MIN: f32 = 0.04;

    fn new(vp: egui::Rect, area: egui::Rect, seams: &[f32], rows: usize) -> DockGrid {
        let fx = |x: f32| ((x - vp.min.x) / vp.width().max(1.0)).clamp(0.0, 1.0);
        let fy = |y: f32| ((y - vp.min.y) / vp.height().max(1.0)).clamp(0.0, 1.0);
        let mut edges = vec![area.min.x];
        for s in seams {
            if *s > area.min.x + 40.0 && *s < area.max.x - 40.0 {
                edges.push(*s);
            }
        }
        edges.push(area.max.x);
        edges.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut xs: Vec<f32> = Vec::new();
        for w in edges.windows(2) {
            let (a, b) = (w[0], w[1]);
            if b - a < 80.0 {
                continue;
            }
            xs.push(fx(a));
            xs.push(fx((a + b) * 0.5));
        }
        xs.push(fx(area.max.x));
        if xs.len() < 2 {
            xs = vec![fx(area.min.x), fx(area.max.x)];
        }
        let rows = rows.clamp(1, 6);
        let ys: Vec<f32> = (0..=rows)
            .map(|i| fy(area.min.y + area.height() * i as f32 / rows as f32))
            .collect();
        DockGrid { vp, area, xs, ys }
    }

    fn cols(&self) -> usize {
        self.xs.len() - 1
    }

    fn rows(&self) -> usize {
        self.ys.len() - 1
    }

    fn to_px(&self, p: Pane) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(
                self.vp.min.x + p.x0 * self.vp.width(),
                self.vp.min.y + p.y0 * self.vp.height(),
            ),
            egui::pos2(
                self.vp.min.x + p.x1 * self.vp.width(),
                self.vp.min.y + p.y1 * self.vp.height(),
            ),
        )
    }

    /// A pane kept inside the window and no smaller than a sliver.
    fn clamp(&self, p: Pane) -> Pane {
        let x0 = p.x0.clamp(0.0, 1.0 - Self::MIN);
        let y0 = p.y0.clamp(0.0, 1.0 - Self::MIN);
        Pane {
            x0,
            y0,
            x1: p.x1.clamp(x0 + Self::MIN, 1.0),
            y1: p.y1.clamp(y0 + Self::MIN, 1.0),
        }
    }

    /// Where a pane is drawn: its rectangle clipped to the free area, with a small margin, never
    /// smaller than a sliver (a pane pushed under a panel keeps a sliver at the area's edge).
    fn block(&self, p: Pane) -> egui::Rect {
        let r = self.to_px(self.clamp(p));
        let a = self.area;
        let min_w = 120.0_f32.min(a.width());
        let min_h = 80.0_f32.min(a.height());
        let mut x0 = r.min.x.max(a.min.x);
        let mut x1 = r.max.x.min(a.max.x);
        if x1 - x0 < min_w {
            if x0 + min_w <= a.max.x {
                x1 = x0 + min_w;
            } else {
                x1 = a.max.x;
                x0 = x1 - min_w;
            }
        }
        let mut y0 = r.min.y.max(a.min.y);
        let mut y1 = r.max.y.min(a.max.y);
        if y1 - y0 < min_h {
            if y0 + min_h <= a.max.y {
                y1 = y0 + min_h;
            } else {
                y1 = a.max.y;
                y0 = y1 - min_h;
            }
        }
        egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1)).shrink(6.0)
    }

    fn cell_rect(&self, c: usize, r: usize) -> egui::Rect {
        self.to_px(self.pane_of(c as u8, c as u8 + 1, r as u8, r as u8 + 1))
    }

    fn pane_of(&self, c0: u8, c1: u8, r0: u8, r1: u8) -> Pane {
        let cx = |i: u8| self.xs[(i as usize).min(self.xs.len() - 1)];
        let cy = |i: u8| self.ys[(i as usize).min(self.ys.len() - 1)];
        Pane {
            x0: cx(c0),
            x1: cx(c1),
            y0: cy(r0),
            y1: cy(r1),
        }
    }

    /// Every block a card can be dropped into: spans of 1, 2 and 4 cells in each direction, each
    /// aligned to its own span. A quarter, a half, a monitor, a strip across the wall, the whole
    /// wall. Their targets sit at the blocks' centres, which never coincide. Once docked, edges
    /// move freely.
    fn targets(&self) -> Vec<Pane> {
        let (cols, rows) = (self.cols() as u8, self.rows() as u8);
        let mut out: Vec<Pane> = Vec::new();
        for &w in &[1u8, 2, 4] {
            for &h in &[1u8, 2, 4] {
                for r in (0..rows).step_by(h as usize) {
                    for c in (0..cols).step_by(w as usize) {
                        if c + w <= cols && r + h <= rows {
                            let p = self.pane_of(c, c + w, r, r + h);
                            if !out.contains(&p) {
                                out.push(p);
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// How many grid cells a pane covers: the cells whose centres fall inside it.
    fn cells(&self, p: Pane) -> f32 {
        let cols = self
            .xs
            .windows(2)
            .filter(|w| {
                let c = (w[0] + w[1]) * 0.5;
                c > p.x0 && c < p.x1
            })
            .count();
        let rows = self
            .ys
            .windows(2)
            .filter(|w| {
                let c = (w[0] + w[1]) * 0.5;
                c > p.y0 && c < p.y1
            })
            .count();
        (cols * rows) as f32
    }

    fn target_rect(&self, p: Pane) -> egui::Rect {
        let cells = self.cells(p);
        let size = if cells >= 7.5 {
            egui::vec2(170.0, 94.0)
        } else if cells >= 3.5 {
            egui::vec2(150.0, 84.0)
        } else if cells >= 1.5 {
            egui::vec2(130.0, 74.0)
        } else {
            egui::vec2(110.0, 64.0)
        };
        egui::Rect::from_center_size(self.block(p).center(), size)
    }

    fn target_at(&self, p: egui::Pos2) -> Option<Pane> {
        self.targets().into_iter().find(|d| self.target_rect(*d).contains(p))
    }

    /// A pointer x as a fraction of the window, pulled onto a grid line when within a few pixels.
    fn snap_x(&self, x: f32) -> f32 {
        let f = ((x - self.vp.min.x) / self.vp.width().max(1.0)).clamp(0.0, 1.0);
        let px = |g: f32| self.vp.min.x + g * self.vp.width();
        self.xs
            .iter()
            .copied()
            .find(|g| (px(*g) - x).abs() <= 12.0)
            .unwrap_or(f)
    }

    fn snap_y(&self, y: f32) -> f32 {
        let f = ((y - self.vp.min.y) / self.vp.height().max(1.0)).clamp(0.0, 1.0);
        let py = |g: f32| self.vp.min.y + g * self.vp.height();
        self.ys
            .iter()
            .copied()
            .find(|g| (py(*g) - y).abs() <= 12.0)
            .unwrap_or(f)
    }
}

/// Take a card's state out of the store and settings for the frame (so the card's closures can
/// borrow those freely), and the grid it docks into.
pub(super) fn begin_card(
    store: &mut Store,
    settings: &mut Settings,
    id: &str,
    ctx: &egui::Context,
) -> (CardState, Option<Pane>, DockGrid) {
    let cs = store.cards.remove(id).unwrap_or_default();
    let dock = settings.panes.remove(id);
    // The dock area is the free space: right of both left sidebars, left of the agent panel,
    // under the top bar, above the status bar. Panes are fractions of it, so they follow when a
    // panel opens or closes.
    let vr = ctx.viewport_rect();
    let (left, top, right, bottom) = store.scene_insets.unwrap_or((0.0, 0.0, 0.0, 0.0));
    let mut area = egui::Rect::from_min_max(
        egui::pos2(vr.min.x + left, vr.min.y + top),
        egui::pos2(vr.max.x - right, vr.max.y - bottom),
    );
    if area.width() < 240.0 || area.height() < 160.0 {
        area = vr;
    }
    let grid = DockGrid::new(vr, area, &store.monitor_seams, settings.grid_rows as usize);
    (cs, dock, grid)
}

pub(super) fn end_card(
    store: &mut Store,
    settings: &mut Settings,
    id: &str,
    cs: CardState,
    dock: Option<Pane>,
    sash: &[(u8, f32, f32)],
) {
    store.cards.insert(id.to_string(), cs);
    // A moved edge drags every other docked edge on the same line with it: a shared sash, as
    // between editor groups. Panes never drop below a sliver.
    for (axis, from, to) in sash {
        for (k, p) in settings.panes.iter_mut() {
            if k == id {
                continue;
            }
            *p = sash_follow(*p, *axis, *from, *to);
        }
    }
    if let Some(d) = dock {
        settings.panes.insert(id.to_string(), d);
    }
}

/// `p` with any edge lying on `from` (axis 0: x, 1: y) moved to `to`, kept at least a sliver wide.
fn sash_follow(p: Pane, axis: u8, from: f32, to: f32) -> Pane {
    let eps = 0.004;
    let min = DockGrid::MIN;
    let mut q = p;
    if axis == 0 {
        if (p.x0 - from).abs() <= eps {
            q.x0 = to.min(q.x1 - min).max(0.0);
        }
        if (p.x1 - from).abs() <= eps {
            q.x1 = to.max(q.x0 + min).min(1.0);
        }
    } else {
        if (p.y0 - from).abs() <= eps {
            q.y0 = to.min(q.y1 - min).max(0.0);
        }
        if (p.y1 - from).abs() <= eps {
            q.y1 = to.max(q.y0 + min).min(1.0);
        }
    }
    q
}

/// While a title is dragged: the grid, faint, and a target for every block a card can fill
/// (quarter, half, whole monitor); the one under the pointer lights up. Dropping anywhere else
/// leaves the card floating where it is.
fn draw_dock_zones(ctx: &egui::Context, grid: &DockGrid, pointer: egui::Pos2) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("dock-zones")));
    let hot = grid.target_at(pointer);
    for r in 0..grid.rows() {
        for c in 0..grid.cols() {
            let cell = grid.cell_rect(c, r).shrink(3.0);
            painter.rect_stroke(
                cell,
                4.0,
                egui::Stroke::new(1.0, ACCENT.gamma_multiply(0.35)),
                egui::StrokeKind::Inside,
            );
        }
    }
    if let Some(d) = hot {
        painter.rect_filled(grid.block(d), 6.0, ACCENT.gamma_multiply(0.12));
        painter.rect_stroke(
            grid.block(d),
            6.0,
            egui::Stroke::new(1.5, ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    for d in grid.targets() {
        let t = grid.target_rect(d);
        let lit = hot == Some(d);
        let cells = grid.cells(d).round() as u32;
        let label = match cells {
            1 => "¼",
            2 => "½",
            4 => "1",
            8 => "2",
            _ => "·",
        };
        painter.rect_filled(
            t,
            6.0,
            if lit {
                ACCENT.gamma_multiply(0.75)
            } else {
                egui::Color32::from_rgba_unmultiplied(6, 12, 5, 190)
            },
        );
        painter.rect_stroke(
            t,
            6.0,
            egui::Stroke::new(1.5, if lit { SAND } else { ACCENT }),
            egui::StrokeKind::Inside,
        );
        painter.text(
            t.center(),
            egui::Align2::CENTER_CENTER,
            if lit { "dock here" } else { label },
            egui::FontId::proportional(if lit { 14.0 } else { 22.0 }),
            if lit { INK } else { SAND },
        );
    }
}

/// Run `add` in a child that cannot grow `ui`: what is wider or taller than the room left is
/// clipped, and `ui` advances by that room at most. egui containers grow to fit their content and
/// never shrink below it, so without this one long line (or a terminal grid sized to its last,
/// bigger block) made a card or the agent panel spring back after every resize.
pub(super) fn clipped<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let rect = ui.available_rect_before_wrap();
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("clipped")
            .max_rect(rect)
            .layout(*ui.layout()),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    let r = add(&mut child);
    let used = child.min_rect().height().min(rect.height());
    ui.advance_cursor_after_rect(egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), used)));
    r
}

/// The chrome shared by every card (reply, terminal, turn, settings): a Tooltip-order area above
/// the badges and labels, solid frame, a title row that drags the card (drop it on a dock target
/// to dock it, drag it again to undock), ✕ and ⛶, an optional search box (Ctrl+F) whose
/// lowercase text the body receives, a corner to resize when floating, edges that snap to the
/// grid when docked, or pinned to fill the screen.
#[allow(clippy::too_many_arguments)]
pub(super) fn floating_card(
    ctx: &egui::Context,
    id: &str,
    maxed: bool,
    cs: &mut CardState,
    dock: &mut Option<Pane>,
    grid: DockGrid,
    default_pos: egui::Pos2,
    default_size: egui::Vec2,
    query: Option<&mut String>,
    header: impl FnOnce(&mut egui::Ui),
    body: impl FnOnce(&mut egui::Ui, &str),
) -> CardOut {
    let vr = ctx.viewport_rect();
    let margin = 16.0;
    let pad = 24.0; // the frame's inner margin, both sides
    let max_size = vr.size() - egui::vec2(2.0 * margin + pad, 2.0 * margin + pad);
    let docked: Option<egui::Rect> = if maxed { None } else { dock.map(|d| grid.block(d)) };
    let floating_pos = cs.pos.map(|p| egui::pos2(p[0], p[1])).unwrap_or(default_pos);
    let (pos, fixed_size) = if maxed {
        (vr.min + egui::vec2(margin, margin), Some(max_size))
    } else if let Some(r) = docked {
        (r.min, Some(r.size()))
    } else {
        (floating_pos, None)
    };
    let mut out = CardOut::default();
    let find =
        ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) && ctx.memory(|m| m.focused().is_none());
    let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new(id));
    let can_drag = !maxed;
    let zoom = cs.zoom;
    let content = |ui: &mut egui::Ui, out: &mut CardOut| {
        if (zoom - 1.0).abs() > 0.01 {
            for f in ui.style_mut().text_styles.values_mut() {
                f.size *= zoom;
            }
        }
        // The title row is the drag handle, declared first so the buttons drawn over it win clicks.
        let row_h = 24.0;
        let header_rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), row_h));
        if can_drag {
            let drag = ui
                .interact(header_rect, egui::Id::new(id).with("drag"), egui::Sense::drag())
                .on_hover_cursor(egui::CursorIcon::Grab);
            out.dragging = drag.dragged();
            out.drag_delta = drag.drag_delta();
            out.drag_stopped = drag.drag_stopped();
        }
        let mut q_lower = String::new();
        ui.horizontal(|ui| {
            ui.set_min_height(row_h);
            // Title text does not select, so a drag anywhere on this row moves the card.
            ui.style_mut().interaction.selectable_labels = false;
            // Buttons first, from the right; the title gets what is left and truncates in it.
            // Laid out the other way round, a long title pushed the buttons past the card's edge
            // and widened the room the body measured (egui grows a layout to what it holds).
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✕  (Esc)").clicked() {
                    out.close = true;
                }
                if ui
                    .small_button(if maxed { "🗗 restore" } else { "⛶ full" })
                    .on_hover_text(if maxed {
                        "Back to the card"
                    } else {
                        "Fill the screen. Floating: drag the corner to resize. Docked: drag an edge."
                    })
                    .clicked()
                {
                    out.toggle_max = true;
                }
                if let Some(q) = query {
                    if !q.is_empty() && ui.small_button("⌫").on_hover_text("Clear the search").clicked() {
                        q.clear();
                    }
                    let resp = ui.add(
                        egui::TextEdit::singleline(q)
                            .id(egui::Id::new((id, "search")))
                            .desired_width(180.0)
                            .font(egui::TextStyle::Monospace)
                            .hint_text("🔍 search  (Ctrl+F)"),
                    );
                    if find {
                        resp.request_focus();
                    }
                    q_lower = q.trim().to_ascii_lowercase();
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), header);
            });
        });
        ui.separator();
        body(ui, &q_lower);
    };
    let resp = egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos)
        .movable(false)
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(egui::Color32::from_rgb(6, 12, 5))
                .stroke(egui::Stroke::new(1.5, if cs.dragging { SAND } else { ACCENT }))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 4],
                    blur: 18,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(180),
                })
                .inner_margin(12.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    if let Some(size) = fixed_size {
                        ui.set_width(size.x - pad);
                        ui.set_height(size.y - pad);
                        clipped(ui, |ui| content(ui, &mut out));
                    } else {
                        egui::Resize::default()
                            .id_salt((id, "size"))
                            .default_size(default_size)
                            .min_size(egui::vec2(320.0, 160.0))
                            .max_size(max_size)
                            .show(ui, |ui| clipped(ui, |ui| content(ui, &mut out)));
                    }
                    // Edges of a docked card: drag to grow or shrink by whole cells.
                    if docked.is_some() && !maxed {
                        let r = ui.min_rect().expand(12.0);
                        let t = 8.0;
                        let strips = [
                            (
                                0u8,
                                egui::Rect::from_min_max(r.left_top(), egui::pos2(r.min.x + t, r.max.y)),
                                egui::CursorIcon::ResizeHorizontal,
                            ),
                            (
                                1u8,
                                egui::Rect::from_min_max(egui::pos2(r.max.x - t, r.min.y), r.right_bottom()),
                                egui::CursorIcon::ResizeHorizontal,
                            ),
                            (
                                2u8,
                                egui::Rect::from_min_max(r.left_top(), egui::pos2(r.max.x, r.min.y + t)),
                                egui::CursorIcon::ResizeVertical,
                            ),
                            (
                                3u8,
                                egui::Rect::from_min_max(egui::pos2(r.min.x, r.max.y - t), r.right_bottom()),
                                egui::CursorIcon::ResizeVertical,
                            ),
                            (
                                4u8,
                                egui::Rect::from_min_max(
                                    egui::pos2(r.max.x - 2.0 * t, r.max.y - 2.0 * t),
                                    r.right_bottom(),
                                ),
                                egui::CursorIcon::ResizeNwSe,
                            ),
                        ];
                        for (side, strip, cursor) in strips {
                            let e = ui
                                .interact(strip, egui::Id::new(id).with(("edge", side)), egui::Sense::drag())
                                .on_hover_cursor(cursor);
                            if e.dragged() {
                                out.edge = Some(side);
                            }
                        }
                    }
                });
        });
    out.rect = Some(resp.response.rect);
    let pointer = ctx.input(|i| i.pointer.latest_pos()).unwrap_or(pos);

    // ---- the title drag: float it, move it, dock it on a target. A press that has not moved
    // is a click on a header button (egui would call a long press a drag), so motion is required.
    let moved = ctx.input(|i| {
        i.pointer
            .press_origin()
            .zip(i.pointer.latest_pos())
            .is_some_and(|(a, b)| a.distance(b) > 6.0)
    });
    if out.dragging && !cs.dragging && moved {
        ctx.move_to_top(layer);
        cs.dragging = true;
        if dock.is_some() {
            *dock = None;
            cs.pos = Some([pointer.x - default_size.x * 0.5, pointer.y - 12.0]);
        } else if cs.pos.is_none() {
            cs.pos = Some([pos.x, pos.y]);
        }
    }
    if cs.dragging {
        if let Some(p) = cs.pos.as_mut() {
            p[0] = (p[0] + out.drag_delta.x).clamp(vr.min.x, vr.max.x - 160.0);
            p[1] = (p[1] + out.drag_delta.y).clamp(vr.min.y, vr.max.y - 48.0);
        }
        draw_dock_zones(ctx, &grid, pointer);
        let released = out.drag_stopped || !ctx.input(|i| i.pointer.primary_down());
        if released {
            cs.dragging = false;
            if let Some(d) = grid.target_at(pointer) {
                *dock = Some(d);
            }
        }
    }

    // ---- edges (and the corner) of a docked card move freely, snapping near grid lines
    if let (Some(side), Some(d)) = (out.edge, dock.as_mut()) {
        let cur = grid.clamp(*d);
        let (fx, fy) = (grid.snap_x(pointer.x), grid.snap_y(pointer.y));
        let min = DockGrid::MIN;
        let next = match side {
            0 => Pane {
                x0: fx.min(cur.x1 - min),
                ..cur
            },
            1 => Pane {
                x1: fx.max(cur.x0 + min),
                ..cur
            },
            2 => Pane {
                y0: fy.min(cur.y1 - min),
                ..cur
            },
            3 => Pane {
                y1: fy.max(cur.y0 + min),
                ..cur
            },
            _ => Pane {
                x1: fx.max(cur.x0 + min),
                y1: fy.max(cur.y0 + min),
                ..cur
            },
        };
        for (axis, from, to) in [
            (0u8, cur.x0, next.x0),
            (0, cur.x1, next.x1),
            (1, cur.y0, next.y0),
            (1, cur.y1, next.y1),
        ] {
            if (from - to).abs() > 1e-6 {
                out.sash.push((axis, from, to));
            }
        }
        *d = next;
    }
    out
}

/// Search mode for a block of text: only the lines that match, numbered, with the hits lit.
fn search_lines(ui: &mut egui::Ui, text: &str, q_lower: &str, mono: bool) {
    let font = if mono {
        egui::FontId::monospace(13.0)
    } else {
        egui::FontId::proportional(14.0)
    };
    let mut n = 0;
    for (ln, line) in text.lines().enumerate() {
        if !contains_ci(line, q_lower) {
            continue;
        }
        n += 1;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(format!("{:>4}", ln + 1))
                    .small()
                    .monospace()
                    .color(DIM),
            );
            ui.add(egui::Label::new(highlighted(line, q_lower, font.clone(), TEXT)).wrap());
        });
    }
    if n == 0 {
        ui.label(egui::RichText::new("no matches").color(DIM));
    }
}

/// The reply card: the commander's last message, the one the editor shows as its answer, with
/// light Markdown. Opened from the 💬 badge; Esc, ✕ or the badge again closes it.
fn draw_reply(ctx: &egui::Context, store: &mut Store, settings: &mut Settings) -> Option<egui::Rect> {
    let (sid, aid) = store.reply_open.clone()?;
    let Some(agent) = store.sessions.get(&sid).and_then(|s| s.agent(&aid)).cloned() else {
        store.reply_open = None;
        return None;
    };
    let project = store.sessions.get(&sid).map(|s| s.project.clone()).unwrap_or_default();
    let anchor = store
        .badges
        .iter()
        .find(|b| b.session_id == sid && b.agent_id == aid)
        .map(|b| egui::pos2(b.x + 28.0, b.y - 20.0));
    let vr = ctx.viewport_rect();
    let width = (vr.width() * 0.38).clamp(380.0, 640.0);
    let height = (vr.height() * 0.45).clamp(220.0, 520.0);
    let default_pos = anchor.unwrap_or_else(|| vr.center() - egui::vec2(width / 2.0, height / 2.0));
    let maxed = store.reply_max;
    let (mut cs, mut dock, grid) = begin_card(store, settings, "reply-card", ctx);
    let mut query = std::mem::take(&mut store.reply_query);
    let out = floating_card(
        ctx,
        "reply-card",
        maxed,
        &mut cs,
        &mut dock,
        grid,
        default_pos,
        egui::vec2(width, height),
        Some(&mut query),
        |ui| {
            ui.label(egui::RichText::new("💬").size(16.0));
            ui.label(
                egui::RichText::new(format!("{project} · {}", agent_title(&agent)))
                    .font(stencil(14.0))
                    .color(SAND),
            );
            ui.label(
                egui::RichText::new(clock(agent.last_activity))
                    .small()
                    .monospace()
                    .color(DIM),
            );
            if ui
                .small_button("⧉ copy")
                .on_hover_text("Copy the whole reply")
                .clicked()
            {
                ctx.copy_text(agent.last_said.clone());
            }
        },
        |ui, q| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_min_width(ui.available_width() - 8.0);
                if agent.last_said.trim().is_empty() {
                    ui.label(egui::RichText::new("nothing said yet").color(DIM));
                } else if q.is_empty() {
                    render_reply(ui, &agent.last_said);
                } else {
                    search_lines(ui, &agent.last_said, q, false);
                }
            });
        },
    );
    store.reply_query = query;
    end_card(store, settings, "reply-card", cs, dock, &out.sash);
    if out.toggle_max {
        store.reply_max = !store.reply_max;
    }
    if out.close {
        store.reply_open = None;
    }
    out.rect
}

/// The integrated terminal: real consoles (ConPTY) in the session's folder, in the same card as
/// the reply. Click the grid and keys go straight to the shell, so PSReadLine, Claude Code and
/// full-screen programs work. The card's search box switches to a text view of the scrollback
/// with the hits lit. Ctrl+T / Ctrl+W open and close tabs while nothing has the keyboard.
fn draw_terminal(
    ctx: &egui::Context,
    store: &mut Store,
    settings: &mut Settings,
    terms: &mut Terminals,
) -> Option<egui::Rect> {
    let sid = store.terminal_open.clone()?;
    let Some((project, cwd)) = store.sessions.get(&sid).map(|s| (s.project.clone(), s.cwd.clone())) else {
        store.terminal_open = None;
        return None;
    };
    let vr = ctx.viewport_rect();
    let width = (vr.width() * 0.55).clamp(520.0, 1100.0);
    let height = (vr.height() * 0.5).clamp(280.0, 640.0);
    let default_pos = egui::pos2(
        vr.center().x - width / 2.0,
        (vr.max.y - height - 56.0).max(vr.min.y + 56.0),
    );
    let maxed = store.terminal_max;
    let font_px = store.term_font;
    let (mut cs, mut dock, grid) = begin_card(store, settings, "terminal-card", ctx);
    let mut query = std::mem::take(&mut store.terminal_query);
    let spawn_error = terms.error.clone();
    let mut restart = false;
    let mut clear = false;
    let mut copy = false;
    let mut new_tab = false;
    let mut close_tab: Option<usize> = None;
    let mut switch_to: Option<usize> = None;
    // Reconnect: a command for a fresh tab (the first tab, when this opens the consoles).
    let run = store.term_run.take();
    let had_tabs = terms.has(&sid);
    let free = ctx.memory(|m| m.focused().is_none());
    let (ctrl_t, ctrl_w) = ctx.input(|i| {
        (
            free && i.modifiers.command && i.key_pressed(egui::Key::T),
            free && i.modifiers.command && i.key_pressed(egui::Key::W),
        )
    });
    let out = match terms.set_mut(&sid, &cwd) {
        None => floating_card(
            ctx,
            "terminal-card",
            maxed,
            &mut cs,
            &mut dock,
            grid,
            default_pos,
            egui::vec2(width, height),
            None,
            |ui| {
                ui.label(egui::RichText::new("⌨").size(16.0));
                ui.label(
                    egui::RichText::new(format!("{project} · terminal"))
                        .font(stencil(14.0))
                        .color(SAND),
                );
            },
            |ui, _q| {
                ui.label(egui::RichText::new(spawn_error.unwrap_or_default()).color(RED));
            },
        ),
        Some(set) => {
            let tabs = set.titles();
            let active = set.active;
            let tab = set.active_mut();
            if let Some(text) = tab.take_clipboard() {
                ctx.copy_text(text);
            }
            let dead = tab.is_dead();
            let title = tab.title().unwrap_or_default();
            floating_card(
                ctx,
                "terminal-card",
                maxed,
                &mut cs,
                &mut dock,
                grid,
                default_pos,
                egui::vec2(width, height),
                Some(&mut query),
                |ui| {
                    ui.label(egui::RichText::new("⌨").size(16.0));
                    ui.label(egui::RichText::new(&project).font(stencil(14.0)).color(SAND));
                    ui.add(egui::Label::new(egui::RichText::new(&title).small().monospace().color(DIM)).truncate());
                    if dead {
                        ui.label(egui::RichText::new("exited").small().color(RED));
                    }
                    if ui
                        .small_button("⟲")
                        .on_hover_text("Restart this tab's shell (ends whatever is running)")
                        .clicked()
                    {
                        restart = true;
                    }
                    if ui.small_button("🗑").on_hover_text("cls").clicked() {
                        clear = true;
                    }
                    if ui
                        .small_button("⧉")
                        .on_hover_text("Copy the whole scrollback")
                        .clicked()
                    {
                        copy = true;
                    }
                },
                |ui, q| {
                    ui.horizontal(|ui| {
                        ui.style_mut().interaction.selectable_labels = false;
                        for (i, (label, title, tab_dead)) in tabs.iter().enumerate() {
                            let text = if *tab_dead {
                                format!("{label}: {title} ✗")
                            } else {
                                format!("{label}: {title}")
                            };
                            let r = ui.selectable_label(i == active, egui::RichText::new(text).monospace().small());
                            if r.clicked() {
                                switch_to = Some(i);
                            }
                            if r.middle_clicked() {
                                close_tab = Some(i);
                            }
                            if i == active && ui.small_button("✕").on_hover_text("Close this tab (Ctrl+W)").clicked()
                            {
                                close_tab = Some(i);
                            }
                        }
                        if ui.small_button("+").on_hover_text("New tab (Ctrl+T)").clicked() {
                            new_tab = true;
                        }
                    });
                    if q.is_empty() {
                        draw_grid(ui, tab, font_px);
                    } else {
                        let lines = tab.all_lines();
                        egui::ScrollArea::vertical()
                            .id_salt("terminal-search")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width() - 8.0);
                                let mut shown = 0;
                                for (n, l) in lines.iter().enumerate() {
                                    if !contains_ci(l, q) {
                                        continue;
                                    }
                                    shown += 1;
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!("{:>5}", n + 1))
                                                .small()
                                                .monospace()
                                                .color(DIM),
                                        );
                                        ui.add(
                                            egui::Label::new(highlighted(l, q, egui::FontId::monospace(13.0), TEXT))
                                                .selectable(true)
                                                .wrap(),
                                        );
                                    });
                                }
                                if shown == 0 {
                                    ui.label(egui::RichText::new("no matches").color(DIM));
                                }
                            });
                    }
                },
            )
        }
    };
    let mut emptied = false;
    if let Some(set) = terms.set_mut(&sid, &cwd) {
        if let Some(i) = switch_to {
            set.active = i.min(set.tabs.len().saturating_sub(1));
        }
        if ctrl_t || new_tab {
            let here = set.active_mut().cwd.clone();
            let _ = set.open(&here);
        }
        if let Some(cmd) = run {
            let opened = !had_tabs || set.open(&cwd).is_ok();
            if opened {
                set.active_mut().write(format!("{cmd}\r").into_bytes());
            }
        }
        if let Some(i) = close_tab.or(if ctrl_w { Some(set.active) } else { None }) {
            set.close(i);
        }
        if set.is_empty() {
            emptied = true;
        } else {
            if restart {
                set.restart_active();
            }
            let tab = set.active_mut();
            if clear {
                tab.write(b"cls\r".to_vec());
            }
            if copy {
                ctx.copy_text(tab.all_lines().join("\n"));
            }
        }
    }
    if emptied {
        terms.remove(&sid);
        store.terminal_open = None;
    }
    store.terminal_query = query;
    end_card(store, settings, "terminal-card", cs, dock, &out.sash);
    if out.toggle_max {
        store.terminal_max = !store.terminal_max;
    }
    if out.close {
        store.terminal_open = None;
    }
    out.rect
}

fn rgb(c: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

/// The console grid: sized to the space it gets (and the shell told), painted cell by cell,
/// keys and paste forwarded while it has the keyboard, the wheel scrolling the history.
fn draw_grid(ui: &mut egui::Ui, tab: &mut terminal::PtyTab, font_px: f32) {
    let t0 = std::time::Instant::now();
    draw_grid_inner(ui, tab, font_px);
    if std::env::var("AGENT_GLOBE_TERM_LOG").is_ok() {
        let us = t0.elapsed().as_micros();
        let key = egui::Id::new("term-timing");
        let (n, sum, max) = ui.data_mut(|d| d.get_temp::<(u32, u128, u128)>(key).unwrap_or((0, 0, 0)));
        let (n, sum, max) = (n + 1, sum + us, max.max(us));
        if n >= 120 {
            bevy::log::info!(
                "terminal grid: avg {} us, max {} us over {n} frames",
                sum / n as u128,
                max
            );
            ui.data_mut(|d| d.insert_temp(key, (0u32, 0u128, 0u128)));
        } else {
            ui.data_mut(|d| d.insert_temp(key, (n, sum, max)));
        }
    }
}

fn draw_grid_inner(ui: &mut egui::Ui, tab: &mut terminal::PtyTab, font_px: f32) {
    let font = egui::FontId::monospace(font_px);
    let (cw, ch) = ui.fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
    let avail = ui.available_size();
    let (rect, resp) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
    let cols = ((rect.width() - 6.0) / cw).floor().clamp(10.0, 400.0) as u16;
    let rows = ((rect.height() - 6.0) / ch).floor().clamp(3.0, 200.0) as u16;
    tab.ensure_size(cols, rows);
    if resp.clicked() || resp.drag_started() {
        resp.request_focus();
    }
    // A press anywhere else (another card, a panel, the scene) hands the keyboard back. Selecting
    // text in another card takes no egui focus, so without this the console kept it, and the
    // Ctrl+C meant to copy that text interrupted whatever ran here.
    let press = ui.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten());
    let pressed_elsewhere = press.is_some_and(|p| !rect.contains(p) || ui.ctx().layer_id_at(p) != Some(ui.layer_id()));
    if pressed_elsewhere && resp.has_focus() {
        resp.surrender_focus();
    }
    let focused = resp.has_focus();
    // Mouse selection: drag selects cells; a plain click clears; right-click copies.
    let origin = rect.min + egui::vec2(3.0, 3.0);
    let cell_at = |p: egui::Pos2| -> (usize, usize) {
        let c = ((p.x - origin.x) / cw).floor().clamp(0.0, cols as f32 - 1.0) as usize;
        let r = ((p.y - origin.y) / ch).floor().clamp(0.0, rows as f32 - 1.0) as usize;
        (r, c)
    };
    if resp.drag_started()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let (r, c) = cell_at(p);
        tab.sel = Some((r, c, r, c));
    } else if resp.dragged()
        && let Some(p) = resp.interact_pointer_pos()
        && let Some(sel) = tab.sel.as_mut()
    {
        let (r, c) = cell_at(p);
        sel.2 = r;
        sel.3 = c;
    } else if resp.clicked() {
        tab.sel = None;
    }
    let mut copy_selection = resp.secondary_clicked();
    if focused {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                resp.id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            )
        });
    }
    if resp.hovered() {
        // Wheel notches in lines, with the fraction carried between frames so slow wheels still move.
        let lines: f32 = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::MouseWheel { unit, delta, .. } => Some(match unit {
                        egui::MouseWheelUnit::Line => delta.y * 3.0,
                        egui::MouseWheelUnit::Point => delta.y / ch,
                        egui::MouseWheelUnit::Page => delta.y * rows as f32,
                    }),
                    _ => None,
                })
                .sum()
        });
        if lines != 0.0 {
            let key = resp.id.with("wheel-rest");
            let total = ui.data_mut(|d| d.get_temp::<f32>(key).unwrap_or(0.0)) + lines;
            let whole = total.trunc();
            ui.data_mut(|d| d.insert_temp(key, total - whole));
            let (col, row) = ui
                .input(|i| i.pointer.hover_pos())
                .map(|p| {
                    let o = rect.min + egui::vec2(3.0, 3.0);
                    (
                        ((p.x - o.x) / cw).max(0.0) as usize,
                        ((p.y - o.y) / ch).max(0.0) as usize,
                    )
                })
                .unwrap_or((0, 0));
            tab.wheel(whole as i32, col, row);
        }
    }
    let frame = tab.frame();
    if focused {
        let (events, mods) = ui.input(|i| (i.events.clone(), i.modifiers));
        // With a selection, Ctrl+C copies instead of interrupting; Ctrl+Shift+C always copies.
        let had_sel = tab.sel.is_some();
        for e in events {
            match e {
                egui::Event::Key {
                    key: egui::Key::C,
                    pressed: true,
                    modifiers,
                    ..
                } if (modifiers.command || modifiers.ctrl) && (modifiers.shift || had_sel) => {
                    copy_selection = true;
                }
                egui::Event::Copy if had_sel => {}
                egui::Event::Text(t) => {
                    if mods.command || mods.ctrl {
                        tab.paste(&t);
                    } else {
                        tab.write(t.into_bytes());
                    }
                    tab.scroll_bottom();
                }
                egui::Event::Paste(t) => {
                    tab.paste(&t);
                    tab.scroll_bottom();
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if let Some(bytes) = terminal::key_bytes(key, modifiers, frame.app_cursor) {
                        tab.write(bytes);
                        tab.scroll_bottom();
                    }
                }
                _ => {}
            }
        }
    }
    if copy_selection {
        let text = tab.selected_text(&frame);
        if !text.is_empty() {
            ui.ctx().copy_text(text);
        }
        tab.sel = None;
    } else if focused && tab.sel.is_some() && ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(_)))) {
        // Typing drops the selection.
        tab.sel = None;
    }
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 4.0, rgb(terminal::BG));
    let sel_range = tab.sel.map(|(r0, c0, r1, c1)| {
        if (r0, c0) <= (r1, c1) {
            ((r0, c0), (r1, c1))
        } else {
            ((r1, c1), (r0, c0))
        }
    });
    for (r, row) in frame.cells.iter().enumerate() {
        let y = origin.y + r as f32 * ch;
        for (c, px) in row.iter().enumerate() {
            let cell = egui::Rect::from_min_size(egui::pos2(origin.x + c as f32 * cw, y), egui::vec2(cw, ch));
            if let Some(bg) = px.bg {
                painter.rect_filled(cell, 0.0, rgb(bg));
            }
            if let Some((start, end)) = sel_range
                && (r, c) >= start
                && (r, c) <= end
            {
                painter.rect_filled(cell, 0.0, egui::Color32::from_rgba_unmultiplied(120, 220, 160, 70));
            }
        }
        // Runs of one style, each placed by its column so wide glyphs never shift the rest.
        let mut run = String::new();
        let mut run_start = 0usize;
        let mut run_style: Option<([u8; 3], bool, bool, bool)> = None;
        let flush = |run: &mut String,
                     start: usize,
                     style: Option<([u8; 3], bool, bool, bool)>,
                     painter: &egui::Painter,
                     ui: &egui::Ui| {
            if run.trim().is_empty() {
                run.clear();
                return;
            }
            let Some((fg, bold, dim, underline)) = style else {
                run.clear();
                return;
            };
            let mut color = rgb(fg);
            if bold {
                color = egui::Color32::from_rgb(
                    fg[0].saturating_add(40),
                    fg[1].saturating_add(40),
                    fg[2].saturating_add(40),
                );
            }
            if dim {
                color = color.gamma_multiply(0.6);
            }
            let x = origin.x + start as f32 * cw;
            let job = egui::text::LayoutJob::simple_singleline(std::mem::take(run), font.clone(), color);
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            painter.galley(egui::pos2(x, y), galley, color);
            if underline {
                let w = run.chars().count().max(1) as f32 * cw;
                painter.hline(x..=x + w, y + ch - 1.5, egui::Stroke::new(1.0, color));
            }
        };
        for (c, px) in row.iter().enumerate() {
            if px.spacer {
                continue;
            }
            let style = (px.fg, px.bold, px.dim, px.underline);
            if run_style != Some(style) {
                flush(&mut run, run_start, run_style, &painter, ui);
                run_style = Some(style);
                run_start = c;
            }
            if run.is_empty() {
                run_start = c;
            }
            run.push(px.c);
            if px.wide {
                flush(&mut run, run_start, run_style, &painter, ui);
            }
        }
        flush(&mut run, run_start, run_style, &painter, ui);
    }
    if let Some((r, c, shape)) = frame.cursor {
        use alacritty_terminal::vte::ansi::CursorShape;
        let x = origin.x + c as f32 * cw;
        let y = origin.y + r as f32 * ch;
        let cell = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cw, ch));
        let col = rgb(terminal::FG);
        match shape {
            CursorShape::Block | CursorShape::HollowBlock => {
                if focused {
                    painter.rect_filled(cell, 0.0, col.gamma_multiply(0.85));
                    let under = frame
                        .cells
                        .get(r)
                        .and_then(|row| row.get(c))
                        .map(|p| p.c)
                        .unwrap_or(' ');
                    painter.text(
                        cell.left_top(),
                        egui::Align2::LEFT_TOP,
                        under,
                        font.clone(),
                        rgb(terminal::BG),
                    );
                } else {
                    painter.rect_stroke(cell, 0.0, egui::Stroke::new(1.0, col), egui::StrokeKind::Inside);
                }
            }
            CursorShape::Underline => {
                painter.rect_filled(
                    egui::Rect::from_min_max(egui::pos2(x, y + ch - 2.0), egui::pos2(x + cw, y + ch)),
                    0.0,
                    col,
                );
            }
            CursorShape::Beam => {
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(2.0, ch)),
                    0.0,
                    col,
                );
            }
            CursorShape::Hidden => {}
        }
    }
    if frame.history > 0 {
        // A thin scrollbar on the right: where the screen sits in the history.
        let total = (frame.history + rows as usize) as f32;
        let track = egui::Rect::from_min_max(
            egui::pos2(rect.max.x - 5.0, rect.min.y + 3.0),
            egui::pos2(rect.max.x - 2.0, rect.max.y - 3.0),
        );
        let thumb_h = (track.height() * rows as f32 / total).max(12.0);
        let top_frac = (frame.history - frame.display_offset) as f32 / total;
        let thumb = egui::Rect::from_min_size(
            egui::pos2(
                track.min.x,
                track.min.y + top_frac * (track.height() - thumb_h) * total / (total - rows as f32).max(1.0),
            ),
            egui::vec2(track.width(), thumb_h),
        );
        painter.rect_filled(track, 1.5, egui::Color32::from_white_alpha(10));
        painter.rect_filled(thumb.intersect(track), 1.5, egui::Color32::from_white_alpha(60));
    }
    if frame.display_offset > 0 {
        let text = format!("▲ {} lines up · wheel or any key", frame.display_offset);
        let pos = rect.right_top() + egui::vec2(-8.0, 6.0);
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(11.0), SAND);
        let r = egui::Rect::from_min_size(
            pos - egui::vec2(galley.size().x + 8.0, 0.0),
            galley.size() + egui::vec2(8.0, 4.0),
        );
        painter.rect_filled(r, 3.0, egui::Color32::from_black_alpha(200));
        painter.galley(r.min + egui::vec2(4.0, 2.0), galley, SAND);
    }
    if !focused {
        painter.text(
            rect.right_bottom() - egui::vec2(6.0, 4.0),
            egui::Align2::RIGHT_BOTTOM,
            "click to type",
            egui::FontId::proportional(11.0),
            DIM,
        );
    }
    if tab.take_bell() {
        painter.rect_stroke(rect, 4.0, egui::Stroke::new(2.0, AMBER), egui::StrokeKind::Outside);
    }
}

/// A prompt group as a card, opened by double-clicking its header: the conversation (your prompt
/// and the commander's reply) and the details (that turn's tool calls, with the narrator lines).
/// The search box lights hits in whichever segment is showing.
fn draw_turn_card(
    ctx: &egui::Context,
    store: &mut Store,
    settings: &mut Settings,
    narrator: bool,
) -> Option<egui::Rect> {
    let (sid, aid, ti) = store.turn_open.clone()?;
    let Some(agent) = store.sessions.get(&sid).and_then(|s| s.agent(&aid)).cloned() else {
        store.turn_open = None;
        return None;
    };
    let Some(turn) = agent.turn_log.get(ti).cloned() else {
        store.turn_open = None;
        return None;
    };
    let project = store.sessions.get(&sid).map(|s| s.project.clone()).unwrap_or_default();
    let range = agent.turn_range(ti);
    let stalled = store.stalled.clone();
    let mut expanded = std::mem::take(&mut store.expanded_calls);
    let mut query = std::mem::take(&mut store.turn_query);
    let mut tab = store.turn_tab;
    let maxed = store.turn_max;
    let (mut cs, mut dock, grid) = begin_card(store, settings, "turn-card", ctx);
    let vr = ctx.viewport_rect();
    let width = (vr.width() * 0.5).clamp(480.0, 960.0);
    let height = (vr.height() * 0.6).clamp(300.0, 720.0);
    let default_pos = vr.center() - egui::vec2(width / 2.0, height / 2.0);
    let first_line: String = turn
        .prompt
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .chars()
        .take(70)
        .collect();
    let out = floating_card(
        ctx,
        "turn-card",
        maxed,
        &mut cs,
        &mut dock,
        grid,
        default_pos,
        egui::vec2(width, height),
        Some(&mut query),
        |ui| {
            ui.label(
                egui::RichText::new(format!("#{}", ti + 1))
                    .font(stencil(14.0))
                    .color(ACCENT),
            );
            ui.label(
                egui::RichText::new(format!("{project} · {}", agent_title(&agent)))
                    .font(stencil(14.0))
                    .color(SAND),
            );
            ui.label(egui::RichText::new(clock(turn.started)).small().monospace().color(DIM));
            ui.add(egui::Label::new(egui::RichText::new(&first_line).color(TEXT)).truncate());
        },
        |ui, q| {
            ui.horizontal(|ui| {
                if ui.selectable_label(tab == 0, "💬 conversation").clicked() {
                    tab = 0;
                }
                let calls = format!(
                    "⚙ details · {} call{}",
                    range.len(),
                    if range.len() == 1 { "" } else { "s" }
                );
                if ui.selectable_label(tab == 1, calls).clicked() {
                    tab = 1;
                }
            });
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .id_salt("turn-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width() - 8.0);
                    if tab == 0 {
                        ui.label(egui::RichText::new("YOU").small().color(DIM));
                        egui::Frame::NONE
                            .fill(INK)
                            .inner_margin(8.0)
                            .corner_radius(4.0)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                if q.is_empty() {
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(&turn.prompt).color(TEXT))
                                            .selectable(true)
                                            .wrap(),
                                    );
                                } else {
                                    search_lines(ui, &turn.prompt, q, false);
                                }
                            });
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("COMMANDER").small().color(DIM));
                        egui::Frame::NONE
                            .fill(INK)
                            .inner_margin(8.0)
                            .corner_radius(4.0)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                if turn.reply.trim().is_empty() {
                                    ui.label(egui::RichText::new("no reply yet").color(DIM));
                                } else if q.is_empty() {
                                    render_reply(ui, &turn.reply);
                                } else {
                                    search_lines(ui, &turn.reply, q, false);
                                }
                            });
                    } else {
                        let mut shown = 0;
                        for i in range.clone() {
                            let c = &agent.calls[i];
                            if !q.is_empty() && !call_matches(c, q) {
                                continue;
                            }
                            shown += 1;
                            call_row(ui, i, c, &stalled, narrator, &mut expanded, q);
                        }
                        if shown == 0 {
                            let msg = if range.is_empty() {
                                "no tool calls in this turn"
                            } else {
                                "no matches"
                            };
                            ui.label(egui::RichText::new(msg).color(DIM));
                        }
                    }
                });
        },
    );
    store.expanded_calls = expanded;
    store.turn_query = query;
    store.turn_tab = tab;
    end_card(store, settings, "turn-card", cs, dock, &out.sash);
    if out.toggle_max {
        store.turn_max = !store.turn_max;
    }
    if out.close {
        store.turn_open = None;
    }
    out.rect
}

/// Light Markdown for replies: headings, bullets, numbered lists, fenced code, inline code, bold.
fn render_reply(ui: &mut egui::Ui, text: &str) {
    let mut in_code = false;
    let mut code = String::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            if in_code {
                egui::Frame::NONE.fill(INK).inner_margin(6.0).show(ui, |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(code.trim_end_matches('\n')).monospace().color(TEXT))
                            .selectable(true),
                    );
                });
                code.clear();
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            code.push_str(line);
            code.push('\n');
            continue;
        }
        let t = line.trim_end();
        if t.is_empty() {
            ui.add_space(4.0);
        } else if let Some(h) = t
            .strip_prefix("### ")
            .or_else(|| t.strip_prefix("## "))
            .or_else(|| t.strip_prefix("# "))
        {
            ui.label(egui::RichText::new(h.trim()).font(stencil(14.0)).color(SAND));
        } else if let Some(item) = t
            .trim_start()
            .strip_prefix("- ")
            .or_else(|| t.trim_start().strip_prefix("* "))
        {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("▸").color(SAND));
                inline_reply(ui, item);
            });
        } else if t.trim_start().chars().next().is_some_and(|c| c.is_ascii_digit()) && t.trim_start().contains(". ") {
            let (num, rest) = t.trim_start().split_once(". ").unwrap_or(("", t));
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(format!("{num}.")).color(SAND));
                inline_reply(ui, rest);
            });
        } else if t.starts_with('|') {
            ui.label(egui::RichText::new(t).monospace().small().color(TEXT));
        } else {
            ui.horizontal_wrapped(|ui| inline_reply(ui, t));
        }
    }
    if in_code && !code.is_empty() {
        egui::Frame::NONE.fill(INK).inner_margin(6.0).show(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new(code.trim_end()).monospace().color(TEXT)).selectable(true));
        });
    }
}

/// Inline `code` and **bold** inside one line.
fn inline_reply(ui: &mut egui::Ui, text: &str) {
    ui.spacing_mut().item_spacing.x = 0.0;
    let mut rest = text;
    while !rest.is_empty() {
        let code_at = rest.find('`');
        let bold_at = rest.find("**");
        match (code_at, bold_at) {
            (Some(c), b) if b.is_none_or(|b| c <= b) => {
                if c > 0 {
                    ui.label(egui::RichText::new(&rest[..c]).color(TEXT));
                }
                if let Some(end) = rest[c + 1..].find('`') {
                    ui.label(egui::RichText::new(&rest[c + 1..c + 1 + end]).monospace().color(ACCENT));
                    rest = &rest[c + 2 + end..];
                } else {
                    ui.label(egui::RichText::new(&rest[c..]).color(TEXT));
                    rest = "";
                }
            }
            (_, Some(b)) => {
                if b > 0 {
                    ui.label(egui::RichText::new(&rest[..b]).color(TEXT));
                }
                if let Some(end) = rest[b + 2..].find("**") {
                    ui.label(egui::RichText::new(&rest[b + 2..b + 2 + end]).strong().color(SAND));
                    rest = &rest[b + 4 + end..];
                } else {
                    ui.label(egui::RichText::new(&rest[b..]).color(TEXT));
                    rest = "";
                }
            }
            _ => {
                ui.label(egui::RichText::new(rest).color(TEXT));
                rest = "";
            }
        }
    }
}

fn draw_toasts(ctx: &egui::Context, store: &Store) {
    if store.toasts.is_empty() {
        return;
    }
    egui::Area::new(egui::Id::new("toasts"))
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -40.0))
        .interactable(false)
        .show(ctx, |ui| {
            for t in store.toasts.iter().rev() {
                let alpha = (1.0 - t.born.elapsed().as_secs_f32() / 7.0).clamp(0.0, 1.0);
                let color = match t.kind {
                    ToastKind::Spawn => ACCENT,
                    ToastKind::Done => GREEN,
                    ToastKind::Error => RED,
                    ToastKind::Warn => AMBER,
                    ToastKind::Info => DIM,
                };
                egui::Frame::NONE
                    .fill(PANEL.gamma_multiply(alpha))
                    .stroke(egui::Stroke::new(1.0, color.gamma_multiply(alpha)))
                    .inner_margin(8.0)
                    .corner_radius(6.0)
                    .show(ui, |ui| {
                        ui.set_max_width(420.0);
                        ui.label(
                            egui::RichText::new(&t.text)
                                .color(color.gamma_multiply(alpha))
                                .monospace(),
                        );
                    });
                ui.add_space(4.0);
            }
        });
}

/// Monospace block capped at `MAX_LINES` with a per-block expand toggle, to keep the panel quiet.
/// Byte ranges of every non-overlapping, case-insensitive (ASCII) occurrence of `needle_lower`.
fn match_ranges(hay: &str, needle_lower: &str) -> Vec<(usize, usize)> {
    let n = needle_lower.as_bytes();
    if n.is_empty() || hay.len() < n.len() {
        return Vec::new();
    }
    let h = hay.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + n.len() <= h.len() {
        if hay.is_char_boundary(i) && h[i..i + n.len()].eq_ignore_ascii_case(n) && hay.is_char_boundary(i + n.len()) {
            out.push((i, i + n.len()));
            i += n.len();
        } else {
            i += 1;
        }
    }
    out
}

/// Text with every search hit boxed in the accent colour, ready for a label.
fn highlighted(text: &str, needle_lower: &str, font: egui::FontId, color: egui::Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let plain = egui::TextFormat {
        font_id: font.clone(),
        color,
        ..Default::default()
    };
    let hit = egui::TextFormat {
        font_id: font,
        color: INK,
        background: ACCENT,
        ..Default::default()
    };
    let mut last = 0;
    for (a, b) in match_ranges(text, needle_lower) {
        if a > last {
            job.append(&text[last..a], 0.0, plain.clone());
        }
        job.append(&text[a..b], 0.0, hit.clone());
        last = b;
    }
    if last < text.len() {
        job.append(&text[last..], 0.0, plain);
    }
    job
}

/// A text label; with a search active the hits are highlighted.
fn text_label(text: &str, needle_lower: &str, font: egui::FontId, color: egui::Color32) -> egui::Label {
    if needle_lower.is_empty() || match_ranges(text, needle_lower).is_empty() {
        egui::Label::new(egui::RichText::new(text).font(font).color(color))
    } else {
        egui::Label::new(highlighted(text, needle_lower, font, color))
    }
}

/// Which lines of a capped block to show: with a search active, the lines around the hits
/// (joined with "⋯" where lines were skipped); otherwise the first `max` lines.
fn preview_lines(lines: &[&str], needle_lower: &str, max: usize) -> String {
    if lines.len() <= max {
        return lines.join("\n");
    }
    if needle_lower.is_empty() {
        return lines[..max].join("\n");
    }
    let hits: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| !match_ranges(l, needle_lower).is_empty())
        .map(|(i, _)| i)
        .collect();
    if hits.is_empty() {
        return lines[..max].join("\n");
    }
    let mut keep: Vec<usize> = Vec::new();
    for h in hits {
        for i in h.saturating_sub(1)..=(h + 1).min(lines.len() - 1) {
            if keep.last().is_none_or(|&k| k < i) {
                keep.push(i);
            }
        }
        if keep.len() >= max {
            break;
        }
    }
    keep.truncate(max);
    let mut out = String::new();
    let mut prev: Option<usize> = None;
    for i in keep {
        if prev.is_some_and(|p| p + 1 < i) {
            out.push_str("⋯\n");
        }
        out.push_str(lines[i]);
        out.push('\n');
        prev = Some(i);
    }
    out.trim_end_matches('\n').to_string()
}

fn code_block(
    ui: &mut egui::Ui,
    key: &str,
    text: &str,
    color: egui::Color32,
    expanded: &mut HashSet<String>,
    needle: &str,
) {
    let lines: Vec<&str> = text.lines().collect();
    let is_open = expanded.contains(key);
    let shown = if lines.len() > MAX_LINES && !is_open {
        preview_lines(&lines, needle, MAX_LINES)
    } else {
        text.to_string()
    };
    ui.add(
        text_label(&shown, needle, egui::FontId::monospace(14.0), color)
            .wrap()
            .selectable(true),
    );
    if lines.len() > MAX_LINES {
        let label = if is_open {
            "▴ collapse".to_string()
        } else {
            format!("▾ {} more lines", lines.len() - MAX_LINES)
        };
        if ui.small_button(egui::RichText::new(label).small()).clicked() {
            if is_open {
                expanded.remove(key);
            } else {
                expanded.insert(key.to_string());
            }
        }
    }
}

/// One tool call in the history list: time, status mark, header, narrator line, command and output.
/// A prompt group header with an absolute id: the arrow and the title toggle it on click, fold /
/// unfold set it from outside, and a double-click or right-click is reported back (it opens the
/// turn card).
fn group_header(
    ui: &mut egui::Ui,
    id: egui::Id,
    title: egui::WidgetText,
    default_open: bool,
    fold: Option<bool>,
    body: impl FnOnce(&mut egui::Ui),
) -> bool {
    let mut st = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, default_open);
    if let Some(open) = fold {
        st.set_open(open);
    }
    let openness = st.openness(ui.ctx());
    let row = ui.horizontal(|ui| {
        ui.style_mut().interaction.selectable_labels = false;
        let (_, icon) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        egui::collapsing_header::paint_default_icon(ui, openness, &icon);
        ui.add(egui::Label::new(title).truncate());
    });
    let hit = ui.interact(row.response.rect, id.with("row"), egui::Sense::click());
    // A double-click or a right-click opens the group as a card; a click folds or unfolds it.
    let double = hit.double_clicked() || hit.secondary_clicked();
    if hit.clicked() && !double {
        st.toggle(ui);
    }
    if hit.hovered() {
        ui.painter().rect_stroke(
            row.response.rect.expand(1.0),
            3.0,
            egui::Stroke::new(1.0, DIM),
            egui::StrokeKind::Outside,
        );
    }
    st.show_body_indented(&row.response, ui, body);
    st.store(ui.ctx());
    double
}

fn call_row(
    ui: &mut egui::Ui,
    i: usize,
    c: &agent_globe::data::model::ToolCall,
    stalled: &HashSet<String>,
    narrator: bool,
    expanded: &mut HashSet<String>,
    needle: &str,
) {
    let open = c.ended.is_none();
    let mark = if c.is_error {
        egui::RichText::new("✖").color(RED)
    } else if open && stalled.contains(&c.id) {
        egui::RichText::new("⏳").color(AMBER)
    } else if open {
        egui::RichText::new("●").color(ACCENT)
    } else {
        egui::RichText::new("✔").color(GREEN)
    };
    let took = c.ended.map(|e| dur(e - c.started)).unwrap_or_else(|| "running".into());
    let header = format!("{:>3}  {:<10} {}", i + 1, c.tool, c.summary);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(clock(c.started)).small().monospace().color(DIM));
        ui.label(mark);
        ui.add(text_label(&header, needle, egui::FontId::monospace(14.0), TEXT).truncate());
        ui.label(egui::RichText::new(took).small().color(DIM));
    });
    if narrator && !c.note.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(26.0);
            let note = format!("“{}”", c.note);
            ui.add(text_label(&note, needle, egui::FontId::proportional(12.0), SAND).wrap());
        });
    }
    if let Some(cmd) = &c.command {
        egui::Frame::NONE
            .fill(INK)
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(50, 90, 35)))
            .inner_margin(6.0)
            .show(ui, |ui| {
                let line = format!("$ {cmd}");
                ui.add(text_label(&line, needle, egui::FontId::monospace(14.0), ACCENT).wrap());
                if !c.stdout.is_empty() {
                    code_block(ui, &c.id, &c.stdout, TEXT, expanded, needle);
                }
                if !c.stderr.is_empty() {
                    code_block(ui, &format!("{}:err", c.id), &c.stderr, RED, expanded, needle);
                }
            });
    } else if !c.stdout.is_empty() {
        egui::Frame::NONE.fill(INK).inner_margin(6.0).show(ui, |ui| {
            code_block(ui, &c.id, &c.stdout, DIM, expanded, needle);
        });
    }
    ui.add_space(4.0);
}

/// Case-insensitive substring test without allocating a lowercase copy of large outputs.
fn contains_ci(hay: &str, needle_lower: &str) -> bool {
    let n = needle_lower.as_bytes();
    if n.is_empty() {
        return true;
    }
    hay.as_bytes().windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

/// Does a tool call mention the search text anywhere: tool name, summary, command, narrator note, output?
fn call_matches(c: &agent_globe::data::model::ToolCall, needle_lower: &str) -> bool {
    contains_ci(&c.tool, needle_lower)
        || contains_ci(&c.summary, needle_lower)
        || c.command.as_deref().is_some_and(|x| contains_ci(x, needle_lower))
        || contains_ci(&c.note, needle_lower)
        || contains_ci(&c.stdout, needle_lower)
        || contains_ci(&c.stderr, needle_lower)
}

fn status_label(a: &Agent, now: i64, stalled: &HashSet<String>) -> (&'static str, egui::Color32) {
    if a.calls.iter().any(|c| c.ended.is_none() && stalled.contains(&c.id)) {
        return ("STALLED", AMBER);
    }
    match a.status(now, theme::ACTIVE_WINDOW_MS) {
        Status::Active => ("ACTIVE", ACCENT),
        Status::Idle => ("IDLE", DIM),
        Status::Done => ("DONE", GREEN),
    }
}

fn agent_title(a: &Agent) -> String {
    if a.depth == 0 {
        "commander · root session".to_string()
    } else if a.name.is_empty() {
        format!("unit {}", short(&a.id))
    } else {
        a.name.clone()
    }
}

#[allow(clippy::too_many_arguments)]
fn agent_panel(
    ui: &mut egui::Ui,
    a: &Agent,
    now: i64,
    expanded: &mut HashSet<String>,
    jump_to_end: bool,
    narrator: bool,
    stalled: &HashSet<String>,
    probe: &Probe,
    git: Option<&GitInfo>,
    images: &mut HashMap<String, Option<egui::TextureHandle>>,
    view_image: &mut Option<String>,
    query: &mut String,
    changes: &[FileChange],
    cwd: &str,
    touched: &HashMap<String, usize>,
    history: &mut String,
    fold_all: &mut Option<bool>,
    servers: &Snapshot,
    kill_armed: &mut Option<(u32, std::time::Instant)>,
    docker_armed: &mut Option<(String, std::time::Instant)>,
    kill_request: &mut Option<u32>,
    docker_request: &mut Option<(String, String, String)>,
    open_turn: &mut Option<usize>,
) {
    let (label, color) = status_label(a, now, stalled);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(agent_title(a)).font(stencil(18.0)).color(ACCENT));
        ui.label(egui::RichText::new(label).color(color).strong());
    });
    ui.horizontal_wrapped(|ui| {
        let meta = |ui: &mut egui::Ui, k: &str, v: String, help: &str| {
            ui.label(egui::RichText::new(k).small().color(DIM)).on_hover_text(help);
            ui.label(egui::RichText::new(v).small().monospace());
        };
        meta(ui, "id", short(&a.id).to_string(), "Agent id from the transcript.");
        meta(
            ui,
            "kind",
            if a.kind.is_empty() { "-".into() } else { a.kind.clone() },
            "fork: a copy of the commander's context.\nExplore / general-purpose: a fresh agent with only its brief.",
        );
        meta(
            ui,
            "model",
            if a.model.is_empty() {
                "-".into()
            } else {
                a.model.clone()
            },
            "Model that answered.",
        );
        meta(
            ui,
            "depth",
            a.depth.to_string(),
            "0 is the commander; each ring out is one more level of delegation.",
        );
        let end = a.ended.unwrap_or(a.last_activity);
        meta(
            ui,
            "duration",
            dur(end - a.started),
            "First to last event of this agent.",
        );
        meta(ui, "calls", a.calls.len().to_string(), "Tool calls this agent made.");
        meta(
            ui,
            "tokens out",
            a.output_tokens.to_string(),
            "Tokens the model generated (what you pay most for).",
        );
        if a.context_tokens > 0 {
            meta(
                ui,
                "ctx",
                format!("{}k", a.context_tokens / 1000),
                "Size of the last request this agent sent: everything it currently remembers.",
            );
        }
    });
    let open_calls = a.calls.iter().filter(|c| c.ended.is_none()).count();
    let done_frac = if a.calls.is_empty() {
        0.0
    } else {
        1.0 - open_calls as f32 / a.calls.len() as f32
    };
    let fill = if open_calls > 0 {
        egui::Color32::from_rgb(70, 130, 45)
    } else {
        egui::Color32::from_rgb(40, 80, 30)
    };
    ui.add(
        egui::ProgressBar::new(done_frac)
            .fill(fill)
            .text(egui::RichText::new(format!("{open_calls} open")).small()),
    );
    if !a.artifacts.is_empty() {
        ui.add_space(4.0);
        let key = format!("{}:artifacts", a.id);
        let folded = dedup_artifacts(&a.artifacts);
        let total = folded.len();
        let row_h = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
        let header = egui::RichText::new(format!(
            "ARTIFACTS  ·  {total} item{}",
            if total == 1 { "" } else { "s" }
        ))
        .small()
        .color(DIM);
        egui::CollapsingHeader::new(header)
            .id_salt((&a.id, "artifacts"))
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("🔍").small());
                    ui.add(
                        egui::TextEdit::singleline(query)
                            .hint_text("search artifacts")
                            .desired_width(220.0),
                    );
                    if !query.is_empty() && ui.small_button("✕").clicked() {
                        query.clear();
                    }
                });
                let needle = query.to_ascii_lowercase();
                egui::ScrollArea::vertical()
                    .id_salt(&key)
                    .max_height(row_h * 6.0 + 4.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for (art, count) in folded.iter().filter(|(x, _)| {
                                needle.is_empty()
                                    || x.label.to_ascii_lowercase().contains(&needle)
                                    || x.target.to_ascii_lowercase().contains(&needle)
                            }) {
                                let alive = if art.kind == ArtifactKind::Url {
                                    probe.is_alive(&art.target)
                                } else {
                                    None
                                };
                                if artifact_chip(ui, art, alive, images, row_h - 6.0, *count).clicked()
                                    && let Some(path) = open_artifact(ui.ctx(), art, git)
                                {
                                    *view_image = Some(path);
                                }
                            }
                        });
                    });
            });
    }
    if a.depth == 0 && (!servers.servers.is_empty() || !servers.procs.is_empty()) {
        ui.add_space(4.0);
        let srv_header = egui::RichText::new(format!(
            "SERVERS & PROCESSES  ·  {} server{}  {} process{}",
            servers.servers.len(),
            if servers.servers.len() == 1 { "" } else { "s" },
            servers.procs.len(),
            if servers.procs.len() == 1 { "" } else { "es" }
        ))
        .small()
        .color(DIM);
        egui::CollapsingHeader::new(srv_header)
            .id_salt((&a.id, "servers"))
            .default_open(true)
            .show(ui, |ui| {
                let armed_for = |pid: u32, armed: &Option<(u32, std::time::Instant)>| {
                    armed.is_some_and(|(p, at)| p == pid && at.elapsed().as_secs_f32() < 3.0)
                };
                let armed_for_str = |id: &str, armed: &Option<(String, std::time::Instant)>| {
                    armed
                        .as_ref()
                        .is_some_and(|(p, at)| p == id && at.elapsed().as_secs_f32() < 3.0)
                };
                let kill_button = |ui: &mut egui::Ui,
                                   pid: u32,
                                   kill_armed: &mut Option<(u32, std::time::Instant)>,
                                   kill_request: &mut Option<u32>| {
                    let armed = armed_for(pid, kill_armed);
                    let label = if armed {
                        egui::RichText::new("kill?").color(AMBER).strong()
                    } else {
                        egui::RichText::new("✕").color(RED)
                    };
                    if ui
                        .small_button(label)
                        .on_hover_text(if armed {
                            "Click again to end this process tree"
                        } else {
                            "End this process (asks once more)"
                        })
                        .clicked()
                    {
                        if armed {
                            *kill_request = Some(pid);
                            *kill_armed = None;
                        } else {
                            *kill_armed = Some((pid, std::time::Instant::now()));
                        }
                    }
                };
                for srv in &servers.servers {
                    ui.horizontal(|ui| {
                        let (dot, color) = if srv.alive { ("●", ACCENT) } else { ("○", DIM) };
                        ui.label(egui::RichText::new(dot).color(color));
                        if let Some(t) = &srv.title {
                            ui.label(egui::RichText::new(t).color(SAND).strong());
                        }
                        let link = ui
                            .add(
                                egui::Button::new(egui::RichText::new(&srv.url).monospace().color(if srv.alive {
                                    ACCENT
                                } else {
                                    DIM
                                }))
                                .frame(false),
                            )
                            .on_hover_text(if srv.alive {
                                "open in the browser"
                            } else {
                                "not listening right now"
                            });
                        if link.clicked() {
                            let u = srv.url.clone();
                            std::thread::spawn(move || {
                                let _ = open::that(&u);
                            });
                        }
                        match srv.pid {
                            Some(_) if srv.container.is_some() => {
                                let c = srv.container.as_ref().unwrap();
                                let (dotc, state) = if c.running {
                                    (ACCENT, "running")
                                } else {
                                    (DIM, "stopped")
                                };
                                ui.label(
                                    egui::RichText::new(format!("· 🐳 {} ({state})", c.name))
                                        .small()
                                        .color(dotc),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let armed = armed_for_str(&c.id, docker_armed);
                                    if armed {
                                        if ui
                                            .small_button(egui::RichText::new("kill?").color(AMBER).strong())
                                            .on_hover_text("Click again to kill the container")
                                            .clicked()
                                        {
                                            *docker_request = Some(("kill".into(), c.id.clone(), c.name.clone()));
                                            *docker_armed = None;
                                        }
                                    } else if ui
                                        .small_button(egui::RichText::new("⛔").color(RED))
                                        .on_hover_text("Kill the container (asks once more)")
                                        .clicked()
                                    {
                                        *docker_armed = Some((c.id.clone(), std::time::Instant::now()));
                                    }
                                    if c.running {
                                        if ui.small_button("⏸").on_hover_text("docker stop (graceful)").clicked() {
                                            *docker_request = Some(("stop".into(), c.id.clone(), c.name.clone()));
                                        }
                                    } else if ui.small_button("▶").on_hover_text("docker start").clicked() {
                                        *docker_request = Some(("start".into(), c.id.clone(), c.name.clone()));
                                    }
                                });
                            }
                            Some(pid) if srv.protected => {
                                let lower = srv.name.to_ascii_lowercase();
                                let owner = if lower.contains("docker") || lower.contains("wsl") {
                                    "docker".to_string()
                                } else {
                                    srv.name.clone()
                                };
                                ui.label(egui::RichText::new(format!("· {owner} (pid {pid})")).small().color(DIM))
                            .on_hover_text(
                                "Owned by Docker, the system or Agent Globe itself: stop it from there, not here.",
                            );
                            }
                            Some(pid) => {
                                ui.label(
                                    egui::RichText::new(format!("· {} (pid {pid})", srv.name))
                                        .small()
                                        .color(DIM),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    kill_button(ui, pid, kill_armed, kill_request);
                                });
                            }
                            None => {
                                ui.label(egui::RichText::new("· not listening").small().color(DIM));
                            }
                        }
                    });
                }
                if !servers.procs.is_empty() {
                    let header = egui::RichText::new(format!(
                        "⚙ {} background process{}",
                        servers.procs.len(),
                        if servers.procs.len() == 1 { "" } else { "es" }
                    ))
                    .small()
                    .color(SAND);
                    egui::CollapsingHeader::new(header)
                        .id_salt((&a.id, "procs"))
                        .default_open(servers.procs.len() <= 2)
                        .show(ui, |ui| {
                            for p in &servers.procs {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{} (pid {})", p.name, p.pid))
                                            .monospace()
                                            .color(TEXT),
                                    );
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(&p.cmdline).small().color(DIM)).truncate(),
                                    )
                                    .on_hover_text(format!("started by: {}", p.from));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        kill_button(ui, p.pid, kill_armed, kill_request);
                                    });
                                });
                            }
                        });
                }
            });
    }
    if !changes.is_empty() {
        ui.add_space(4.0);
        let key = format!("{}:changes", a.id);
        let (add, del) = changes.iter().fold((0, 0), |(x, y), c| (x + c.added, y + c.deleted));
        let header = egui::RichText::new(format!(
            "CHANGES  ·  {} file{}  +{add} −{del}",
            changes.len(),
            if changes.len() == 1 { "" } else { "s" }
        ))
        .small()
        .color(DIM);
        let cr = egui::CollapsingHeader::new(header)
            .id_salt((&a.id, "changes"))
            .default_open(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(&key)
                    .max_height(360.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for c in changes {
                            ui.horizontal(|ui| {
                                let stat = if c.untracked {
                                    "new".to_string()
                                } else {
                                    format!("+{} −{}", c.added, c.deleted)
                                };
                                let stat_color = if c.untracked { SAND } else { GREEN };
                                ui.label(
                                    egui::RichText::new(format!("{stat:>9}"))
                                        .monospace()
                                        .small()
                                        .color(stat_color),
                                );
                                if ui
                                    .add(
                                        egui::Button::new(egui::RichText::new(&c.path).monospace().small())
                                            .frame(false)
                                            .truncate(),
                                    )
                                    .on_hover_text("open in the editor")
                                    .clicked()
                                {
                                    open_in_editor(cwd, &c.path);
                                }
                                if let Some(n) = touched.get(&c.path) {
                                    ui.label(egui::RichText::new(format!("✎ {n}")).small().color(DIM))
                                        .on_hover_text("edits by this session");
                                }
                            });
                        }
                    });
            });
        cr.header_response.on_hover_text(
            "Files the working tree changed against the last commit. Click one to open it in the editor; ✎ counts edits this session made to it.",
        );
    }
    ui.separator();

    // History search sits at the bottom of the panel, like the artifact search in the shelf.
    let needle = history.trim().to_ascii_lowercase();
    let hits: Vec<usize> = a
        .calls
        .iter()
        .enumerate()
        .filter(|(_, c)| needle.is_empty() || call_matches(c, &needle))
        .map(|(i, _)| i)
        .collect();
    egui::Panel::bottom(egui::Id::new(("history-search", &a.id))).show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("🔍").small());
            let w = (ui.available_width() - 150.0).max(120.0);
            ui.add(
                egui::TextEdit::singleline(history)
                    .hint_text("search this unit's history: tool, command, output")
                    .desired_width(w),
            );
            if !history.is_empty() && ui.small_button("✕").clicked() {
                history.clear();
            }
            if !needle.is_empty() {
                ui.label(
                    egui::RichText::new(format!("{} / {}", hits.len(), a.calls.len()))
                        .small()
                        .monospace()
                        .color(if hits.is_empty() { AMBER } else { DIM }),
                );
            }
        });
        ui.add_space(2.0);
    });

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(needle.is_empty())
        .show(ui, |ui| {
            if !a.prompt.is_empty() && needle.is_empty() && a.turn_log.is_empty() {
                ui.collapsing(egui::RichText::new("orders (prompt)").color(SAND), |ui| {
                    code_block(ui, &format!("{}:prompt", a.id), &a.prompt, TEXT, expanded, "");
                });
            }
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("TOOL CALLS").small().color(DIM));
                if !a.turn_log.is_empty() {
                    if ui
                        .small_button(egui::RichText::new("▴ fold all").small())
                        .on_hover_text("Collapse every prompt group: a list of turns to jump between")
                        .clicked()
                    {
                        *fold_all = Some(false);
                    }
                    if ui
                        .small_button(egui::RichText::new("▾ unfold").small())
                        .on_hover_text("Open every prompt group")
                        .clicked()
                    {
                        *fold_all = Some(true);
                    }
                }
            });
            // Fold / unfold: every group below applies it to its own state this frame. Groups use
            // absolute ids (not ui-relative ones), so the state can be driven from anywhere.
            let fold = *fold_all;
            if !needle.is_empty() && hits.is_empty() {
                ui.label(
                    egui::RichText::new(format!("nothing in this unit's history matches \"{}\"", history.trim()))
                        .color(DIM),
                );
            }
            if a.turn_log.is_empty() {
                for i in hits.iter().copied() {
                    call_row(ui, i, &a.calls[i], stalled, narrator, expanded, &needle);
                }
            } else {
                // Grouped by your prompts: each turn folds its calls, units and time.
                let hit_set: HashSet<usize> = hits.iter().copied().collect();
                let n = a.turn_log.len();
                let lead: Vec<usize> = (0..a.turn_log[0].first_call.min(a.calls.len()))
                    .filter(|i| hit_set.contains(i))
                    .collect();
                if !lead.is_empty() {
                    let title = egui::RichText::new(format!("before the first prompt · {} calls", lead.len()))
                        .small()
                        .color(DIM);
                    group_header(
                        ui,
                        egui::Id::new(("turn-group", &a.id, "lead")),
                        title.into(),
                        false,
                        fold,
                        |ui| {
                            for i in lead {
                                call_row(ui, i, &a.calls[i], stalled, narrator, expanded, &needle);
                            }
                        },
                    );
                }
                for ti in 0..n {
                    let t = &a.turn_log[ti];
                    let range = a.turn_range(ti);
                    let idx: Vec<usize> = range.clone().filter(|i| hit_set.contains(i)).collect();
                    let searching = !needle.is_empty();
                    let reply_hit = searching && t.reply.to_lowercase().contains(&needle.to_lowercase());
                    let matched = searching && (!idx.is_empty() || reply_hit);
                    let first_line: String = t
                        .prompt
                        .lines()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("")
                        .trim()
                        .chars()
                        .take(90)
                        .collect();
                    let took = t.ended.unwrap_or(a.last_activity) - t.started;
                    let units = a.calls[range.clone()]
                        .iter()
                        .filter(|c| c.tool == "Agent" || c.tool == "Task")
                        .count();
                    // With a search active: matching groups become a lit band with their hit count,
                    // the others stay visible but dim and folded, so the eye lands on the hits.
                    let title = if matched {
                        format!(
                            "#{}  {}  {}   · {} call{} match",
                            ti + 1,
                            clock(t.started),
                            first_line,
                            idx.len(),
                            if idx.len() == 1 { "" } else { "s" }
                        )
                    } else {
                        format!("#{}  {}  {}", ti + 1, clock(t.started), first_line)
                    };
                    let header: egui::WidgetText = if matched {
                        highlighted(
                            &title,
                            &needle,
                            egui::FontId::proportional(14.0),
                            egui::Color32::from_rgb(230, 255, 220),
                        )
                        .into()
                    } else if searching {
                        egui::RichText::new(title).color(DIM).into()
                    } else {
                        egui::RichText::new(title).color(SAND).into()
                    };
                    let band = if matched {
                        egui::Frame::NONE
                            .fill(egui::Color32::from_rgb(18, 58, 28))
                            .stroke(egui::Stroke::new(1.0, ACCENT))
                            .inner_margin(egui::Margin::symmetric(6, 2))
                            .corner_radius(4.0)
                    } else {
                        egui::Frame::NONE
                    };
                    band.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let gid = egui::Id::new(("turn-group", &a.id, ti));
                        let default_open = (ti + 1 == n && !searching) || matched;
                        let double = group_header(ui, gid, header, default_open, fold, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} calls · {} units · {}",
                                        range.len(),
                                        units,
                                        dur(took)
                                    ))
                                    .small()
                                    .monospace()
                                    .color(DIM),
                                );
                                if t.ended.is_none() {
                                    ui.label(egui::RichText::new("● in progress").small().color(ACCENT));
                                }
                            });
                            if t.prompt.trim().len() > first_line.len() {
                                ui.collapsing(egui::RichText::new("full prompt").small().color(DIM), |ui| {
                                    code_block(ui, &format!("{}:turn{ti}", a.id), &t.prompt, TEXT, expanded, "");
                                });
                            }
                            if searching && !matched {
                                ui.label(egui::RichText::new("no matches in this turn").small().color(DIM));
                            }
                            for i in idx {
                                call_row(ui, i, &a.calls[i], stalled, narrator, expanded, &needle);
                            }
                            if !t.reply.trim().is_empty() {
                                let reply_first: String = t
                                    .reply
                                    .lines()
                                    .find(|l| !l.trim().is_empty())
                                    .unwrap_or("")
                                    .trim()
                                    .chars()
                                    .take(80)
                                    .collect();
                                let head = egui::RichText::new(format!("💬 {reply_first}")).color(if reply_hit {
                                    egui::Color32::from_rgb(230, 255, 220)
                                } else {
                                    SAND
                                });
                                egui::CollapsingHeader::new(head)
                                    .id_salt((&a.id, ti, "reply"))
                                    .default_open(range.is_empty() || ti + 1 == n || reply_hit)
                                    .show(ui, |ui| {
                                        egui::Frame::NONE.fill(INK).inner_margin(8.0).corner_radius(4.0).show(
                                            ui,
                                            |ui| {
                                                ui.set_width(ui.available_width());
                                                render_reply(ui, &t.reply);
                                            },
                                        );
                                    });
                            }
                        });
                        if double {
                            *open_turn = Some(ti);
                        }
                    });
                }
            }
            if a.calls.is_empty() {
                ui.label(egui::RichText::new("no tool calls yet").color(DIM));
            }
            if jump_to_end {
                ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
            }
        });
    *fold_all = None;
}

fn hover_card(ui: &mut egui::Ui, a: &Agent, now: i64) {
    let (label, color) = match a.status(now, theme::ACTIVE_WINDOW_MS) {
        Status::Active => ("ACTIVE", ACCENT),
        Status::Idle => ("IDLE", DIM),
        Status::Done => ("DONE", GREEN),
    };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(agent_title(a)).font(stencil(15.0)).color(ACCENT));
        ui.label(egui::RichText::new(label).color(color).small());
    });
    let end = a.ended.unwrap_or(a.last_activity);
    let open = a.calls.iter().filter(|c| c.ended.is_none()).count();
    let kind = if a.kind.is_empty() { "agent" } else { a.kind.as_str() };
    ui.label(
        egui::RichText::new(format!(
            "{kind} · {} calls ({open} open) · {} · {} tok",
            a.calls.len(),
            dur(end - a.started),
            a.output_tokens
        ))
        .small()
        .monospace(),
    );
    if let Some(c) = a.calls.iter().rev().find(|c| c.ended.is_none()).or(a.calls.last()) {
        ui.label(
            egui::RichText::new(format!("▸ {} {}", c.tool, c.summary))
                .small()
                .color(DIM),
        );
        if !c.note.is_empty() {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("“{}”", c.note))
                        .italics()
                        .small()
                        .color(SAND),
                )
                .wrap(),
            );
        }
    }
    if a.waiting_for_user && !a.last_said.trim().is_empty() {
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!("💬 {}", snippet(&a.last_said, 160)))
                    .small()
                    .color(ACCENT),
            )
            .wrap(),
        );
    }
    ui.label(egui::RichText::new("click to open").small().color(DIM));
}

/// Indented subagent tree under a session row. Clicking a row selects that agent.
fn agent_tree(
    ui: &mut egui::Ui,
    session: &Session,
    parent: &str,
    depth: u32,
    now: i64,
    selected: Option<&str>,
    pick: &mut Option<String>,
) {
    let mut children: Vec<&Agent> = session.children(parent).collect();
    children.sort_by_key(|a| a.order);
    let root = if depth == 0 { session.agent(parent) } else { None };
    for a in root.into_iter().chain(children) {
        let (dot, color) = match a.status(now, theme::ACTIVE_WINDOW_MS) {
            Status::Active => ("●", ACCENT),
            Status::Idle => ("○", DIM),
            Status::Done => ("✔", GREEN),
        };
        let name = if a.depth == 0 {
            "commander".to_string()
        } else if a.name.is_empty() {
            short(&a.id).to_string()
        } else {
            a.name.clone()
        };
        let indent = 14.0 * (a.depth as f32 + 1.0);
        ui.horizontal(|ui| {
            ui.add_space(indent);
            let text = egui::RichText::new(format!("{dot} {name}  {}", a.calls.len()))
                .small()
                .color(color);
            if ui.selectable_label(selected == Some(a.id.as_str()), text).clicked() {
                *pick = Some(a.id.clone());
            }
        });
        if a.depth > 0 || a.id != parent {
            agent_tree(ui, session, &a.id, depth + 1, now, selected, pick);
        }
    }
}

fn glyph(a: &Artifact) -> (&'static str, egui::Color32) {
    match a.kind {
        ArtifactKind::Url => ("🌐", ACCENT),
        ArtifactKind::Pr => ("🔀", SAND),
        ArtifactKind::File => ("📄", TEXT),
        ArtifactKind::Image => ("🖼", SAND),
        ArtifactKind::Commit => ("🔖", SAND),
        ArtifactKind::Test => {
            if a.ok == Some(false) {
                ("❌", RED)
            } else {
                ("✅", GREEN)
            }
        }
    }
}

/// One clickable chip. A dead URL is dimmed; an image shows a small thumbnail.
/// Newest first, with repeats folded into one chip and a count: forty "check passed" results
/// become one chip that says ×40.
fn dedup_artifacts(list: &[Artifact]) -> Vec<(&Artifact, usize)> {
    let mut out: Vec<(&Artifact, usize)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for a in list.iter().rev() {
        let key = if a.kind == ArtifactKind::Test {
            format!("test:{}", a.label)
        } else {
            format!("{:?}:{}", a.kind, a.target)
        };
        match index.get(&key) {
            Some(&i) => out[i].1 += 1,
            None => {
                index.insert(key, out.len());
                out.push((a, 1));
            }
        }
    }
    out
}

/// Files asked to open from places that have no access to the store (chips, lists): drained
/// into the editor card each frame.
static OPEN_QUEUE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Open a file in our editor card (relative paths resolve against `cwd`, or the session's folder).
pub(super) fn open_in_editor(cwd: &str, path: &str) {
    let p = std::path::Path::new(path);
    let full = if p.is_absolute() || cwd.is_empty() {
        path.to_string()
    } else {
        std::path::Path::new(cwd).join(path).display().to_string()
    };
    if let Ok(mut q) = OPEN_QUEUE.lock() {
        q.push(full);
    }
}

fn artifact_chip(
    ui: &mut egui::Ui,
    a: &Artifact,
    alive: Option<bool>,
    images: &mut HashMap<String, Option<egui::TextureHandle>>,
    thumb_px: f32,
    count: usize,
) -> egui::Response {
    let (g, mut color) = glyph(a);
    if alive == Some(false) {
        color = DIM;
    }
    let mut text = format!("{g} {}", a.label);
    if count > 1 {
        text.push_str(&format!("  ×{count}"));
    }
    if alive == Some(false) {
        text.push_str("  (down)");
    }
    let hover = match a.kind {
        ArtifactKind::Url | ArtifactKind::Pr => format!("open {}", a.target),
        ArtifactKind::File => format!("open: {}", a.target),
        ArtifactKind::Image => format!("open {}", a.target),
        ArtifactKind::Commit => format!("commit {}", a.target),
        ArtifactKind::Test => "test result".to_string(),
    };
    if a.kind == ArtifactKind::Image
        && let Some(tex) = thumbnail(ui.ctx(), images, &a.target)
    {
        let size = fit(tex.size_vec2(), thumb_px);
        return ui
            .add(egui::Button::image(egui::Image::new(egui::load::SizedTexture::new(
                tex.id(),
                size,
            ))))
            .on_hover_text(hover);
    }
    ui.button(egui::RichText::new(text).color(color)).on_hover_text(hover)
}

fn fit(size: egui::Vec2, max: f32) -> egui::Vec2 {
    let k = (max / size.x.max(size.y)).min(1.0);
    egui::vec2((size.x * k).max(1.0), (size.y * k).max(1.0))
}

/// Decode an image once and keep it as a texture. None when the file cannot be read.
fn thumbnail<'a>(
    ctx: &egui::Context,
    images: &'a mut HashMap<String, Option<egui::TextureHandle>>,
    path: &str,
) -> Option<&'a egui::TextureHandle> {
    if !images.contains_key(path) {
        let loaded = image::open(path).ok().map(|img| {
            let thumb = img.thumbnail(320, 320).to_rgba8();
            let (w, h) = thumb.dimensions();
            let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], thumb.as_raw());
            ctx.load_texture(format!("thumb:{path}"), ci, egui::TextureOptions::LINEAR)
        });
        images.insert(path.to_string(), loaded);
    }
    images.get(path).and_then(|t| t.as_ref())
}

/// Click handling for every artifact kind. Opening runs off-thread so the frame never waits.
/// Returns Some(path) when the artifact is an image: the caller opens the in-app viewer.
fn open_artifact(ctx: &egui::Context, a: &Artifact, git: Option<&GitInfo>) -> Option<String> {
    match a.kind {
        ArtifactKind::Image => return Some(a.target.clone()),
        ArtifactKind::File if doc_kind(&a.target) == DocKind::Image => return Some(a.target.clone()),
        ArtifactKind::Url | ArtifactKind::Pr => {
            let t = a.target.clone();
            std::thread::spawn(move || {
                let _ = open::that(&t);
            });
        }
        ArtifactKind::File => open_in_editor("", &a.target),
        ArtifactKind::Commit => match git.and_then(GitInfo::web_url) {
            Some(web) => {
                let url = format!("{web}/commit/{}", a.target);
                std::thread::spawn(move || {
                    let _ = open::that(&url);
                });
            }
            None => ctx.copy_text(a.target.clone()),
        },
        ArtifactKind::Test => {}
    }
    None
}

/// Speech bubbles: the newest artifact of each unit, for 90 s, floating above the unit.
fn draw_bubbles(
    ctx: &egui::Context,
    store: &mut Store,
    probe: &Probe,
    git: Option<&GitInfo>,
    images: &mut HashMap<String, Option<egui::TextureHandle>>,
    now: i64,
) {
    const FRESH_MS: i64 = 90_000;
    if store.viewer.is_some() {
        return;
    }
    let Some(session) = store.current_session() else { return };
    let mut bubbles: Vec<(String, Artifact, (f32, f32))> = Vec::new();
    for a in &session.agents {
        let Some(art) = a.artifacts.iter().rev().find(|x| now - x.at < FRESH_MS && x.at > 0) else {
            continue;
        };
        if store.dismissed_bubbles.contains(&art.target) {
            continue;
        }
        let Some(&pos) = store.unit_screen.get(&a.id) else {
            continue;
        };
        bubbles.push((a.id.clone(), art.clone(), pos));
    }
    let mut dismiss: Vec<String> = Vec::new();
    let mut view: Vec<String> = Vec::new();
    for (agent_id, art, (x, y)) in bubbles {
        let alive = if art.kind == ArtifactKind::Url {
            probe.is_alive(&art.target)
        } else {
            None
        };
        egui::Area::new(egui::Id::new(("bubble", &agent_id)))
            .fixed_pos(egui::pos2(x + 18.0, y - 70.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(PANEL)
                    .stroke(egui::Stroke::new(1.5, glyph(&art).1))
                    .inner_margin(8.0)
                    .corner_radius(10.0)
                    .show(ui, |ui| {
                        ui.set_max_width(260.0);
                        ui.horizontal(|ui| {
                            if artifact_chip(ui, &art, alive, images, 120.0, 1).clicked()
                                && let Some(path) = open_artifact(ui.ctx(), &art, git)
                            {
                                view.push(path);
                            }
                            if ui.small_button("✕").on_hover_text("dismiss").clicked() {
                                dismiss.push(art.target.clone());
                            }
                        });
                        let what = match art.kind {
                            ArtifactKind::Url => "serving, click to open",
                            ArtifactKind::Pr => "pull request",
                            ArtifactKind::File => "written, click to open",
                            ArtifactKind::Image => "looked at this",
                            ArtifactKind::Commit => "committed",
                            ArtifactKind::Test => "test run",
                        };
                        ui.label(egui::RichText::new(what).small().color(DIM));
                    });
                // Tail pointing down at the unit.
                let r = ui.min_rect();
                let p = ui.painter();
                let tip_x = r.left() + 14.0;
                p.add(egui::Shape::convex_polygon(
                    vec![
                        egui::pos2(tip_x, r.bottom()),
                        egui::pos2(tip_x + 12.0, r.bottom()),
                        egui::pos2(tip_x - 6.0, r.bottom() + 12.0),
                    ],
                    PANEL,
                    egui::Stroke::new(1.0, glyph(&art).1),
                ));
            });
    }
    for t in dismiss {
        store.dismissed_bubbles.insert(t);
    }
    if let Some(path) = view.pop() {
        open_viewer(store, &path);
    }
}

/// The authorization card: the oldest pending approval, with Allow / Deny / Ask in VS Code.
/// Y and N on the keyboard do the same.
fn draw_approvals(ctx: &egui::Context, store: &mut Store, bridge: &Bridge, inset_top: f32) -> Option<egui::Rect> {
    let req = store.approvals.first().cloned()?;
    let pending = store.approvals.len();
    let mut decision: Option<&str> = None;
    let key_yes = ctx.input(|i| i.key_pressed(egui::Key::Y));
    let key_no = ctx.input(|i| i.key_pressed(egui::Key::N));
    let win = egui::Window::new("🛡 AUTHORIZATION REQUEST")
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, inset_top + 24.0))
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.set_min_width(520.0);
            ui.label(
                egui::RichText::new(format!(
                    "{} wants to run {}",
                    if req.agent_id == ROOT_AGENT {
                        "commander"
                    } else {
                        &req.agent_id
                    },
                    req.tool
                ))
                .font(stencil(15.0))
                .color(AMBER),
            );
            if !req.cwd.is_empty() {
                ui.label(egui::RichText::new(&req.cwd).small().color(DIM));
            }
            egui::Frame::NONE.fill(INK).inner_margin(8.0).show(ui, |ui| {
                let body = req.command.clone().unwrap_or_else(|| req.summary.clone());
                let shown: String = body.lines().take(12).collect::<Vec<_>>().join("\n");
                ui.add(
                    egui::Label::new(egui::RichText::new(shown).monospace())
                        .wrap()
                        .selectable(true),
                );
                if body.lines().count() > 12 {
                    ui.label(egui::RichText::new("…").small().color(DIM));
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .button(egui::RichText::new("✔ Allow  (Y)").color(GREEN).strong())
                    .clicked()
                    || key_yes
                {
                    decision = Some("allow");
                }
                if ui
                    .button(egui::RichText::new("✖ Deny  (N)").color(RED).strong())
                    .clicked()
                    || key_no
                {
                    decision = Some("deny");
                }
                if ui
                    .button(egui::RichText::new("↪ Ask in Claude Code").color(DIM))
                    .clicked()
                {
                    decision = Some("ask");
                }
                if pending > 1 {
                    ui.label(
                        egui::RichText::new(format!("+{} more waiting", pending - 1))
                            .small()
                            .color(DIM),
                    );
                }
            });
        })?;
    if let Some(d) = decision {
        store.decide(bridge, req.id, d);
    }
    Some(win.response.rect)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DocKind {
    Image,
    Other,
}

fn doc_kind(path: &str) -> DocKind {
    let p = path.to_ascii_lowercase();
    if [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]
        .iter()
        .any(|e| p.ends_with(e))
    {
        DocKind::Image
    } else {
        DocKind::Other
    }
}

/// Open the in-app viewer on `path`, with every viewable artifact of the same unit as siblings.
fn open_viewer(store: &mut Store, path: &str) {
    let mut items: Vec<String> = store
        .current_session()
        .map(|s| {
            s.agents
                .iter()
                .find(|a| a.artifacts.iter().any(|x| x.target == path))
                .map(|a| {
                    a.artifacts
                        .iter()
                        .filter(|x| doc_kind(&x.target) == DocKind::Image)
                        .map(|x| x.target.clone())
                        .collect()
                })
                .unwrap_or_default()
        })
        .unwrap_or_default();
    if items.is_empty() {
        items.push(path.to_string());
    }
    let index = items.iter().position(|p| p == path).unwrap_or(0);
    store.viewer = Some(Viewer {
        items,
        index,
        zoom: 1.0,
        offset: (0.0, 0.0),
        max: false,
    });
}

/// Full-resolution texture for the viewer, cached under its own key.
fn full_image<'a>(
    ctx: &egui::Context,
    images: &'a mut HashMap<String, Option<egui::TextureHandle>>,
    path: &str,
) -> Option<&'a egui::TextureHandle> {
    let key = format!("full:{path}");
    if !images.contains_key(&key) {
        let loaded = image::open(path).ok().map(|img| {
            let img = img.thumbnail(2048, 2048).to_rgba8();
            let (w, h) = img.dimensions();
            let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
            ctx.load_texture(key.clone(), ci, egui::TextureOptions::LINEAR)
        });
        images.insert(key.clone(), loaded);
    }
    images.get(&key).and_then(|t| t.as_ref())
}

/// The image viewer, a card like the others (drag the title, dock it, ⛶, ✕). Drag the image to
/// pan, scroll to zoom. ◀ ▶ or arrow keys browse the unit's images, Esc or ✕ closes.
fn draw_viewer(
    ctx: &egui::Context,
    store: &mut Store,
    settings: &mut Settings,
    images: &mut HashMap<String, Option<egui::TextureHandle>>,
) -> Option<egui::Rect> {
    let mut viewer = store.viewer.clone()?;
    let n = viewer.items.len();
    let (left, right) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowLeft),
            i.key_pressed(egui::Key::ArrowRight),
        )
    });
    let step = |v: &mut Viewer, d: isize| {
        if n > 0 {
            v.index = ((v.index as isize + d).rem_euclid(n as isize)) as usize;
            v.zoom = 1.0;
            v.offset = (0.0, 0.0);
        }
    };
    if left {
        step(&mut viewer, -1);
    }
    if right {
        step(&mut viewer, 1);
    }
    let path = viewer.items.get(viewer.index).cloned().unwrap_or_default();
    let kind = doc_kind(&path);
    let vr = ctx.viewport_rect();
    let default_size = egui::vec2(vr.width() * 0.5, vr.height() * 0.6);
    let mut close = false;
    let mut open_ext = false;
    let mut nav = 0isize;
    let (index, zoom) = (viewer.index, viewer.zoom);
    let name = path.rsplit(['/', '\\']).next().unwrap_or("").to_string();
    let (mut cs, mut dock, grid) = begin_card(store, settings, "viewer-card", ctx);
    let out = floating_card(
        ctx,
        "viewer-card",
        viewer.max,
        &mut cs,
        &mut dock,
        grid,
        vr.center() - default_size / 2.0,
        default_size,
        None,
        |ui| {
            ui.label(egui::RichText::new("🖼").size(16.0));
            ui.add(egui::Label::new(egui::RichText::new(&name).font(stencil(14.0)).color(SAND)).truncate())
                .on_hover_text(&path);
            ui.label(
                egui::RichText::new(format!("{} / {}  ·  {:.0}%", index + 1, n.max(1), zoom * 100.0))
                    .small()
                    .color(DIM),
            );
            if n > 1 && ui.small_button("◀").on_hover_text("Previous (←)").clicked() {
                nav = -1;
            }
            if n > 1 && ui.small_button("▶").on_hover_text("Next (→)").clicked() {
                nav = 1;
            }
            if ui
                .small_button("↗")
                .on_hover_text("Open in the system's viewer")
                .clicked()
            {
                open_ext = true;
            }
        },
        |ui, _q| {
            let avail = ui.available_size();
            match kind {
                DocKind::Image => match full_image(ctx, images, &path) {
                    Some(tex) => {
                        let fitted = fit(tex.size_vec2(), avail.x.min(avail.y)) * viewer.zoom;
                        let out = egui::ScrollArea::both()
                            .id_salt("viewer-scroll")
                            .auto_shrink([false, false])
                            .scroll_offset(egui::vec2(viewer.offset.0, viewer.offset.1))
                            .show(ui, |ui| {
                                ui.add(egui::Image::new(egui::load::SizedTexture::new(tex.id(), fitted)))
                                    .interact(egui::Sense::drag())
                                    .on_hover_text("drag to pan · scroll to zoom")
                            });
                        let resp = out.inner;
                        let mut offset = out.state.offset;
                        if resp.dragged() {
                            offset -= resp.drag_delta();
                        }
                        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                        if out
                            .inner_rect
                            .contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default()))
                            && scroll != 0.0
                        {
                            viewer.zoom = (viewer.zoom * (1.0 + scroll * 0.002)).clamp(0.2, 8.0);
                        }
                        viewer.offset = (offset.x.max(0.0), offset.y.max(0.0));
                    }
                    None => {
                        ui.label(egui::RichText::new(format!("cannot read {path}")).color(RED));
                    }
                },
                DocKind::Other => {
                    ui.label(egui::RichText::new("this file opens in the editor, not here").color(DIM));
                }
            }
        },
    );
    end_card(store, settings, "viewer-card", cs, dock, &out.sash);
    if nav != 0 {
        step(&mut viewer, nav);
    }
    if out.toggle_max {
        viewer.max = !viewer.max;
    }
    close |= out.close;
    if open_ext {
        let p = path.clone();
        std::thread::spawn(move || {
            let _ = open::that(&p);
        });
    }
    if close {
        store.viewer = None;
    } else {
        store.viewer = Some(viewer);
    }
    out.rect
}

/// Under every unit: a ring with the task-list percentage when the agent keeps one, and a hint of
/// what it is doing right now, or "idle". Finished units show nothing.
fn draw_progress(ctx: &egui::Context, store: &Store, settings: &Settings, now: i64) {
    if store.viewer.is_some() || !settings.progress_rings {
        return;
    }
    let Some(session) = store.current_session() else { return };
    let t = ctx.input(|i| i.time) as f32;
    let vr = ctx.viewport_rect();
    let (l, tp, r, b) = store.scene_insets.unwrap_or((0.0, 0.0, 0.0, 0.0));
    let scene = egui::Rect::from_min_max(
        egui::pos2(vr.min.x + l, vr.min.y + tp),
        egui::pos2(vr.max.x - r, vr.max.y - b),
    );
    let painter = ctx
        .layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("progress")))
        .with_clip_rect(scene);
    for (id, &(x, y)) in &store.unit_screen {
        let Some(agent) = session.agent(id) else { continue };
        if agent.ended.is_some() {
            continue;
        }
        let center = egui::pos2(x, y + 34.0);
        if !scene.contains(center) {
            continue;
        }
        let active = agent.status(now, theme::ACTIVE_WINDOW_MS) == Status::Active;
        // An open call is work in progress however long ago it started (a long build is still work).
        let busy = agent.has_open_call();
        let open_call = agent.calls.iter().rev().find(|c| c.ended.is_none());
        // Your prompt is open and nothing has been answered yet: the model is generating. The
        // transcript stays silent until a reply completes, so this is "thinking", not idle.
        let in_turn = agent.turn_log.last().is_some_and(|t| t.ended.is_none()) && !agent.waiting_for_user;
        // Hint: the in-progress task, else the running call, else thinking, else idle.
        let hint: String = if busy {
            agent
                .progress
                .as_ref()
                .filter(|p| !p.current.is_empty())
                .map(|p| p.current.clone())
                .or_else(|| {
                    open_call.map(|c| {
                        if c.note.is_empty() {
                            format!("{} {}", c.tool, c.summary)
                        } else {
                            c.note.clone()
                        }
                    })
                })
                .unwrap_or_default()
        } else if active || in_turn {
            "thinking…".into()
        } else {
            "idle".into()
        };
        let hint: String = hint.chars().take(42).collect();
        let color = if busy { ACCENT } else { DIM };
        let radius = 11.0;
        match agent.progress.as_ref().filter(|p| p.total > 0) {
            Some(p) => {
                painter.circle_stroke(
                    center,
                    radius,
                    egui::Stroke::new(2.5, egui::Color32::from_rgb(35, 55, 28)),
                );
                let frac = p.fraction().clamp(0.0, 1.0);
                if frac > 0.0 {
                    let steps = (frac * 40.0).ceil().max(1.0) as usize;
                    let pts: Vec<egui::Pos2> = (0..=steps)
                        .map(|i| {
                            let a =
                                -std::f32::consts::FRAC_PI_2 + frac * std::f32::consts::TAU * (i as f32 / steps as f32);
                            egui::pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
                        })
                        .collect();
                    painter.add(egui::Shape::line(pts, egui::Stroke::new(2.5, SAND)));
                }
                painter.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    format!("{:.0}", frac * 100.0),
                    egui::FontId::proportional(10.0),
                    TEXT,
                );
            }
            None if busy || in_turn => {
                // No task list: a spinner says "working".
                let a0 = t * 4.0;
                let pts: Vec<egui::Pos2> = (0..=12)
                    .map(|i| {
                        let a = a0 + i as f32 * 0.14;
                        egui::pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
                    })
                    .collect();
                painter.circle_stroke(
                    center,
                    radius,
                    egui::Stroke::new(1.5, egui::Color32::from_rgb(35, 55, 28)),
                );
                painter.add(egui::Shape::line(pts, egui::Stroke::new(2.5, ACCENT)));
            }
            None => {
                painter.circle_stroke(
                    center,
                    radius,
                    egui::Stroke::new(1.5, egui::Color32::from_rgb(35, 55, 28)),
                );
            }
        }
        painter.text(
            center + egui::vec2(0.0, radius + 4.0),
            egui::Align2::CENTER_TOP,
            hint,
            egui::FontId::proportional(11.0),
            color,
        );
    }
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn art(kind: ArtifactKind, label: &str, target: &str) -> Artifact {
        Artifact {
            kind,
            label: label.into(),
            target: target.into(),
            call_id: String::new(),
            at: 0,
            ok: None,
        }
    }

    #[test]
    fn shelf_folds_repeats_newest_first() {
        let list = vec![
            art(ArtifactKind::File, "a.rs", "/x/a.rs"),
            art(ArtifactKind::Test, "check passed", "check:1"),
            art(ArtifactKind::Test, "check passed", "check:2"),
            art(ArtifactKind::File, "a.rs", "/x/a.rs"),
            art(ArtifactKind::Test, "check passed", "check:3"),
        ];
        let folded = dedup_artifacts(&list);
        assert_eq!(folded.len(), 2);
        assert_eq!((folded[0].0.label.as_str(), folded[0].1), ("check passed", 3));
        assert_eq!((folded[1].0.label.as_str(), folded[1].1), ("a.rs", 2));
    }

    #[test]
    fn search_hits_are_found_and_previewed_around_matches() {
        assert_eq!(
            match_ranges("Password: x, password!", "password"),
            vec![(0, 8), (13, 21)]
        );
        assert_eq!(match_ranges("héllo wörld", "wörld"), vec![(7, 13)]);
        assert!(match_ranges("abc", "").is_empty());
        let lines: Vec<&str> = (1..=30)
            .map(|i| if i == 20 { "hit here" } else { "noise" })
            .collect::<Vec<_>>();
        let p = preview_lines(&lines, "hit", 10);
        assert!(p.starts_with("noise\nhit here\nnoise"), "{p}");
        assert!(!p.contains("⋯"), "one cluster, no gap marker: {p}");
        let lines2: Vec<&str> = (1..=30)
            .map(|i| if i == 3 || i == 25 { "hit" } else { "noise" })
            .collect();
        let p2 = preview_lines(&lines2, "hit", 10);
        assert!(p2.contains("⋯"), "two clusters show a gap: {p2}");
        assert_eq!(
            preview_lines(&lines, "", 10).lines().count(),
            10,
            "no search: first ten lines"
        );
        assert_eq!(preview_lines(&["a", "b"], "zzz", 10), "a\nb");
    }

    #[test]
    fn history_search_is_case_insensitive_and_reads_output() {
        let mut c = agent_globe::data::model::ToolCall {
            id: "1".into(),
            tool: "Bash".into(),
            summary: "run the tests".into(),
            command: Some("cargo test".into()),
            note: String::new(),
            stdout: "test result: ok. 12 passed".into(),
            stderr: String::new(),
            is_error: false,
            started: 0,
            ended: Some(1),
            background: false,
        };
        assert!(call_matches(&c, "CARGO"));
        assert!(call_matches(&c, "12 passed"));
        assert!(!call_matches(&c, "clippy"));
        c.stderr = "warning: unused import".into();
        assert!(call_matches(&c, "unused"));
        assert!(contains_ci("Héllo", "héllo"), "non-ascii bytes compare exactly");
    }
}

#[cfg(test)]
mod dock_tests {
    use super::*;

    fn close(a: Pane, b: Pane) -> bool {
        (a.x0 - b.x0).abs() < 1e-4
            && (a.x1 - b.x1).abs() < 1e-4
            && (a.y0 - b.y0).abs() < 1e-4
            && (a.y1 - b.y1).abs() < 1e-4
    }

    fn wall() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(3400.0, 900.0))
    }

    #[test]
    fn columns_follow_the_monitors() {
        // two monitors, nothing open: four even columns
        let grid = DockGrid::new(wall(), wall(), &[1700.0], 2);
        assert_eq!(grid.cols(), 4);
        assert!((grid.xs[2] - 0.5).abs() < 1e-6, "the seam is a grid line");
        // a 300 px sidebar: the left monitor's free span is split, the seam stays put
        let area = egui::Rect::from_min_max(egui::pos2(300.0, 0.0), egui::pos2(3400.0, 900.0));
        let grid = DockGrid::new(wall(), area, &[1700.0], 2);
        assert_eq!(grid.cols(), 4);
        assert!((grid.xs[0] - 300.0 / 3400.0).abs() < 1e-6);
        assert!((grid.xs[1] - 1000.0 / 3400.0).abs() < 1e-6);
        assert!((grid.xs[2] - 0.5).abs() < 1e-6, "the seam is still a grid line");
        let left = grid.block(grid.pane_of(0, 2, 0, 2));
        assert!(
            (left.max.x - (1700.0 - 6.0)).abs() < 0.5,
            "a left-monitor pane ends at the seam"
        );
        assert!((left.min.x - (300.0 + 6.0)).abs() < 0.5, "and starts after the sidebar");
        // a pane saved before the sidebar opened is clipped to the free area, down to a sliver
        let under = grid.block(Pane {
            x0: 0.0,
            y0: 0.0,
            x1: 0.05,
            y1: 1.0,
        });
        assert!(under.min.x >= 300.0 && under.width() >= 120.0 - 12.0 - 0.5);
    }

    #[test]
    fn dock_grid_math() {
        let grid = DockGrid::new(wall(), wall(), &[1700.0], 2);
        let wild = Pane {
            x0: 1.4,
            y0: -0.5,
            x1: 0.1,
            y1: 9.0,
        };
        let c = grid.clamp(wild);
        assert!(c.x0 <= 1.0 - DockGrid::MIN && c.x1 >= c.x0 + DockGrid::MIN && c.y0 == 0.0 && c.y1 == 1.0);
        let left = grid.block(Pane {
            x0: 0.0,
            y0: 0.0,
            x1: 0.5,
            y1: 1.0,
        });
        assert!((left.width() - (1700.0 - 12.0)).abs() < 0.5 && (left.height() - (900.0 - 12.0)).abs() < 0.5);
        assert!(close(
            grid.target_at(egui::pos2(425.0, 225.0)).unwrap(),
            grid.pane_of(0, 1, 0, 1)
        ));
        assert!(close(
            grid.target_at(egui::pos2(3000.0, 700.0)).unwrap(),
            grid.pane_of(3, 4, 1, 2)
        ));
        assert!(
            close(
                grid.target_at(egui::pos2(425.0, 450.0)).unwrap(),
                grid.pane_of(0, 1, 0, 2)
            ),
            "left half"
        );
        assert!(
            close(
                grid.target_at(egui::pos2(850.0, 225.0)).unwrap(),
                grid.pane_of(0, 2, 0, 1)
            ),
            "top half"
        );
        assert!(
            close(
                grid.target_at(egui::pos2(850.0, 450.0)).unwrap(),
                grid.pane_of(0, 2, 0, 2)
            ),
            "whole monitor"
        );
        assert!(
            close(
                grid.target_at(egui::pos2(1700.0, 225.0)).unwrap(),
                grid.pane_of(0, 4, 0, 1)
            ),
            "top strip across the wall"
        );
        assert!(
            close(
                grid.target_at(egui::pos2(1700.0, 450.0)).unwrap(),
                grid.pane_of(0, 4, 0, 2)
            ),
            "the whole wall"
        );
        assert_eq!(
            grid.target_at(egui::pos2(600.0, 450.0)),
            None,
            "between targets a card stays floating"
        );
        assert_eq!(
            grid.targets().len(),
            8 + 4 + 4 + 2 + 2 + 1,
            "8 quarters, 4 top/bottom halves, 4 left/right halves, 2 monitors, 2 wall strips, the wall"
        );
        assert!(
            (grid.snap_x(1690.0) - 0.5).abs() < 1e-6,
            "ten pixels from the seam snaps"
        );
        assert!(
            (grid.snap_x(1300.0) - 1300.0 / 3400.0).abs() < 1e-6,
            "far from a line stays free"
        );
        assert!((grid.snap_y(455.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_shared_edge_moves_the_neighbour() {
        let editor = Pane {
            x0: 0.25,
            y0: 0.0,
            x1: 0.5,
            y1: 1.0,
        };
        let moved = sash_follow(editor, 0, 0.25, 0.15);
        assert!((moved.x0 - 0.15).abs() < 1e-6 && (moved.x1 - 0.5).abs() < 1e-6);
        let other = Pane {
            x0: 0.5,
            y0: 0.0,
            x1: 0.75,
            y1: 1.0,
        };
        assert_eq!(sash_follow(other, 0, 0.25, 0.15), other);
        let thin = Pane {
            x0: 0.25,
            y0: 0.0,
            x1: 0.3,
            y1: 1.0,
        };
        let pushed = sash_follow(thin, 0, 0.25, 0.29);
        assert!(pushed.x1 - pushed.x0 >= DockGrid::MIN - 1e-6);
    }
}

#[cfg(test)]
mod term_bench {
    use super::*;

    fn stat(v: &mut [u128]) -> (u128, u128, u128) {
        v.sort_unstable();
        (v[v.len() / 2], v[v.len() * 95 / 100], v[v.len() - 1])
    }

    fn measure(tab: &mut terminal::PtyTab, frames: usize) -> ((u128, u128, u128), (u128, u128, u128)) {
        let ctx = egui::Context::default();
        let mut read_us = Vec::new();
        let mut frame_us = Vec::new();
        for i in 0..frames {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 1100.0))),
                time: Some(i as f64 / 60.0),
                ..Default::default()
            };
            let t = std::time::Instant::now();
            let _ = tab.frame();
            read_us.push(t.elapsed().as_micros());
            let t = std::time::Instant::now();
            let _ = ctx.run_ui(input.clone(), |ui| draw_grid_inner(ui, tab, 13.0));
            frame_us.push(t.elapsed().as_micros());
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        (stat(&mut read_us), stat(&mut frame_us))
    }

    /// A TUI's worth of output: 300 full-screen colour redraws, then a marker; how long until
    /// the marker shows, and how long a keystroke takes to echo while the redraws run.
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn term_tui_throughput() {
        let tab = terminal::PtyTab::spawn("", 200, 60).expect("conpty");
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let script = "$e=[char]27; $l=(1..58|%{ \"$e[3$($_%7+1)m line $_ $('x'*150)$e[0m\" }) -join \"`r`n\"; 1..300|%{ [Console]::Write(\"$e[H\" + $l) }; [Console]::Write(\"$e[2J$e[HAGDONE\")";
        let t = std::time::Instant::now();
        tab.write(script.as_bytes().to_vec());
        tab.write(vec![13u8]);
        loop {
            if tab.all_lines().iter().any(|l| l.trim() == "AGDONE") || t.elapsed().as_secs() > 60 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        eprintln!(
            "TUI BENCH: 300 full-screen redraws shown in {} ms",
            t.elapsed().as_millis()
        );
    }

    /// How long a line takes to show, plain and wrapped in synchronized-output markers.
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn term_sync_latency() {
        let tab = terminal::PtyTab::spawn("", 120, 30).expect("conpty");
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let shown = |needle: &str| tab.all_lines().iter().any(|l| l.contains(needle));
        for (label, pre, post) in [("plain", "", ""), ("sync", "$e[?2026h", "$e[?2026l")] {
            let mut ms = Vec::new();
            for i in 0..8 {
                let tag = format!("AGLAT{label}{i}");
                let cmd = format!("$e=[char]27; [Console]::Write(\"{pre}{tag}{post}\")");
                tab.write(cmd.into_bytes());
                tab.write(vec![13u8]);
                // the echo of the command line contains the tag too: wait for the printed one
                let t = std::time::Instant::now();
                loop {
                    let n = tab.all_lines().iter().filter(|l| l.contains(&tag)).count();
                    if n >= 2 || t.elapsed().as_millis() > 2000 {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                ms.push(t.elapsed().as_millis());
                let _ = shown("");
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            eprintln!("SYNC BENCH {label}: {ms:?} ms");
        }
    }

    /// `cargo test --release term_frame_cost -- --ignored --nocapture`: a real console, once
    /// static and once under a Claude-like stream of full-screen coloured redraws.
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn term_frame_cost() {
        let mut tab = terminal::PtyTab::spawn("", 200, 60).expect("conpty");
        std::thread::sleep(std::time::Duration::from_millis(1500));
        tab.write(b"Get-ChildItem C:\\Windows\\System32 | Select-Object -First 400 | Format-Table -AutoSize".to_vec());
        tab.write(vec![13u8]);
        std::thread::sleep(std::time::Duration::from_millis(3000));
        let (r, f) = measure(&mut tab, 120);
        eprintln!("TERM BENCH static: read p50/p95/max {r:?} us | grid frame {f:?} us");
        // A TUI redrawing the whole screen in colour, as fast as it can (Claude Code's spinner and
        // status lines redraw like this many times a second).
        let script = "$e=[char]27; $l=(1..58|%{ \"$e[3$($_%7+1)m line $_ $('x'*150)$e[0m\" }) -join \"`r`n\"; $t=[Diagnostics.Stopwatch]::StartNew(); while($t.Elapsed.TotalSeconds -lt 6){ [Console]::Write(\"$e[H\" + $l) }";
        tab.write(script.as_bytes().to_vec());
        tab.write(vec![13u8]);
        std::thread::sleep(std::time::Duration::from_millis(500));
        let (r, f) = measure(&mut tab, 200);
        eprintln!("TERM BENCH streaming: read p50/p95/max {r:?} us | grid frame {f:?} us");
    }
}
