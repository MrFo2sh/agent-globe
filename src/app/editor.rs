//! The files card (tree + finder) and the editor card (tabs over the WebView surface). Both are
//! ordinary cards: they float, dock, maximize and close like the rest. The editor's body only
//! reserves a rectangle; `webview.rs` puts the real surface there on the main thread.

use super::files::{DocKind, FileIndex, kind_of};
use super::settings::Settings;
use super::state::{EditorCmd, Store};
use super::ui::{ACCENT, AMBER, DIM, INK, SAND, TEXT, begin_card, end_card, floating_card, open_in_editor, stencil};
use bevy_egui::egui;
use std::collections::HashSet;

fn file_name(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .find(|p| !p.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn icon_for(kind: DocKind) -> &'static str {
    match kind {
        DocKind::Markdown => "📄",
        DocKind::Pdf => "📕",
        DocKind::Docx => "📘",
        DocKind::Image => "🖼",
        DocKind::Code => "·",
    }
}

/// The finder's row: the path with the matched characters lit.
fn hit_label(path: &str, hits: &[usize]) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let font = egui::FontId::monospace(13.0);
    let plain = egui::TextFormat {
        font_id: font.clone(),
        color: TEXT,
        ..Default::default()
    };
    let lit = egui::TextFormat {
        font_id: font,
        color: ACCENT,
        ..Default::default()
    };
    let set: HashSet<usize> = hits.iter().copied().collect();
    for (i, ch) in path.chars().enumerate() {
        let mut s = String::new();
        s.push(ch);
        job.append(&s, 0.0, if set.contains(&i) { lit.clone() } else { plain.clone() });
    }
    job
}

#[allow(clippy::too_many_arguments)]
fn tree(
    ui: &mut egui::Ui,
    index: &mut FileIndex,
    dir: &str,
    depth: usize,
    open_dirs: &HashSet<String>,
    toggle: &mut Option<String>,
    open: &mut Option<(String, bool)>,
    reveal: &mut Option<String>,
) {
    for e in index.list(dir) {
        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 14.0);
            if e.is_dir {
                let is_open = open_dirs.contains(&e.path);
                let label = format!("{} {}", if is_open { "▾" } else { "▸" }, e.name);
                if ui
                    .add(egui::Button::new(egui::RichText::new(label).color(SAND)).frame(false))
                    .clicked()
                {
                    *toggle = Some(e.path.clone());
                }
            } else {
                let kind = kind_of(&e.path);
                let r = ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(format!("{} {}", icon_for(kind), e.name))
                                .monospace()
                                .color(TEXT),
                        )
                        .frame(false),
                    )
                    .on_hover_text(format!(
                        "{}\nclick: select · click again: open · double-click: keep open · right-click: VS Code",
                        e.path
                    ));
                if r.double_clicked() {
                    *open = Some((e.path.clone(), false));
                } else if r.clicked() {
                    *open = Some((e.path.clone(), true));
                }
                if r.secondary_clicked() {
                    *reveal = Some(e.path.clone());
                }
            }
        });
        if e.is_dir && open_dirs.contains(&e.path) && depth < 12 {
            tree(ui, index, &e.path, depth + 1, open_dirs, toggle, open, reveal);
        }
    }
}

/// What the files sidebar asked for this frame.
#[derive(Default)]
pub struct FilesActions {
    pub collapse: bool,
}

