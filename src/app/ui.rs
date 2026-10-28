//! egui layer: HUD (top), sessions (left), selected agent (right), status (bottom), toasts,
//! settings window. Also reports whether the pointer is over any of these so the camera can
//! ignore input that belongs to the UI.

use super::settings::Settings;
use super::state::{BridgeRes, Store, ToastKind};
use super::theme;
use agent_globe::data::claude::SessionRef;
use agent_globe::data::model::{Agent, Session, Status, now_millis};
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use bevy_panorbit_camera::PanOrbitCamera;
use std::collections::HashSet;

const CYAN: egui::Color32 = egui::Color32::from_rgb(110, 255, 150);
const DIM: egui::Color32 = egui::Color32::from_rgb(60, 140, 85);
const GREEN: egui::Color32 = egui::Color32::from_rgb(170, 255, 190);
const RED: egui::Color32 = egui::Color32::from_rgb(255, 95, 80);
const YELLOW: egui::Color32 = egui::Color32::from_rgb(255, 205, 80);
const TEXT: egui::Color32 = egui::Color32::from_rgb(150, 235, 170);
const PANEL: egui::Color32 = egui::Color32::from_rgba_premultiplied(2, 10, 4, 220);
const SCANLINE: egui::Color32 = egui::Color32::from_rgba_premultiplied(0, 0, 0, 70);

fn style_once(ctx: &egui::Context, done: &mut bool) {
    if *done {
        return;
    }
    // Retro terminal font (VT323, OFL) for everything but code, which stays in egui's monospace.
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "vt323".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/VT323-Regular.ttf"
        ))),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "vt323".to_owned());
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
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(20, 70, 35));
    v.widgets.inactive.bg_fill = egui::Color32::from_rgb(4, 24, 10);
    v.widgets.hovered.bg_fill = egui::Color32::from_rgb(10, 50, 22);
    v.widgets.active.bg_fill = egui::Color32::from_rgb(14, 70, 30);
    v.selection.bg_fill = egui::Color32::from_rgb(10, 80, 35);
    v.selection.stroke = egui::Stroke::new(1.0, CYAN);
    v.hyperlink_color = CYAN;
    ctx.set_visuals(v);
    *done = true;
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

