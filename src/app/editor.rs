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
    open: &mut Option<String>,
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
                    .on_hover_text(format!("{}\nclick: open here · right-click: VS Code", e.path));
                if r.clicked() {
                    *open = Some(e.path.clone());
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

/// The files card: the session folder as a tree, and the card's search box as a fuzzy finder
/// over the whole folder (Ctrl+P from anywhere opens it with the box focused).
pub fn draw_files_card(
    ctx: &egui::Context,
    store: &mut Store,
    settings: &mut Settings,
    index: &mut FileIndex,
) -> Option<egui::Rect> {
    if !store.files_open {
        return None;
    }
    let (cwd, project) = store
        .current_session()
        .map(|s| (s.cwd.clone(), s.project.clone()))
        .unwrap_or_default();
    if cwd.is_empty() {
        store.files_open = false;
        return None;
    }
    index.ensure(&cwd, false);
    let building = index.is_building();
    let count = index.len();
    let maxed = store.files_max;
    let (mut cs, mut dock, grid) = begin_card(store, settings, "files-card", ctx);
    let mut query = std::mem::take(&mut store.files_query);
    let focus_search = std::mem::take(&mut store.files_focus_search);
    let open_dirs = store.files_open_dirs.clone();
    let mut refresh = false;
    let mut open_path: Option<String> = None;
    let mut reveal: Option<String> = None;
    let mut toggle_dir: Option<String> = None;
    let vr = ctx.viewport_rect();
    let default_pos = egui::pos2(vr.min.x + 72.0, vr.min.y + 90.0);
    let out = floating_card(
        ctx,
        "files-card",
        maxed,
        &mut cs,
        &mut dock,
        grid,
        default_pos,
        egui::vec2(400.0, 640.0),
        Some(&mut query),
        |ui| {
            ui.label(egui::RichText::new("🗂").size(16.0));
            ui.label(egui::RichText::new(&project).font(stencil(14.0)).color(SAND));
            ui.add(egui::Label::new(egui::RichText::new(&cwd).small().monospace().color(DIM)).truncate());
            if building {
                ui.spinner();
            }
            if ui.small_button("↻").on_hover_text("Rebuild the file index").clicked() {
                refresh = true;
            }
        },
        |ui, q| {
            if focus_search {
                ctx.memory_mut(|m| m.request_focus(egui::Id::new(("files-card", "search"))));
            }
            if !q.is_empty() {
                let hits = index.search(q, 40);
                ui.label(
                    egui::RichText::new(format!("{} of {count} files · Enter opens the first", hits.len()))
                        .small()
                        .color(DIM),
                );
                if ctx.input(|i| i.key_pressed(egui::Key::Enter))
                    && let Some((p, _)) = hits.first()
                {
                    open_path = Some(format!("{cwd}/{p}"));
                }
                egui::ScrollArea::vertical()
                    .id_salt("files-finder")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width() - 8.0);
                        for (p, idx) in &hits {
                            let r = ui
                                .add(egui::Button::new(hit_label(p, idx)).frame(false))
                                .on_hover_text("click: open here · right-click: VS Code");
                            if r.clicked() {
                                open_path = Some(format!("{cwd}/{p}"));
                            }
                            if r.secondary_clicked() {
                                reveal = Some(format!("{cwd}/{p}"));
                            }
                        }
                    });
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("files-tree")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width() - 8.0);
                        tree(
                            ui,
                            index,
                            &cwd,
                            0,
                            &open_dirs,
                            &mut toggle_dir,
                            &mut open_path,
                            &mut reveal,
                        );
                    });
            }
        },
    );
    store.files_query = query;
    end_card(store, settings, "files-card", cs, dock);
    if refresh {
        index.ensure(&cwd, true);
    }
    if let Some(d) = toggle_dir
        && !store.files_open_dirs.remove(&d)
    {
        store.files_open_dirs.insert(d);
    }
    if let Some(p) = open_path {
        store.open_doc(&p);
    }
    if let Some(p) = reveal {
        open_in_editor("", &p);
    }
    if out.toggle_max {
        store.files_max = !store.files_max;
    }
    if out.close {
        store.files_open = false;
    }
    out.rect
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
                    if ui
                        .selectable_label(Some(i) == active, egui::RichText::new(label).small().monospace())
                        .on_hover_text(&d.path)
                        .clicked()
                    {
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
    end_card(store, settings, "editor-card", cs, dock);
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