/// The files sidebar's content: the session folder as a tree, with a finder box on top (Ctrl+P
/// from anywhere expands the sidebar and focuses it). Click opens a file in the editor card;
/// right-click reveals it in VS Code.
pub fn files_body(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    store: &mut Store,
    index: &mut FileIndex,
    cwd: &str,
    project: &str,
) -> FilesActions {
    let mut act = FilesActions::default();
    index.ensure(cwd, false);
    let building = index.is_building();
    let count = index.len();
    let mut refresh = false;
    let mut open_path: Option<(String, bool)> = None;
    let mut reveal: Option<String> = None;
    let mut toggle_dir: Option<String> = None;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("🗂").size(14.0));
        ui.label(egui::RichText::new(project).font(stencil(13.0)).color(SAND));
        if building {
            ui.spinner();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("⟨")
                .on_hover_text("Collapse the files sidebar")
                .clicked()
            {
                act.collapse = true;
            }
            if ui.small_button("↻").on_hover_text("Rebuild the file index").clicked() {
                refresh = true;
            }
        });
    });
    ui.horizontal(|ui| {
        let focus = std::mem::take(&mut store.files_focus_search);
        let resp = ui.add(
            egui::TextEdit::singleline(&mut store.files_query)
                .id(egui::Id::new(("files-panel", "search")))
                .desired_width(ui.available_width() - 30.0)
                .font(egui::TextStyle::Monospace)
                .hint_text("🔍 find a file  (Ctrl+P)"),
        );
        if focus {
            resp.request_focus();
        }
        if !store.files_query.is_empty() && ui.small_button("⌫").on_hover_text("Clear").clicked() {
            store.files_query.clear();
        }
    });
    ui.separator();
    let q = store.files_query.trim().to_string();
    if !q.is_empty() {
        let hits = index.search(&q, 60);
        ui.label(
            egui::RichText::new(format!("{} of {count} files · Enter opens the first", hits.len()))
                .small()
                .color(DIM),
        );
        if ctx.input(|i| i.key_pressed(egui::Key::Enter))
            && let Some((p, _)) = hits.first()
        {
            open_path = Some((format!("{cwd}/{p}"), true));
        }
        egui::ScrollArea::vertical()
            .id_salt("files-finder")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width() - 8.0);
                for (p, idx) in &hits {
                    let r = ui.add(egui::Button::new(hit_label(p, idx)).frame(false)).on_hover_text(
                        "click: select · click again: open · double-click: keep open · right-click: VS Code",
                    );
                    if r.double_clicked() {
                        open_path = Some((format!("{cwd}/{p}"), false));
                    } else if r.clicked() {
                        open_path = Some((format!("{cwd}/{p}"), true));
                    }
                    if r.secondary_clicked() {
                        reveal = Some(format!("{cwd}/{p}"));
                    }
                }
            });
    } else {
        let open_dirs = store.files_open_dirs.clone();
        egui::ScrollArea::vertical()
            .id_salt("files-tree")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width() - 8.0);
                tree(
                    ui,
                    index,
                    cwd,
                    0,
                    &open_dirs,
                    &mut toggle_dir,
                    &mut open_path,
                    &mut reveal,
                );
            });
    }
    if refresh {
        index.ensure(cwd, true);
    }
    if let Some(d) = toggle_dir
        && !store.files_open_dirs.remove(&d)
    {
        store.files_open_dirs.insert(d);
    }
    if let Some((p, preview)) = open_path {
        if !preview {
            store.open_doc_pinned(&p);
        } else if store
            .selected_file
            .as_deref()
            .is_some_and(|s| super::scene::same_file(s, &p))
        {
            store.open_doc(&p);
        } else {
            store.selected_file = Some(p.clone());
            store.focus_file = Some(p);
        }
    }
    if let Some(p) = reveal {
        open_in_editor("", &p);
    }
    act
}