#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut contexts: EguiContexts,
    mut store: ResMut<Store>,
    mut settings: ResMut<Settings>,
    bridge: Res<BridgeRes>,
    diagnostics: Res<DiagnosticsStore>,
    cams: Query<&PanOrbitCamera>,
    mut styled: Local<bool>,
    mut last_scale: Local<f32>,
    mut last_panel: Local<Option<String>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    style_once(ctx, &mut styled);
    if (*last_scale - settings.ui_scale).abs() > 1e-3 {
        ctx.set_zoom_factor(settings.ui_scale);
        *last_scale = settings.ui_scale;
    }
    let now = now_millis();
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
    let hud = egui::Panel::top("hud").show(&mut root, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("AGENT GLOBE").color(CYAN).strong().size(18.0));
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
                    ui.label(
                        egui::RichText::new(format!("{}  {}", s.project, short(&s.id)))
                            .color(YELLOW)
                            .monospace(),
                    );
                    ui.separator();
                    ui.label(egui::RichText::new(format!("⏱ {}", dur(s.last_activity - s.started))).monospace());
                    ui.label(egui::RichText::new(format!("agents {}", s.agents.len())).monospace());
                    ui.label(egui::RichText::new(format!("active {active}")).monospace().color(CYAN));
                    ui.label(egui::RichText::new(format!("calls {}", s.total_calls())).monospace());
                    ui.label(egui::RichText::new(format!("{}/min", s.calls_since(now - 60_000))).monospace());
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⚙ settings").clicked() {
                    toggle_settings = true;
                }
                if ui.button("⌂ home").clicked() {
                    go_home = true;
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
    if go_home {
        store.selected = None;
        store.reset_view = true;
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
                // Group by project, keeping newest-first order of first appearance.
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
                    let header = egui::RichText::new(project).color(YELLOW).small();
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
                                let rt = if active { rt.color(CYAN) } else { rt };
                                if ui.selectable_label(is_current, rt).clicked() {
                                    open = Some(s.clone());
                                }
                                if is_current && let Some(session) = store.sessions.get(&s.id) {
                                    agent_tree(
                                        ui,
                                        session,
                                        agent_globe::data::model::ROOT_AGENT,
                                        0,
                                        now,
                                        store.selected.as_deref(),
                                        &mut pick,
                                    );
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
    let cam_focus = cams.iter().next().map(|c| c.focus).unwrap_or(Vec3::ZERO);
    let bottom = egui::Panel::bottom("status").show(&mut root, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{fps:5.0} fps")).monospace().color(DIM));
            ui.label(
                egui::RichText::new(format!(
                    "cam {cam_dist:4.1} @ {:.1},{:.1},{:.1}",
                    cam_focus.x, cam_focus.y, cam_focus.z
                ))
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
                egui::RichText::new("drag rotate · scroll zoom · right-drag pan · click node · R reset")
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
    let mut inset_right = 0.0_f32;
    let just_opened = selected.as_ref().map(|a| &a.id) != last_panel.as_ref();
    *last_panel = selected.as_ref().map(|a| a.id.clone());
    if let Some(agent) = selected.as_ref() {
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
                agent_panel(ui, agent, now, &mut store.expanded_calls, just_opened);
            });
        inset_right = right.response.rect.width();
        ui_rects.push(right.response.rect);
    }
    if close {
        store.selected = None;
    }
    // Hover: a small card at the top right that never steals the pointer.
    if let Some(agent) = hovered
        .as_ref()
        .filter(|h| selected.as_ref().map(|s| &s.id) != Some(&h.id))
    {
        egui::Area::new(egui::Id::new("hover-card"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 16.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(PANEL)
                    .stroke(egui::Stroke::new(1.0, CYAN))
                    .inner_margin(10.0)
                    .corner_radius(6.0)
                    .show(ui, |ui| {
                        ui.set_max_width(320.0);
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

    // ---- toasts ----
    if !store.toasts.is_empty() {
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -40.0))
            .show(ctx, |ui| {
                for t in store.toasts.iter().rev() {
                    let alpha = (1.0 - t.born.elapsed().as_secs_f32() / 7.0).clamp(0.0, 1.0);
                    let color = match t.kind {
                        ToastKind::Spawn => CYAN,
                        ToastKind::Done => GREEN,
                        ToastKind::Error => RED,
                        ToastKind::Info => DIM,
                    };
                    egui::Frame::NONE
                        .fill(PANEL.gamma_multiply(alpha))
                        .stroke(egui::Stroke::new(1.0, color.gamma_multiply(alpha)))
                        .inner_margin(8.0)
                        .corner_radius(6.0)
                        .show(ui, |ui| {
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

const MAX_LINES: usize = 10;

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

fn agent_panel(ui: &mut egui::Ui, a: &Agent, now: i64, expanded: &mut HashSet<String>, jump_to_end: bool) {
    let status = a.status(now, theme::ACTIVE_WINDOW_MS);
    let (label, color) = match status {
        Status::Active => ("ACTIVE", CYAN),
        Status::Idle => ("IDLE", DIM),
        Status::Done => ("DONE", GREEN),
    };
    let title = if a.depth == 0 {
        "core · root session".to_string()
    } else if a.name.is_empty() {
        format!("agent {}", short(&a.id))
    } else {
        a.name.clone()
    };
    ui.horizontal(|ui| {
        ui.heading(egui::RichText::new(title).color(CYAN));
        ui.label(egui::RichText::new(label).color(color).strong());
    });
    ui.horizontal_wrapped(|ui| {
        let meta = |ui: &mut egui::Ui, k: &str, v: String| {
            ui.label(egui::RichText::new(k).small().color(DIM));
            ui.label(egui::RichText::new(v).small().monospace());
        };
        meta(ui, "id", short(&a.id).to_string());
        meta(ui, "kind", if a.kind.is_empty() { "-".into() } else { a.kind.clone() });
        meta(
            ui,
            "model",
            if a.model.is_empty() {
                "-".into()
            } else {
                a.model.clone()
            },
        );
        meta(ui, "depth", a.depth.to_string());
        let end = a.ended.unwrap_or(a.last_activity);
        meta(ui, "duration", dur(end - a.started));
        meta(ui, "calls", a.calls.len().to_string());
        meta(ui, "tokens out", a.output_tokens.to_string());
    });
    let open_calls = a.calls.iter().filter(|c| c.ended.is_none()).count();
    let done_frac = if a.calls.is_empty() {
        0.0
    } else {
        1.0 - open_calls as f32 / a.calls.len() as f32
    };
    let fill = if open_calls > 0 {
        egui::Color32::from_rgb(30, 150, 70)
    } else {
        egui::Color32::from_rgb(18, 70, 36)
    };
    ui.add(
        egui::ProgressBar::new(done_frac)
            .fill(fill)
            .text(egui::RichText::new(format!("{open_calls} open")).small()),
    );
    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            if !a.prompt.is_empty() {
                ui.collapsing(egui::RichText::new("prompt").color(YELLOW), |ui| {
                    code_block(ui, &format!("{}:prompt", a.id), &a.prompt, TEXT, expanded);
                });
            }
            ui.label(egui::RichText::new("TOOL CALLS").small().color(DIM));
            for (i, c) in a.calls.iter().enumerate() {
                let open = c.ended.is_none();
                let mark = if c.is_error {
                    egui::RichText::new("✖").color(RED)
                } else if open {
                    egui::RichText::new("●").color(CYAN)
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
                if let Some(cmd) = &c.command {
                    egui::Frame::NONE
                        .fill(egui::Color32::from_rgb(1, 8, 3))
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(20, 90, 40)))
                        .inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(format!("$ {cmd}")).monospace().color(CYAN));
                            if !c.stdout.is_empty() {
                                code_block(ui, &c.id, &c.stdout, TEXT, expanded);
                            }
                            if !c.stderr.is_empty() {
                                code_block(ui, &format!("{}:err", c.id), &c.stderr, RED, expanded);
                            }
                        });
                } else if !c.stdout.is_empty() {
                    egui::Frame::NONE
                        .fill(egui::Color32::from_rgb(1, 8, 3))
                        .inner_margin(6.0)
                        .show(ui, |ui| {
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
    let status = a.status(now, theme::ACTIVE_WINDOW_MS);
    let (label, color) = match status {
        Status::Active => ("ACTIVE", CYAN),
        Status::Idle => ("IDLE", DIM),
        Status::Done => ("DONE", GREEN),
    };
    let title = if a.depth == 0 {
        "core · root session".to_string()
    } else if a.name.is_empty() {
        format!("agent {}", short(&a.id))
    } else {
        a.name.clone()
    };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).color(CYAN).strong());
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
            Status::Active => ("●", CYAN),
            Status::Idle => ("○", DIM),
            Status::Done => ("✔", GREEN),
        };
        let name = if a.depth == 0 {
            "core".to_string()
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
