//! egui layer: HUD (top), sessions (left), selected agent (right), status (bottom), toasts,
//! hover card, settings and debrief windows. Reports whether the pointer is over any of these so
//! the camera can ignore input that belongs to the UI.

use super::debrief;
use super::gitinfo::{GitInfo, GitWatch};
use super::probe::Probe;
use super::settings::Settings;
use super::sounds::{Sfx, Sounds};
use super::state::{BadgeKind, BridgeRes, Store, ToastKind, Viewer};
use super::theme;
use agent_globe::bridge::Bridge;
use agent_globe::data::artifacts::{Artifact, ArtifactKind};
use agent_globe::data::claude::SessionRef;
use agent_globe::data::cost;
use agent_globe::data::model::{Agent, ROOT_AGENT, Session, Status, now_millis};
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use bevy_panorbit_camera::PanOrbitCamera;
use std::collections::{HashMap, HashSet};

// Army palette.
const ACCENT: egui::Color32 = egui::Color32::from_rgb(160, 232, 110); // light plastic
const DIM: egui::Color32 = egui::Color32::from_rgb(120, 140, 95);
const GREEN: egui::Color32 = egui::Color32::from_rgb(190, 240, 140);
const RED: egui::Color32 = egui::Color32::from_rgb(255, 100, 80);
const AMBER: egui::Color32 = egui::Color32::from_rgb(255, 185, 70);
const SAND: egui::Color32 = egui::Color32::from_rgb(222, 200, 135);
const TEXT: egui::Color32 = egui::Color32::from_rgb(222, 226, 200);
const PANEL: egui::Color32 = egui::Color32::from_rgba_premultiplied(8, 14, 5, 228);
const INK: egui::Color32 = egui::Color32::from_rgb(4, 8, 3);
const SCANLINE: egui::Color32 = egui::Color32::from_rgba_premultiplied(0, 0, 0, 60);
const MAX_LINES: usize = 10;

