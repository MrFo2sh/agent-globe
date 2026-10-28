//! egui layer: HUD (top), sessions (left), selected agent (right), status (bottom), toasts,
//! hover card, settings and debrief windows. Reports whether the pointer is over any of these so
//! the camera can ignore input that belongs to the UI.

use super::debrief;
use super::gitinfo::{self, FileChange, GitInfo, GitWatch};
use super::probe::Probe;
use super::settings::Settings;
use super::sounds::{Sfx, Sounds};
use super::state::{BadgeKind, BridgeRes, Store, ToastKind, Viewer};
use super::theme;
use super::winjump;
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
    mut docs: Local<HashMap<String, Option<String>>>,
    mut styled: Local<bool>,
    mut last_scale: Local<f32>,
    mut last_panel: Local<Option<String>>,
) -> Result {
    let mut jump: Option<String> = None;
    let ctx = contexts.ctx_mut()?;
    if style_once(ctx, &mut styled) {
        return Ok(());
    }
    if (*last_scale - settings.ui_scale).abs() > 1e-3 {
        ctx.set_zoom_factor(settings.ui_scale);
        *last_scale = settings.ui_scale;
    }
    let now = now_millis();
    let plan_api = settings.plan == "api";
    let compact = settings.compact || ctx.viewport_rect().width() < 1100.0;
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
        draw_progress(ctx, &store, &settings, now);
        draw_satellites(ctx, &mut store, now);
        draw_bubbles(ctx, &mut store, &probe, git.as_ref(), &mut images, now);
        draw_viewer(ctx, &mut store, &mut images, &mut docs);
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
                    if jump_button(ui).clicked() {
                        jump = Some(winjump::needle_for(&s.cwd, &s.project));
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
                    let spent = s.cost_usd();
                    let spent_color = if settings.budget_usd > 0.0 && spent >= settings.budget_usd as f64 { RED } else { SAND };
                    tip(
                        ui,
                        egui::RichText::new(money(spent, plan_api)).monospace().color(spent_color),
                        if plan_api {
                            "Estimated spend for this session from token counts and API list prices (input, cache reads, cache writes, output).\nSet a budget alarm in settings."
                        } else {
                            "API-equivalent value of this session's tokens. On a Pro or Max plan you do not pay this: it shows how much work the plan absorbed, at API list prices.\nSwitch to API billing in settings if you pay per token."
                        },
                    );
                    if !plan_api {
                        let five_h = store.spent_since(now, 5 * 3_600_000);
                        tip(
                            ui,
                            egui::RichText::new(format!("5h {}", money(five_h, false))).monospace().color(DIM),
                            "API-equivalent value of everything your watched sessions did in the last five hours: the window Pro and Max limits are measured on.",
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
                            if jump_button(ui).clicked() {
                                let cwd = store.sessions.get(&a.session_id).map(|s| s.cwd.clone()).unwrap_or_default();
                                jump = Some(winjump::needle_for(&cwd, &a.project));
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
                                    if (is_current || store.watched.contains(&s.id)) && jump_button(ui).clicked() {
                                        let cwd = store.sessions.get(&s.id).map(|x| x.cwd.clone()).unwrap_or_default();
                                        jump = Some(winjump::needle_for(&cwd, &s.project));
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
    if let Some(needle) = jump.take()
        && !winjump::focus(&needle)
    {
        store.toast(ToastKind::Warn, format!("no editor window titled \"{needle}\" found"));
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
                    "drag · scroll · R home · F"
                } else {
                    "drag rotate · scroll zoom · right-drag pan · click unit · R home · F cinematic"
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
    let mut view_diffs: Option<(String, Vec<String>)> = None;
    let mut query = std::mem::take(&mut store.artifact_query);
    let mut history = std::mem::take(&mut store.history_query);
    let mut inset_right = 0.0_f32;
    let just_opened = selected.as_ref().map(|a| &a.id) != last_panel.as_ref();
    *last_panel = selected.as_ref().map(|a| a.id.clone());
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
        let right = egui::Panel::right("agent")
            .resizable(true)
            .default_size(if compact { vw * 0.55 } else { 520.0 })
            .max_size(vw * if compact { 0.8 } else { 0.6 })
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
                    &mut query,
                    &changes,
                    &cwd,
                    &touched,
                    &mut view_diffs,
                    &mut history,
                    plan_api,
                );
            });
        inset_right = right.response.rect.width();
        ui_rects.push(right.response.rect);
    }
    store.artifact_query = query;
    store.history_query = history;
    if close {
        store.selected = None;
    }
    if let Some(path) = view_image.take() {
        open_viewer(&mut store, &path);
    }
    if let Some((path, items)) = view_diffs.take() {
        let index = items.iter().position(|p| p == &path).unwrap_or(0);
        store.viewer = Some(Viewer {
            items,
            index,
            zoom: 1.0,
            offset: (0.0, 0.0),
        });
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
                ui.checkbox(&mut settings.progress_rings, "progress rings and hints under units");
                ui.checkbox(
                    &mut settings.satellites,
                    "other active sessions as satellites around this one",
                );
                ui.checkbox(&mut settings.compact, "compact layout (also automatic under 1100 px wide)");
                ui.separator();
                ui.label(egui::RichText::new("BILLING").small().color(DIM));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut settings.plan, "subscription".to_string(), "Pro / Max subscription")
                        .on_hover_text("Dollar figures are API-equivalent, shown with ≈: what the plan absorbed, not what you pay.");
                    ui.selectable_value(&mut settings.plan, "api".to_string(), "API, pay per token")
                        .on_hover_text("Dollar figures are what you will be billed at list prices.");
                });
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
    draw_progress(ctx, &store, &settings, now);
    draw_satellites(ctx, &mut store, now);
    draw_bubbles(ctx, &mut store, &probe, git.as_ref(), &mut images, now);
    if let Some(rect) = draw_viewer(ctx, &mut store, &mut images, &mut docs) {
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
fn badge_style(kind: BadgeKind) -> (&'static str, egui::Color32, &'static str) {
    match kind {
        BadgeKind::Done => ("!", egui::Color32::from_rgb(255, 215, 90), "done"),
        BadgeKind::Waiting => ("💬", ACCENT, "awaiting your orders"),
        BadgeKind::Error => ("⚠", RED, "failed call"),
        BadgeKind::Approval => ("🛡", AMBER, "authorization needed"),
        BadgeKind::Stalled => ("⏳", AMBER, "stalled"),
    }
}

fn jump_button(ui: &mut egui::Ui) -> egui::Response {
    ui.small_button("🖥")
        .on_hover_text("Bring this session's editor window (VS Code) to the front")
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
        let (state, color) = if root.waiting_for_user {
            ("💬 awaiting orders".to_string(), ACCENT)
        } else if active > 0 {
            (format!("active · {active} unit(s) moving"), ACCENT)
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
            "{} units · {active} active · {} calls · {} · {}",
            s.agents.len(),
            s.total_calls(),
            dur(s.last_activity - s.started),
            cost::fmt_usd(s.cost_usd())
        ))
        .small()
        .monospace(),
    );
    let root = s.root();
    if root.waiting_for_user {
        ui.label(
            egui::RichText::new("💬 commander is waiting for your orders")
                .small()
                .color(ACCENT),
        );
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

fn draw_badges(ctx: &egui::Context, store: &Store) {
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
    for b in &store.badges {
        if !scene.contains(egui::pos2(b.x, b.y)) {
            continue;
        }
        let (glyph, color, label) = badge_style(b.kind);
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

/// One tool call in the history list: time, status mark, header, narrator line, command and output.
fn call_row(
    ui: &mut egui::Ui,
    i: usize,
    c: &agent_globe::data::model::ToolCall,
    stalled: &HashSet<String>,
    narrator: bool,
    expanded: &mut HashSet<String>,
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

/// Dollar label: exact on API billing, API-equivalent (≈) on a Pro / Max subscription.
fn money(usd: f64, plan_api: bool) -> String {
    if plan_api {
        cost::fmt_usd(usd)
    } else {
        format!("≈{}", cost::fmt_usd(usd))
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
    query: &mut String,
    changes: &[FileChange],
    cwd: &str,
    touched: &HashMap<String, usize>,
    view_diffs: &mut Option<(String, Vec<String>)>,
    history: &mut String,
    plan_api: bool,
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
        let folded = dedup_artifacts(&a.artifacts);
        let total = folded.len();
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
        if all {
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
        }
        let needle = if all { query.to_ascii_lowercase() } else { String::new() };
        egui::ScrollArea::vertical()
            .id_salt(&key)
            .max_height(shelf_h)
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
        if toggle {
            if all {
                expanded.remove(&key);
            } else {
                expanded.insert(key);
            }
        }
    }
    if !changes.is_empty() {
        ui.add_space(4.0);
        let key = format!("{}:changes", a.id);
        let all = expanded.contains(&key);
        let (add, del) = changes.iter().fold((0, 0), |(x, y), c| (x + c.added, y + c.deleted));
        let mut toggle = false;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("CHANGES").small().color(DIM))
                .on_hover_text("Files the working tree changed against the last commit. Click one to read its diff here; ✎ counts edits this session made to it.");
            ui.label(
                egui::RichText::new(format!("{} files  +{add} −{del}", changes.len()))
                    .small()
                    .color(SAND),
            );
            let label = if all { "▴ collapse" } else { "▾ expand" };
            if ui.small_button(egui::RichText::new(label).small()).clicked() {
                toggle = true;
            }
        });
        let row_h = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
        let max_h = if all { 360.0 } else { row_h * 3.0 + 4.0 };
        let all_paths: Vec<String> = changes.iter().map(|c| diff_path(cwd, &c.path, c.untracked)).collect();
        egui::ScrollArea::vertical()
            .id_salt(&key)
            .max_height(max_h)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (i, c) in changes.iter().enumerate() {
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
                            .on_hover_text("read the diff")
                            .clicked()
                        {
                            *view_diffs = Some((all_paths[i].clone(), all_paths.clone()));
                        }
                        if let Some(n) = touched.get(&c.path) {
                            ui.label(egui::RichText::new(format!("✎ {n}")).small().color(DIM))
                                .on_hover_text("edits by this session");
                        }
                        if ui.small_button("open").on_hover_text("open in VS Code").clicked() {
                            open_in_editor(cwd, &c.path);
                        }
                    });
                }
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
                    code_block(ui, &format!("{}:prompt", a.id), &a.prompt, TEXT, expanded);
                });
            }
            ui.label(egui::RichText::new("TOOL CALLS").small().color(DIM));
            if !needle.is_empty() && hits.is_empty() {
                ui.label(
                    egui::RichText::new(format!("nothing in this unit's history matches \"{}\"", history.trim()))
                        .color(DIM),
                );
            }
            if a.turn_log.is_empty() {
                for i in hits.iter().copied() {
                    call_row(ui, i, &a.calls[i], stalled, narrator, expanded);
                }
            } else {
                // Grouped by your prompts: each turn folds its calls, time and cost.
                let hit_set: HashSet<usize> = hits.iter().copied().collect();
                let n = a.turn_log.len();
                let lead: Vec<usize> = (0..a.turn_log[0].first_call.min(a.calls.len()))
                    .filter(|i| hit_set.contains(i))
                    .collect();
                if !lead.is_empty() {
                    egui::CollapsingHeader::new(
                        egui::RichText::new(format!("before the first prompt · {} calls", lead.len()))
                            .small()
                            .color(DIM),
                    )
                    .id_salt((&a.id, "lead"))
                    .default_open(false)
                    .show(ui, |ui| {
                        for i in lead {
                            call_row(ui, i, &a.calls[i], stalled, narrator, expanded);
                        }
                    });
                }
                for ti in 0..n {
                    let t = &a.turn_log[ti];
                    let range = a.turn_range(ti);
                    let idx: Vec<usize> = range.clone().filter(|i| hit_set.contains(i)).collect();
                    if !needle.is_empty() && idx.is_empty() {
                        continue;
                    }
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
                    let cost = money(a.turn_cost_usd(t), plan_api);
                    let title = format!("#{}  {}  {}", ti + 1, clock(t.started), first_line);
                    egui::CollapsingHeader::new(egui::RichText::new(title).color(SAND))
                        .id_salt((&a.id, ti))
                        .default_open(ti + 1 == n || !needle.is_empty())
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} calls · {} units · {} · {}",
                                        range.len(),
                                        units,
                                        dur(took),
                                        cost
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
                                    code_block(ui, &format!("{}:turn{ti}", a.id), &t.prompt, TEXT, expanded);
                                });
                            }
                            for i in idx {
                                call_row(ui, i, &a.calls[i], stalled, narrator, expanded);
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

/// Viewer path for a diff: `diff:<cwd>|<repo path>|<1 if untracked>`. `|` cannot appear in paths.
fn diff_path(cwd: &str, path: &str, untracked: bool) -> String {
    format!("diff:{cwd}|{path}|{}", u8::from(untracked))
}

fn parse_diff_path(p: &str) -> Option<(&str, &str, bool)> {
    let rest = p.strip_prefix("diff:")?;
    let mut it = rest.split('|');
    let (cwd, path, flag) = (it.next()?, it.next()?, it.next().unwrap_or("0"));
    Some((cwd, path, flag == "1"))
}

fn open_in_editor(cwd: &str, path: &str) {
    let full = std::path::Path::new(cwd).join(path).display().to_string();
    std::thread::spawn(move || {
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/C", "code", "-g", &full]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let _ = cmd.status();
    });
}

/// A unified diff with the usual colours: additions green, removals red, hunk headers amber.
fn render_diff(ui: &mut egui::Ui, text: &str) {
    for line in text.lines() {
        let color = if line.starts_with("+++") || line.starts_with("---") {
            SAND
        } else if line.starts_with('+') {
            GREEN
        } else if line.starts_with('-') {
            RED
        } else if line.starts_with("@@") {
            AMBER
        } else if line.starts_with("diff ") || line.starts_with("index ") {
            DIM
        } else {
            TEXT
        };
        ui.add(
            egui::Label::new(egui::RichText::new(line).monospace().color(color))
                .wrap_mode(egui::TextWrapMode::Extend)
                .selectable(true),
        );
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
        ArtifactKind::File if doc_kind(&a.target) != DocKind::Other => return Some(a.target.clone()),
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DocKind {
    Image,
    Markdown,
    Pdf,
    Other,
    Diff,
}

fn doc_kind(path: &str) -> DocKind {
    if path.starts_with("diff:") {
        return DocKind::Diff;
    }
    let p = path.to_ascii_lowercase();
    if [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]
        .iter()
        .any(|e| p.ends_with(e))
    {
        DocKind::Image
    } else if p.ends_with(".md") || p.ends_with(".markdown") {
        DocKind::Markdown
    } else if p.ends_with(".pdf") {
        DocKind::Pdf
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
                        .filter(|x| doc_kind(&x.target) != DocKind::Other)
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

/// Text of a markdown or pdf file, read once. PDFs give extracted text with page breaks.
fn doc_text<'a>(docs: &'a mut HashMap<String, Option<String>>, path: &str) -> Option<&'a str> {
    if !docs.contains_key(path) {
        let loaded = match doc_kind(path) {
            DocKind::Pdf => pdf_extract::extract_text(path).ok(),
            DocKind::Diff => parse_diff_path(path).and_then(|(cwd, rel, new)| gitinfo::diff_text(cwd, rel, new)),
            _ => std::fs::read_to_string(path).ok(),
        };
        docs.insert(path.to_string(), loaded);
    }
    docs.get(path).and_then(|t| t.as_deref())
}

/// A small markdown renderer: headings, bullets, numbered lists, fenced code, inline code, bold.
fn render_markdown(ui: &mut egui::Ui, text: &str) {
    let mut in_code = false;
    let mut code = String::new();
    for raw in text.lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            if in_code {
                egui::Frame::NONE.fill(INK).inner_margin(6.0).show(ui, |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(code.trim_end()).monospace())
                            .wrap()
                            .selectable(true),
                    );
                });
                code.clear();
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            code.push_str(raw);
            code.push('\n');
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            ui.add_space(6.0);
        } else if let Some(h) = trimmed.strip_prefix("### ") {
            ui.label(egui::RichText::new(h).strong().color(SAND));
        } else if let Some(h) = trimmed.strip_prefix("## ") {
            ui.label(egui::RichText::new(h).font(stencil(15.0)).color(SAND));
        } else if let Some(h) = trimmed.strip_prefix("# ") {
            ui.label(egui::RichText::new(h).font(stencil(18.0)).color(ACCENT));
        } else if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            ui.horizontal_wrapped(|ui| {
                ui.add_space((line.len() - trimmed.len()) as f32 * 4.0);
                ui.label(egui::RichText::new("•").color(SAND));
                inline_markdown(ui, item);
            });
        } else if trimmed.starts_with('|') {
            ui.label(egui::RichText::new(trimmed).monospace().small());
        } else {
            ui.horizontal_wrapped(|ui| inline_markdown(ui, trimmed));
        }
    }
    if in_code && !code.is_empty() {
        ui.add(
            egui::Label::new(egui::RichText::new(code.trim_end()).monospace())
                .wrap()
                .selectable(true),
        );
    }
}

