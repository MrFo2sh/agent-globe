//! The editor surface: a WebView2 child window (Edge's engine, part of Windows 11) hosted inside
//! the editor card's rectangle. One page carries Monaco for code, Markdown rendered with Mermaid,
//! Word documents converted in the page, and images; a second view shows PDFs in Edge's own
//! viewer. Runs on the main thread (the views are not `Send`), one frame behind the UI that
//! reserved the rectangle. A native child window always paints above the scene and above any
//! card that overlaps it: that is the price of a real editor.

use super::files::{DocKind, kind_of, language_of};
use super::state::{EditorCmd, Store, ToastKind};
use base64::Engine;
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, RawHandleWrapper};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

const PAGE: &str = include_str!("../../assets/editor/page.html");

enum Ipc {
    Ready,
    Log(String),
    QuickOpen,
    Mode { preview: bool },
    CloseTab,
    Dirty { path: String, dirty: bool },
    Save { path: String, content: String },
}

fn parse_ipc(body: &str) -> Option<Ipc> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let path = v["path"].as_str().unwrap_or("").to_string();
    match v["type"].as_str()? {
        "ready" => Some(Ipc::Ready),
        "log" => Some(Ipc::Log(v["text"].as_str().unwrap_or("").to_string())),
        "quickopen" => Some(Ipc::QuickOpen),
        "mode" => Some(Ipc::Mode {
            preview: v["preview"].as_bool().unwrap_or(true),
        }),
        "closetab" => Some(Ipc::CloseTab),
        "dirty" => Some(Ipc::Dirty {
            path,
            dirty: v["dirty"].as_bool().unwrap_or(true),
        }),
        "save" => Some(Ipc::Save {
            path,
            content: v["content"].as_str().unwrap_or("").to_string(),
        }),
        _ => None,
    }
}

#[derive(Default)]
pub struct WebViews {
    page: Option<wry::WebView>,
    pdf: Option<wry::WebView>,
    pdf_url: String,
    rx: Option<Receiver<Ipc>>,
    tx: Option<Sender<Ipc>>,
    ready: bool,
    queued: Vec<String>,
    created: Option<Instant>,
    warned: bool,
    failed: bool,
    shown: bool,
}

fn bounds(r: [f32; 4]) -> wry::Rect {
    wry::Rect {
        position: wry::dpi::PhysicalPosition::new(r[0].round() as i32, r[1].round() as i32).into(),
        size: wry::dpi::PhysicalSize::new(r[2].round().max(1.0) as u32, r[3].round().max(1.0) as u32).into(),
    }
}

/// `file:///D:/x/y.pdf` for a Windows path.
pub fn file_url(path: &str) -> String {
    let p = path
        .replace('\\', "/")
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('#', "%23")
        .replace('?', "%3F");
    if p.starts_with('/') {
        format!("file://{p}")
    } else {
        format!("file:///{p}")
    }
}

fn mime_of(path: &str) -> &'static str {
    match path.rsplit('.').next().map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// The script that opens `path` in the page, read fresh from disk.
fn open_script(path: &str, preview: bool) -> Result<String, String> {
    let kind = kind_of(path);
    let msg = match kind {
        DocKind::Image | DocKind::Docx => {
            let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path))?;
            let b64 = base64::prelude::BASE64_STANDARD.encode(bytes);
            serde_json::json!({
                "kind": if kind == DocKind::Image { "image" } else { "docx" },
                "path": path,
                "mime": mime_of(path),
                "b64": b64,
            })
        }
        _ => {
            let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path))?;
            let content = String::from_utf8_lossy(&bytes).to_string();
            serde_json::json!({
                "kind": if kind == DocKind::Markdown { "md" } else { "code" },
                "path": path,
                "lang": language_of(path),
                "content": content,
                "preview": preview,
            })
        }
    };
    Ok(format!("window.agOpen({msg})"))
}

impl WebViews {
    fn run(&mut self, script: String) {
        if self.ready
            && let Some(v) = &self.page
        {
            let _ = v.evaluate_script(&script);
        } else {
            self.queued.push(script);
        }
    }
}