fn stencil(size: f32) -> egui::FontId {
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
    mut images: Local<HashMap<String, Option<egui::TextureHandle>>>,
    mut styled: Local<bool>,
    mut last_scale: Local<f32>,
    mut last_panel: Local<Option<String>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    if style_once(ctx, &mut styled) {
        return Ok(());
    }
    if (*last_scale - settings.ui_scale).abs() > 1e-3 {
        ctx.set_zoom_factor(settings.ui_scale);
        *last_scale = settings.ui_scale;
    }
    let now = now_millis();
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

    // ---- cinematic mode: scene only ----
    if store.hide_ui {
        store.scene_insets = Some((0.0, 0.0, 0.0, 0.0));
        store.pointer_over_ui = false;
        egui::Area::new(egui::Id::new("cinematic-hint"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -12.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("F or Esc: back to the console").small().color(DIM));
            });
        draw_badges(ctx, &store);
        draw_bubbles(ctx, &mut store, &probe, git.as_ref(), &mut images, now);
        draw_viewer(ctx, &mut store, &mut images);
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
    let mut toggle_debrief = false;
    let mut go_home = false;
    let mut cinematic = false;
    let mut toggle_sound = false;
    let hud = egui::Panel::top("hud").show(&mut root, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("AGENT GLOBE").font(stencil(20.0)).color(ACCENT));
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
                    ui.separator();
                    tip(
                        ui,
                        egui::RichText::new(format!("⏱ {}", dur(s.last_activity - s.started))).monospace(),
                        "Session length: first event to the latest one.",
                    );
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
                    tip(
                        ui,
                        egui::RichText::new(format!("ctx {ctx_k}k")).monospace().color(ctx_color),
                        "Context size: how many tokens the commander's last request carried (prompt + history + tool results).\nAmber at two thirds of this model's window, red near the limit: that is where the agent starts forgetting early instructions.",
                    );
                    let spent = s.cost_usd();
                    let spent_color = if settings.budget_usd > 0.0 && spent >= settings.budget_usd as f64 { RED } else { SAND };
                    tip(
                        ui,
                        egui::RichText::new(cost::fmt_usd(spent)).monospace().color(spent_color),
                        "Estimated spend for this session from token counts and list prices (input, cache reads, cache writes, output).\nSet a budget alarm in settings.",
                    );
                    if let Some(g) = git.as_ref() {
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
                if ui
                    .button(egui::RichText::new("🎖 debrief").color(SAND))
                    .on_hover_text("After-action report: where the time went and what to do better next prompt.")
                    .clicked()
                {
                    toggle_debrief = true;
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
    if toggle_debrief {
        store.show_debrief = !store.show_debrief;
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
    let left = egui::Panel::left("sessions")
        .resizable(true)
        .default_size(230.0)
        .show(&mut root, |ui| {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(store.claude_dir.display().to_string())
                    .small()
                    .color(DIM),
            );
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
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
                                let text =
                                    format!("{dot} {}  {:>3}  {:>6}k", short(&s.id), age(s.modified), s.size / 1024);
                                let rt = egui::RichText::new(text).monospace();
                                let rt = if active { rt.color(ACCENT) } else { rt };
                                if ui.selectable_label(is_current, rt).clicked() {
                                    open = Some(s.clone());
                                }
                                if is_current && let Some(session) = store.sessions.get(&s.id) {
                                    agent_tree(ui, session, ROOT_AGENT, 0, now, store.selected.as_deref(), &mut pick);
                                }
                            }
                        });
                }
            });
        });
    ui_rects.push(left.response.rect);
    let inset_left = left.response.rect.width();
    if let Some(s) = open {
        store.open(&s, &bridge.0);
    }
    if let Some(id) = show {
        store.show(&id);
    }
    if let Some(id) = pick {
        store.selected = Some(id);
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
                egui::RichText::new("drag rotate · scroll zoom · right-drag pan · click unit · R home · F cinematic")
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
    let mut inset_right = 0.0_f32;
    let just_opened = selected.as_ref().map(|a| &a.id) != last_panel.as_ref();
    *last_panel = selected.as_ref().map(|a| a.id.clone());
    if let Some(agent) = selected.as_ref() {
        let narrator = settings.narrator;
        let stalled = store.stalled.clone();
        let right = egui::Panel::right("agent")
            .resizable(true)
            .default_size(520.0)
            .max_size(ctx.viewport_rect().width() * 0.6)
            .show(&mut root, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.small_button("✕ close  (Esc)").clicked() {
                        close = true;
                    }
                });
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
                );
            });
        inset_right = right.response.rect.width();
        ui_rects.push(right.response.rect);
    }
    if close {
        store.selected = None;
    }
    if let Some(path) = view_image.take() {
        open_viewer(&mut store, &path);
    }
    // Hover: a small card at the top right that never steals the pointer.
    if let Some(agent) = hovered
        .as_ref()
        .filter(|h| selected.as_ref().map(|s| &s.id) != Some(&h.id))
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

    // ---- settings window ----
    if store.show_settings {
        let mut open = true;
        let win = egui::Window::new("⚙ settings")
            .open(&mut open)
            .resizable(false)
            .default_pos(egui::pos2(260.0, 80.0))
            .show(ctx, |ui| {
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
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("test:").small().color(DIM));
                    for s in [
                        Sfx::Prompt,
                        Sfx::Spawn,
                        Sfx::Done,
                        Sfx::Error,
                        Sfx::Complete,
                        Sfx::Stall,
                        Sfx::Loop,
                    ] {
                        if ui.small_button(s.name()).on_hover_text(s.line()).clicked() {
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
                ui.checkbox(&mut settings.narrator, "narrator line under each call");
                ui.separator();
                ui.label(egui::RichText::new("ALERTS").small().color(DIM));
                ui.add(
                    egui::Slider::new(&mut settings.budget_usd, 0.0..=200.0)
                        .text("budget alarm ($ per session, 0 = off)"),
                );
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
                ui.horizontal(|ui| {
                    if ui.button("reset defaults").clicked() {
                        *settings = Settings::default();
                    }
                    ui.label(
                        egui::RichText::new(super::settings::path().display().to_string())
                            .small()
                            .color(DIM),
                    );
                });
            });
        if let Some(w) = win {
            ui_rects.push(w.response.rect);
        }
        if !open {
            store.show_settings = false;
        }
    }

    // ---- debrief window ----
    if store.show_debrief {
        let mut open = true;
        let report = store.current_session().map(debrief::report);
        let win = egui::Window::new("🎖 debrief")
            .open(&mut open)
            .resizable(true)
            .default_width(520.0)
            .default_pos(egui::pos2(300.0, 90.0))
            .show(ctx, |ui| match report {
                None => {
                    ui.label(egui::RichText::new("no session open").color(DIM));
                }
                Some(r) => {
                    ui.label(
                        egui::RichText::new("AFTER-ACTION REPORT")
                            .font(stencil(16.0))
                            .color(SAND),
                    );
                    ui.add_space(4.0);
                    egui::Grid::new("debrief-rows")
                        .num_columns(2)
                        .spacing([18.0, 2.0])
                        .show(ui, |ui| {
                            for (k, v) in &r.rows {
                                ui.label(egui::RichText::new(k).color(DIM));
                                ui.label(egui::RichText::new(v).monospace());
                                ui.end_row();
                            }
                        });
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("WHERE THE TIME WENT").small().color(DIM));
                    for (cat, share, n) in &r.shares {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::ProgressBar::new(*share)
                                    .desired_width(180.0)
                                    .fill(egui::Color32::from_rgb(60, 110, 40))
                                    .text(egui::RichText::new(format!("{:.0}%", share * 100.0)).small()),
                            );
                            ui.label(egui::RichText::new(format!("{}  ({n} calls)", cat.label())).small());
                        });
                    }
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("COACH").small().color(DIM));
                    for t in &r.tips {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(egui::RichText::new("▸").color(SAND));
                            ui.label(egui::RichText::new(t).color(TEXT));
                        });
                    }
                }
            });
        if let Some(w) = win {
            ui_rects.push(w.response.rect);
        }
        if !open {
            store.show_debrief = false;
        }
    }

    draw_badges(ctx, &store);
    draw_bubbles(ctx, &mut store, &probe, git.as_ref(), &mut images, now);
    if let Some(rect) = draw_viewer(ctx, &mut store, &mut images) {
        ui_rects.push(rect);
    }
    if let Some(rect) = draw_approvals(ctx, &mut store, &bridge.0, inset_top) {
        ui_rects.push(rect);
    }
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