/// The quick-open palette (Ctrl+P): a box at the top, the best matches under it, arrows to move,
/// Enter to open, Esc to close. With an empty box it lists the files already open.
pub fn draw_quick_open(ctx: &egui::Context, store: &mut Store, index: &mut FileIndex) {
    if !store.quick_open {
        return;
    }
    let Some(cwd) = store.current_session().map(|s| s.cwd.clone()).filter(|c| !c.is_empty()) else {
        store.quick_open = false;
        return;
    };
    index.ensure(&cwd, false);
    // Taken before the box is drawn, so the box never moves its caret on them.
    let (esc, enter, up, down) = ctx.input_mut(|i| {
        let none = egui::Modifiers::NONE;
        (
            i.consume_key(none, egui::Key::Escape),
            i.consume_key(none, egui::Key::Enter),
            i.consume_key(none, egui::Key::ArrowUp),
            i.consume_key(none, egui::Key::ArrowDown),
        )
    });
    let query = store.quick_query.trim().to_string();
    let rows: Vec<(String, Vec<usize>, bool)> = if query.is_empty() {
        store.docs.iter().map(|d| (d.path.clone(), Vec::new(), true)).collect()
    } else {
        index
            .search(&query, 14)
            .into_iter()
            .map(|(p, idx)| (format!("{cwd}/{p}"), idx, false))
            .collect()
    };
    if up {
        store.quick_sel = store.quick_sel.saturating_sub(1);
    }
    if down && !rows.is_empty() {
        store.quick_sel = (store.quick_sel + 1).min(rows.len() - 1);
    }
    store.quick_sel = store.quick_sel.min(rows.len().saturating_sub(1));
    let mut chosen: Option<String> = None;
    let mut close = esc;
    let focus = std::mem::take(&mut store.quick_focus);
    let vr = ctx.viewport_rect();
    // On a wall of monitors the palette sits on the monitor under the pointer, never on a seam.
    let px = ctx.pointer_latest_pos().map(|p| p.x).unwrap_or(vr.min.x + 1.0);
    let (mut lo, mut hi) = (vr.min.x, vr.max.x);
    for s in &store.monitor_seams {
        let s = vr.min.x + *s;
        if s <= px {
            lo = lo.max(s);
        } else {
            hi = hi.min(s);
        }
    }
    let centre_x = (lo + hi) * 0.5;
    egui::Area::new(egui::Id::new("quick-open"))
        .order(egui::Order::Tooltip)
        .fixed_pos(egui::pos2(centre_x - 330.0, vr.min.y + 70.0))
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(egui::Color32::from_rgb(6, 12, 5))
                .stroke(egui::Stroke::new(1.5, ACCENT))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 6],
                    blur: 22,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(190),
                })
                .inner_margin(10.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.set_width(660.0);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut store.quick_query)
                            .id(egui::Id::new("quick-open-search"))
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace)
                            .hint_text("file name · ↑ ↓ · Enter opens · Esc closes"),
                    );
                    if focus {
                        resp.request_focus();
                    }
                    if resp.changed() {
                        store.quick_sel = 0;
                    }
                    ui.add_space(4.0);
                    if rows.is_empty() {
                        ui.label(
                            egui::RichText::new(if query.is_empty() {
                                "no files open · type to search"
                            } else {
                                "no matches"
                            })
                            .small()
                            .color(DIM),
                        );
                    }
                    for (i, (path, idx, is_open)) in rows.iter().enumerate() {
                        let selected = i == store.quick_sel;
                        let shown = if *is_open {
                            path.strip_prefix(&format!("{cwd}\\"))
                                .unwrap_or(path)
                                .replace('\\', "/")
                        } else {
                            path.strip_prefix(&format!("{cwd}/")).unwrap_or(path).to_string()
                        };
                        let job = hit_label(&shown, idx);
                        let r = ui.add(
                            egui::Button::new(job)
                                .frame(false)
                                .fill(if selected {
                                    egui::Color32::from_rgb(18, 58, 28)
                                } else {
                                    egui::Color32::TRANSPARENT
                                })
                                .min_size(egui::vec2(ui.available_width(), 0.0)),
                        );
                        if r.hovered() && ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                            store.quick_sel = i;
                        }
                        if r.clicked() {
                            chosen = Some(path.clone());
                        }
                    }
                });
        });
    if enter && let Some((path, _, _)) = rows.get(store.quick_sel) {
        chosen = Some(path.clone());
    }
    if let Some(p) = chosen {
        store.open_doc(&p);
        close = true;
    }
    if close {
        store.quick_open = false;
        store.quick_query.clear();
        store.quick_sel = 0;
    }
}