/// Every frame on the main thread: create the views when first needed, feed them the editor's
/// commands, apply saves and dirty marks the page reports, and place or hide them to match the
/// rectangle the editor card reserved.
pub fn sync(
    handles: Query<&RawHandleWrapper, With<PrimaryWindow>>,
    mut views: NonSendMut<WebViews>,
    mut store: ResMut<Store>,
) {
    let Ok(wrapper) = handles.single() else { return };
    // This system holds a NonSend parameter, so Bevy runs it on the main thread, where the
    // window handle may be used.
    let window = unsafe { wrapper.get_handle() };
    let want = store.webview_rect;
    if want.is_some() && views.created.is_none() && !views.failed {
        info!("webview sync: surface wanted at {:?}", want);
    }
    let active_kind = store.active_doc.and_then(|i| store.docs.get(i)).map(|d| d.kind);

    // ---- creation, the first time the editor shows a file
    if want.is_some() && views.page.is_none() && !views.failed {
        let (tx, rx) = channel::<Ipc>();
        let tx2 = tx.clone();
        let r = want.unwrap_or([0.0, 0.0, 10.0, 10.0]);
        let built = wry::WebViewBuilder::new()
            .with_html(PAGE)
            .with_background_color((6, 12, 5, 255))
            .with_visible(false)
            .with_bounds(bounds(r))
            .with_ipc_handler(move |req: wry::http::Request<String>| {
                if let Some(m) = parse_ipc(req.body()) {
                    let _ = tx2.send(m);
                }
            })
            .build_as_child(&window);
        match built {
            Ok(v) => {
                info!("editor surface created at {:?}", r);
                views.page = Some(v);
                views.rx = Some(rx);
                views.tx = Some(tx);
                views.created = Some(Instant::now());
            }
            Err(e) => {
                views.failed = true;
                error!("editor surface failed: {e}");
                store.toast(ToastKind::Error, format!("editor surface failed: {e}"));
            }
        }
    }

    // ---- what the page reports
    let mut inbox = Vec::new();
    if let Some(rx) = &views.rx {
        while let Ok(m) = rx.try_recv() {
            inbox.push(m);
        }
    }
    for m in inbox {
        match m {
            Ipc::Log(text) => info!("editor page: {text}"),
            Ipc::QuickOpen => {
                store.quick_open = true;
                store.quick_focus = true;
                store.focus_app = true;
            }
            Ipc::Mode { preview } => store.md_preview = preview,
            Ipc::CloseTab => {
                if let Some(i) = store.active_doc {
                    store.close_doc(i);
                }
            }
            Ipc::Ready => {
                info!("editor page ready");
                views.ready = true;
                let queued = std::mem::take(&mut views.queued);
                if let Some(v) = &views.page {
                    for s in queued {
                        let _ = v.evaluate_script(&s);
                    }
                }
            }
            Ipc::Dirty { path, dirty } => {
                if let Some(d) = store.docs.iter_mut().find(|d| d.path == path) {
                    d.dirty = dirty;
                    if dirty {
                        d.preview = false;
                    }
                }
            }
            Ipc::Save { path, content } => match std::fs::write(&path, content) {
                Ok(()) => {
                    if let Some(d) = store.docs.iter_mut().find(|d| d.path == path) {
                        d.dirty = false;
                    }
                    let name = path.rsplit(['\\', '/']).next().unwrap_or(&path).to_string();
                    store.toast(ToastKind::Done, format!("saved {name}"));
                    let js = format!("window.agSaved({})", serde_json::json!(path));
                    views.run(js);
                }
                Err(e) => store.toast(ToastKind::Error, format!("could not save: {e}")),
            },
        }
    }
    if !views.ready && !views.warned && views.created.is_some_and(|t| t.elapsed().as_secs() > 12) {
        views.warned = true;
        warn!("editor page not ready after 12 s");
        store.toast(
            ToastKind::Warn,
            "the editor page has not loaded: Monaco and the Markdown tools come from the internet",
        );
    }

    // ---- the editor's commands
    let cmds = std::mem::take(&mut store.editor_cmds);
    let preview = store.md_preview;
    for c in cmds {
        match c {
            EditorCmd::Open { path } => {
                if kind_of(&path) == DocKind::Pdf {
                    let url = file_url(&path);
                    if views.pdf.is_none() {
                        let r = want.unwrap_or([0.0, 0.0, 10.0, 10.0]);
                        match wry::WebViewBuilder::new()
                            .with_url(&url)
                            .with_visible(false)
                            .with_bounds(bounds(r))
                            .build_as_child(&window)
                        {
                            Ok(v) => views.pdf = Some(v),
                            Err(e) => store.toast(ToastKind::Error, format!("pdf view failed: {e}")),
                        }
                    } else if views.pdf_url != url
                        && let Some(v) = &views.pdf
                    {
                        let _ = v.load_url(&url);
                    }
                    views.pdf_url = url;
                } else {
                    match open_script(&path, preview) {
                        Ok(js) => views.run(js),
                        Err(e) => store.toast(ToastKind::Error, format!("could not open {e}")),
                    }
                }
            }
            EditorCmd::Mode { preview } => {
                views.run(format!(
                    "window.agMode({})",
                    serde_json::json!(if preview { "preview" } else { "edit" })
                ));
            }
            EditorCmd::Save => views.run("window.agSave()".to_string()),
            EditorCmd::Zoom(d) => views.run(format!("window.agZoom({d})")),
            EditorCmd::Close { path } => views.run(format!("window.agClose({})", serde_json::json!(path))),
        }
    }

    // ---- the page asked for the keyboard to come back to the app (quick open)
    if std::mem::take(&mut store.focus_app) {
        #[cfg(windows)]
        if let raw_window_handle::RawWindowHandle::Win32(h) = wrapper.get_window_handle() {
            unsafe {
                windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus(h.hwnd.get() as _);
            }
        }
    }

    // ---- placement: the page or the pdf view fills the reserved rectangle, the other hides
    let show_pdf = active_kind == Some(DocKind::Pdf);
    if let Some(v) = &views.page {
        match (want, show_pdf) {
            (Some(r), false) => {
                if let Err(e) = v.set_bounds(bounds(r)) {
                    warn!("editor surface bounds: {e}");
                }
                if let Err(e) = v.set_visible(true) {
                    warn!("editor surface visible: {e}");
                }
                if !views.shown {
                    views.shown = true;
                    info!("editor surface shown at {:?}", r);
                }
            }
            _ => {
                let _ = v.set_visible(false);
            }
        }
    }
    if let Some(v) = &views.pdf {
        match (want, show_pdf) {
            (Some(r), true) => {
                let _ = v.set_bounds(bounds(r));
                let _ = v.set_visible(true);
            }
            _ => {
                let _ = v.set_visible(false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_and_ipc_parse() {
        assert_eq!(
            file_url("D:\\docs\\my paper #2.pdf"),
            "file:///D:/docs/my%20paper%20%232.pdf"
        );
        assert!(matches!(parse_ipc(r#"{"type":"ready"}"#), Some(Ipc::Ready)));
        match parse_ipc(r#"{"type":"save","path":"a.rs","content":"fn x(){}"}"#) {
            Some(Ipc::Save { path, content }) => {
                assert_eq!(path, "a.rs");
                assert_eq!(content, "fn x(){}");
            }
            _ => panic!("save not parsed"),
        }
        assert!(parse_ipc("not json").is_none());
    }
}