/// Notification badges floating above units: lamp for done, chat bubble for waiting, red for error.
fn draw_badges(ctx: &egui::Context, store: &Store) {
    if store.badges.is_empty() {
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
    for b in &store.badges {
        if !scene.contains(egui::pos2(b.x, b.y)) {
            continue;
        }
        let (glyph, color, label) = match b.kind {
            BadgeKind::Done => ("!", egui::Color32::from_rgb(255, 215, 90), "done"),
            BadgeKind::Waiting => ("💬", ACCENT, "awaiting your orders"),
            BadgeKind::Error => ("⚠", RED, "failed call"),
            BadgeKind::Approval => ("🛡", AMBER, "authorization needed"),
        };
        let center = egui::pos2(b.x, b.y - 10.0 - 4.0 * (t * 2.0).sin());
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
    ctx.request_repaint();
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
fn code_block(ui: &mut egui::Ui, key: &str, text: &str, color: egui::Color32, expanded: &mut HashSet<String>) {
    let lines: Vec<&str> = text.lines().collect();
    let is_open = expanded.contains(key);
    let shown = if lines.len() > MAX_LINES && !is_open {
        lines[..MAX_LINES].join("\n")
    } else {
        text.to_string()
    };
    ui.add(
        egui::Label::new(egui::RichText::new(shown).monospace().color(color))
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
        ui.label(egui::RichText::new("ARTIFACTS").small().color(DIM));
        let key = format!("{}:artifacts", a.id);
        let all = expanded.contains(&key);
        let total = a.artifacts.len();
        let row_h = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
        let shelf_h = if all { f32::INFINITY } else { row_h * 2.0 + 4.0 };
        let mut toggle = false;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{total} items")).small().color(DIM));
            let label = if all { "▴ collapse" } else { "▾ expand" };
            if ui.small_button(egui::RichText::new(label).small()).clicked() {
                toggle = true;
            }
        });
        egui::ScrollArea::vertical()
            .id_salt(&key)
            .max_height(shelf_h)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for art in a.artifacts.iter().rev() {
                        let alive = if art.kind == ArtifactKind::Url {
                            probe.is_alive(&art.target)
                        } else {
                            None
                        };
                        if artifact_chip(ui, art, alive, images, row_h - 6.0).clicked()
                            && let Some(path) = open_artifact(ui.ctx(), art, git)
                        {
                            *view_image = Some(path);
                        }
                    }
                });
            });
        if toggle {
            if all {
                expanded.remove(&key);
            } else {
                expanded.insert(key);
            }
        }
    }
    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            if !a.prompt.is_empty() {
                ui.collapsing(egui::RichText::new("orders (prompt)").color(SAND), |ui| {
                    code_block(ui, &format!("{}:prompt", a.id), &a.prompt, TEXT, expanded);
                });
            }
            ui.label(egui::RichText::new("TOOL CALLS").small().color(DIM));
            for (i, c) in a.calls.iter().enumerate() {
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
                    ui.add(egui::Label::new(egui::RichText::new(header).monospace()).truncate());
                    ui.label(egui::RichText::new(took).small().color(DIM));
                });
                if narrator && !c.note.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(26.0);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!("“{}”", c.note))
                                    .italics()
                                    .small()
                                    .color(SAND),
                            )
                            .wrap(),
                        );
                    });
                }
                if let Some(cmd) = &c.command {
                    egui::Frame::NONE
                        .fill(INK)
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(50, 90, 35)))
                        .inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(format!("$ {cmd}")).monospace().color(ACCENT));
                            if !c.stdout.is_empty() {
                                code_block(ui, &c.id, &c.stdout, TEXT, expanded);
                            }
                            if !c.stderr.is_empty() {
                                code_block(ui, &format!("{}:err", c.id), &c.stderr, RED, expanded);
                            }
                        });
                } else if !c.stdout.is_empty() {
                    egui::Frame::NONE.fill(INK).inner_margin(6.0).show(ui, |ui| {
                        code_block(ui, &c.id, &c.stdout, DIM, expanded);
                    });
                }
                ui.add_space(4.0);
            }
            if a.calls.is_empty() {
                ui.label(egui::RichText::new("no tool calls yet").color(DIM));
            }
            if jump_to_end {
                ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
            }
        });
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
fn artifact_chip(
    ui: &mut egui::Ui,
    a: &Artifact,
    alive: Option<bool>,
    images: &mut HashMap<String, Option<egui::TextureHandle>>,
    thumb_px: f32,
) -> egui::Response {
    let (g, mut color) = glyph(a);
    if alive == Some(false) {
        color = DIM;
    }
    let mut text = format!("{g} {}", a.label);
    if alive == Some(false) {
        text.push_str("  (down)");
    }
    let hover = match a.kind {
        ArtifactKind::Url | ArtifactKind::Pr => format!("open {}", a.target),
        ArtifactKind::File => format!("open in VS Code: {}", a.target),
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
        ArtifactKind::Url | ArtifactKind::Pr => {
            let t = a.target.clone();
            std::thread::spawn(move || {
                let _ = open::that(&t);
            });
        }
        ArtifactKind::File => {
            let t = a.target.clone();
            std::thread::spawn(move || {
                let mut cmd = std::process::Command::new("cmd");
                cmd.args(["/c", "code", "-g", &t]);
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    cmd.creation_flags(0x0800_0000);
                }
                let _ = cmd.spawn();
            });
        }
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
                            if artifact_chip(ui, &art, alive, images, 120.0).clicked()
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
                            ArtifactKind::File => "written, click to open in VS Code",
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
                if ui.button(egui::RichText::new("↪ Ask in VS Code").color(DIM)).clicked() {
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

/// Open the in-app viewer on `path`, with every image of the same unit as siblings.
fn open_viewer(store: &mut Store, path: &str) {
    let mut paths: Vec<String> = store
        .current_session()
        .map(|s| {
            s.agents
                .iter()
                .find(|a| a.artifacts.iter().any(|x| x.target == path))
                .map(|a| {
                    a.artifacts
                        .iter()
                        .filter(|x| x.kind == ArtifactKind::Image)
                        .map(|x| x.target.clone())
                        .collect()
                })
                .unwrap_or_default()
        })
        .unwrap_or_default();
    if paths.is_empty() {
        paths.push(path.to_string());
    }
    let index = paths.iter().position(|p| p == path).unwrap_or(0);
    store.viewer = Some(Viewer {
        paths,
        index,
        zoom: 1.0,
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

/// Centered image viewer: scroll to zoom, ◀ ▶ or arrow keys to browse, Esc or ✕ to close.
fn draw_viewer(
    ctx: &egui::Context,
    store: &mut Store,
    images: &mut HashMap<String, Option<egui::TextureHandle>>,
) -> Option<egui::Rect> {
    let mut viewer = store.viewer.clone()?;
    let n = viewer.paths.len();
    let (left, right) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowLeft),
            i.key_pressed(egui::Key::ArrowRight),
        )
    });
    if left && n > 0 {
        viewer.index = (viewer.index + n - 1) % n;
    }
    if right && n > 0 {
        viewer.index = (viewer.index + 1) % n;
    }
    let path = viewer.paths.get(viewer.index).cloned().unwrap_or_default();
    let vr = ctx.viewport_rect();
    let max = egui::vec2(vr.width() * 0.82, vr.height() * 0.78);
    let mut close = false;
    let mut open_ext = false;
    let win = egui::Window::new("image viewer")
        .title_bar(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .resizable(false)
        .collapsible(false)
        .frame(
            egui::Frame::NONE
                .fill(INK)
                .stroke(egui::Stroke::new(1.5, SAND))
                .inner_margin(10.0)
                .corner_radius(8.0),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(short_path(&path)).font(stencil(14.0)).color(SAND));
                ui.label(
                    egui::RichText::new(format!("{} / {}", viewer.index + 1, n.max(1)))
                        .small()
                        .color(DIM),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✕  (Esc)").clicked() {
                        close = true;
                    }
                    if ui.button("open externally").clicked() {
                        open_ext = true;
                    }
                    if n > 1 && ui.button("▶").clicked() {
                        viewer.index = (viewer.index + 1) % n;
                    }
                    if n > 1 && ui.button("◀").clicked() {
                        viewer.index = (viewer.index + n - 1) % n;
                    }
                    ui.label(
                        egui::RichText::new(format!("{:.0}%  (scroll to zoom)", viewer.zoom * 100.0))
                            .small()
                            .color(DIM),
                    );
                });
            });
            ui.add_space(6.0);
            match full_image(ctx, images, &path) {
                Some(tex) => {
                    let fitted = fit(tex.size_vec2(), max.x.min(max.y)) * viewer.zoom;
                    let shown = egui::vec2(fitted.x.min(max.x), fitted.y.min(max.y));
                    let resp = egui::ScrollArea::both()
                        .max_width(max.x)
                        .max_height(max.y)
                        .show(ui, |ui| {
                            ui.add(egui::Image::new(egui::load::SizedTexture::new(tex.id(), fitted)))
                        })
                        .inner;
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if resp.hovered() && scroll != 0.0 {
                        viewer.zoom = (viewer.zoom * (1.0 + scroll * 0.002)).clamp(0.2, 8.0);
                    }
                    let _ = shown;
                }
                None => {
                    ui.label(egui::RichText::new(format!("cannot read {path}")).color(RED));
                }
            }
        })?;
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
    Some(win.response.rect)
}

fn short_path(p: &str) -> String {
    let parts: Vec<&str> = p.rsplit(['/', '\\']).take(3).collect();
    parts.into_iter().rev().collect::<Vec<_>>().join("/")
}