/// The editor card: tabs for the open files and the rectangle the WebView surface fills.
pub fn draw_editor_card(ctx: &egui::Context, store: &mut Store, settings: &mut Settings) -> Option<egui::Rect> {
    if !store.editor_open {
        store.webview_rect = None;
        return None;
    }
    let maxed = store.editor_max;
    let (mut cs, mut dock, grid) = begin_card(store, settings, "editor-card", ctx);
    let docs = store.docs.clone();
    let active = store.active_doc.filter(|i| *i < docs.len());
    let active_doc = active.and_then(|i| docs.get(i).cloned());
    let title = active_doc
        .as_ref()
        .map(|d| file_name(&d.path))
        .unwrap_or_else(|| "editor".to_string());
    let preview = store.md_preview;
    let mut switch: Option<usize> = None;
    let mut pin: Option<usize> = None;
    let mut close: Option<usize> = None;
    let mut save = false;
    let mut toggle_preview = false;
    let mut reveal = false;
    let mut surface: Option<egui::Rect> = None;
    let ppp = ctx.pixels_per_point();
    let vr = ctx.viewport_rect();
    let default_pos = egui::pos2(vr.center().x - 420.0, vr.min.y + 80.0);
    let out = floating_card(
        ctx,
        "editor-card",
        maxed,
        &mut cs,
        &mut dock,
        grid,
        default_pos,
        egui::vec2(840.0, 600.0),
        None,
        |ui| {
            ui.label(egui::RichText::new("📝").size(16.0));
            ui.label(egui::RichText::new(&title).font(stencil(14.0)).color(SAND));
            if let Some(d) = &active_doc {
                ui.add(egui::Label::new(egui::RichText::new(&d.path).small().monospace().color(DIM)).truncate());
                if d.dirty {
                    ui.label(egui::RichText::new("●").color(AMBER))
                        .on_hover_text("unsaved changes");
                }
                if d.kind == DocKind::Markdown
                    && ui
                        .small_button(if preview { "✎ edit" } else { "👁 preview" })
                        .on_hover_text("Markdown: switch between the source and the rendered page")
                        .clicked()
                {
                    toggle_preview = true;
                }
                if matches!(d.kind, DocKind::Code | DocKind::Markdown)
                    && ui
                        .small_button("💾")
                        .on_hover_text("Save (Ctrl+S inside the editor)")
                        .clicked()
                {
                    save = true;
                }
                if ui.small_button("🖥").on_hover_text("Open in VS Code").clicked() {
                    reveal = true;
                }
            }
        },
        |ui, _q| {
            ui.horizontal_wrapped(|ui| {
                ui.style_mut().interaction.selectable_labels = false;
                for (i, d) in docs.iter().enumerate() {
                    let name = file_name(&d.path);
                    let label = if d.dirty { format!("{name} ●") } else { name };
                    let mut text = egui::RichText::new(label).small().monospace();
                    if d.preview {
                        text = text.italics().color(DIM);
                    }
                    let r = ui.selectable_label(Some(i) == active, text).on_hover_text(format!(
                        "{}\n{}",
                        d.path,
                        if d.preview {
                            "preview tab: double-click to keep"
                        } else {
                            ""
                        }
                    ));
                    if r.double_clicked() {
                        pin = Some(i);
                    } else if r.clicked() {
                        switch = Some(i);
                    }
                    if Some(i) == active && ui.small_button("✕").on_hover_text("Close this file").clicked() {
                        close = Some(i);
                    }
                }
            });
            let avail = ui.available_size();
            let (rect, _) = ui.allocate_exact_size(avail, egui::Sense::hover());
            ui.painter().rect_filled(rect, 4.0, INK);
            if docs.is_empty() {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "open a file from the files card · Ctrl+P",
                    egui::FontId::proportional(14.0),
                    DIM,
                );
            } else {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "loading the editor…",
                    egui::FontId::proportional(13.0),
                    DIM,
                );
            }
            surface = Some(rect);
        },
    );
    end_card(store, settings, "editor-card", cs, dock, &out.sash);
    store.webview_rect = surface
        .filter(|_| !docs.is_empty())
        .map(|r| [r.min.x * ppp, r.min.y * ppp, r.width() * ppp, r.height() * ppp]);
    if store.webview_rect.is_some() && !store.webview_logged {
        store.webview_logged = true;
        bevy::log::info!("editor card: surface rect {:?} (ppp {ppp})", store.webview_rect);
    }
    if let Some(i) = switch {
        store.active_doc = Some(i);
        if let Some(d) = store.docs.get(i) {
            let path = d.path.clone();
            store.editor_cmds.push(EditorCmd::Open { path });
        }
    }
    if let Some(i) = pin {
        store.pin_doc(i);
    }
    if let Some(i) = close {
        store.close_doc(i);
    }
    if save {
        store.editor_cmds.push(EditorCmd::Save);
    }
    if toggle_preview {
        store.md_preview = !store.md_preview;
        let preview = store.md_preview;
        store.editor_cmds.push(EditorCmd::Mode { preview });
    }
    if reveal && let Some(d) = &active_doc {
        open_in_editor("", &d.path);
    }
    if out.toggle_max {
        store.editor_max = !store.editor_max;
    }
    if out.close {
        store.editor_open = false;
        store.webview_rect = None;
    }
    out.rect
}