/// `code` and **bold** inside a line.
fn inline_markdown(ui: &mut egui::Ui, text: &str) {
    ui.spacing_mut().item_spacing.x = 0.0;
    let mut rest = text;
    loop {
        let code_at = rest.find('`');
        let bold_at = rest.find("**");
        let next = match (code_at, bold_at) {
            (None, None) => None,
            (Some(c), None) => Some((c, false)),
            (None, Some(b)) => Some((b, true)),
            (Some(c), Some(b)) => Some(if c <= b { (c, false) } else { (b, true) }),
        };
        let Some((at, is_bold)) = next else {
            ui.label(rest);
            return;
        };
        ui.label(&rest[..at]);
        let marker = if is_bold { "**" } else { "`" };
        let after = &rest[at + marker.len()..];
        match after.find(marker) {
            Some(end) => {
                let inner = &after[..end];
                if is_bold {
                    ui.label(egui::RichText::new(inner).strong());
                } else {
                    ui.label(egui::RichText::new(inner).monospace().color(ACCENT));
                }
                rest = &after[end + marker.len()..];
            }
            None => {
                ui.label(after);
                return;
            }
        }
    }
}

/// Centered, movable, resizable viewer. Images: drag to pan, scroll to zoom. Markdown is rendered,
/// PDFs show extracted text. ◀ ▶ or arrow keys browse the unit's items, Esc or ✕ closes.
fn draw_viewer(
    ctx: &egui::Context,
    store: &mut Store,
    images: &mut HashMap<String, Option<egui::TextureHandle>>,
    docs: &mut HashMap<String, Option<String>>,
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
    let default_size = egui::vec2(vr.width() * 0.7, vr.height() * 0.75);
    let mut close = false;
    let mut open_ext = false;
    let win = egui::Window::new("viewer")
        .title_bar(false)
        .default_pos(vr.center() - default_size / 2.0)
        .default_size(default_size)
        .resizable(true)
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
                let name = match parse_diff_path(&path) {
                    Some((_, rel, _)) => format!("Δ {rel}"),
                    None => path.rsplit(['/', '\\']).next().unwrap_or("").to_string(),
                };
                ui.add(egui::Label::new(egui::RichText::new(name).font(stencil(14.0)).color(SAND)).truncate())
                    .on_hover_text(&path);
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
                        step(&mut viewer, 1);
                    }
                    if n > 1 && ui.button("◀").clicked() {
                        step(&mut viewer, -1);
                    }
                    if kind == DocKind::Image {
                        ui.label(
                            egui::RichText::new(format!(
                                "{:.0}%  · drag to pan · scroll to zoom · drag the corner to resize",
                                viewer.zoom * 100.0
                            ))
                            .small()
                            .color(DIM),
                        );
                    } else {
                        ui.label(egui::RichText::new("drag the corner to resize").small().color(DIM));
                    }
                });
            });
            ui.add_space(6.0);
            let avail = ui.available_size();
            match kind {
                DocKind::Image => match full_image(ctx, images, &path) {
                    Some(tex) => {
                        let fitted = fit(tex.size_vec2(), avail.x.min(avail.y)) * viewer.zoom;
                        let out = egui::ScrollArea::both()
                            .id_salt("viewer-scroll")
                            .scroll_offset(egui::vec2(viewer.offset.0, viewer.offset.1))
                            .max_width(avail.x)
                            .max_height(avail.y)
                            .show(ui, |ui| {
                                ui.add(egui::Image::new(egui::load::SizedTexture::new(tex.id(), fitted)))
                                    .interact(egui::Sense::drag())
                            });
                        let resp = out.inner;
                        let mut offset = out.state.offset;
                        if resp.dragged() {
                            offset -= resp.drag_delta();
                        }
                        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                        if (resp.hovered()
                            || out
                                .inner_rect
                                .contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default())))
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
                DocKind::Markdown | DocKind::Pdf | DocKind::Other | DocKind::Diff => {
                    let body = doc_text(docs, &path).map(str::to_owned);
                    egui::ScrollArea::vertical()
                        .id_salt("viewer-doc")
                        .max_width(avail.x)
                        .max_height(avail.y)
                        .show(ui, |ui| {
                            ui.set_min_width(avail.x - 20.0);
                            match (kind, body) {
                                (DocKind::Markdown, Some(t)) => render_markdown(ui, &t),
                                (DocKind::Diff, Some(t)) => render_diff(ui, &t),
                                (_, Some(t)) => {
                                    for (i, page) in t.split('\u{c}').enumerate() {
                                        if i > 0 {
                                            ui.separator();
                                            ui.label(egui::RichText::new(format!("page {}", i + 1)).small().color(DIM));
                                        }
                                        ui.add(
                                            egui::Label::new(egui::RichText::new(page.trim()).monospace())
                                                .wrap()
                                                .selectable(true),
                                        );
                                    }
                                }
                                (_, None) => {
                                    ui.label(egui::RichText::new(format!("cannot read {path}")).color(RED));
                                }
                            }
                        });
                }
            }
        })?;
    if open_ext {
        match parse_diff_path(&path) {
            Some((cwd, rel, _)) => open_in_editor(cwd, rel),
            None => {
                let p = path.clone();
                std::thread::spawn(move || {
                    let _ = open::that(&p);
                });
            }
        }
    }
    if close {
        store.viewer = None;
    } else {
        store.viewer = Some(viewer);
    }
    Some(win.response.rect)
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
        // Hint: the in-progress task, else the running call, else idle.
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
        } else if active {
            "thinking".into()
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
            None if busy => {
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
        };
        assert!(call_matches(&c, "CARGO"));
        assert!(call_matches(&c, "12 passed"));
        assert!(!call_matches(&c, "clippy"));
        c.stderr = "warning: unused import".into();
        assert!(call_matches(&c, "unused"));
        assert!(contains_ci("Héllo", "héllo"), "non-ascii bytes compare exactly");
    }

    #[test]
    fn diff_paths_round_trip() {
        let p = diff_path("D:\\repo", "src/app/ui.rs", true);
        assert_eq!(doc_kind(&p), DocKind::Diff);
        assert_eq!(parse_diff_path(&p), Some(("D:\\repo", "src/app/ui.rs", true)));
        assert_eq!(doc_kind("notes.md"), DocKind::Markdown);
    }
}
