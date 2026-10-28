//! Desktop notifications and the tray icon. The tray icon's colour is the one-glance status:
//! green all clear, amber someone is waiting for you, red a unit took fire.

use super::settings::Settings;
use super::state::{BadgeKind, Store};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use std::collections::HashSet;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    Clear,
    Waiting,
    Fire,
}

/// The app icon at 32 px with a status dot in the corner: green clear, amber waiting, red fire.
fn icon(mood: Mood) -> Icon {
    let (r, g, b) = match mood {
        Mood::Clear => (120u8, 210u8, 90u8),
        Mood::Waiting => (255, 190, 70),
        Mood::Fire => (240, 80, 60),
    };
    let base = image::load_from_memory(include_bytes!("../../assets/icon/agent-globe-32.png"))
        .map(|i| i.to_rgba8())
        .unwrap_or_else(|_| image::RgbaImage::from_pixel(32, 32, image::Rgba([30, 50, 25, 255])));
    let (w, h) = base.dimensions();
    let mut rgba = base.into_raw();
    // Status dot, bottom-right, 11 px with a dark rim.
    let (cx, cy, rad) = (w as i32 - 7, h as i32 - 7, 5.5f32);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let d = (((x - cx).pow(2) + (y - cy).pow(2)) as f32).sqrt();
            let i = ((y as u32 * w + x as u32) * 4) as usize;
            if d <= rad {
                rgba[i..i + 4].copy_from_slice(&[r, g, b, 255]);
            } else if d <= rad + 1.5 {
                rgba[i..i + 4].copy_from_slice(&[10, 14, 8, 255]);
            }
        }
    }
    Icon::from_rgba(rgba, w, h).expect("tray icon")
}

pub struct Tray {
    icon: TrayIcon,
    mood: Mood,
    show_id: tray_icon::menu::MenuId,
    mute_id: tray_icon::menu::MenuId,
    quit_id: tray_icon::menu::MenuId,
}

/// Lives on the main thread only (Win32 requirement), hence NonSend.
pub fn setup(world: &mut World) {
    let menu = Menu::new();
    let show = MenuItem::new("Show Agent Globe", true, None);
    let mute = MenuItem::new("Mute / unmute radio", true, None);
    let quit = MenuItem::new("Quit", true, None);
    let _ = menu.append_items(&[&show, &mute, &PredefinedMenuItem::separator(), &quit]);
    match TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Agent Globe: all clear")
        .with_icon(icon(Mood::Clear))
        .build()
    {
        Ok(tray) => {
            world.insert_non_send(Tray {
                icon: tray,
                mood: Mood::Clear,
                show_id: show.id().clone(),
                mute_id: mute.id().clone(),
                quit_id: quit.id().clone(),
            });
        }
        Err(e) => warn!("tray icon unavailable: {e}"),
    }
}

/// Poll tray and menu events, keep the icon colour in sync with the badges.
pub fn update(
    tray: Option<NonSendMut<Tray>>,
    mut store: ResMut<Store>,
    mut settings: ResMut<Settings>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut tray) = tray else { return };
    while let Ok(ev) = MenuEvent::receiver().try_recv() {
        if ev.id == tray.show_id {
            for mut w in &mut windows {
                w.set_minimized(false);
                w.focused = true;
            }
        } else if ev.id == tray.mute_id {
            settings.sound = !settings.sound;
        } else if ev.id == tray.quit_id {
            exit.write(AppExit::Success);
        }
    }
    while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
        if let TrayIconEvent::DoubleClick { .. } = ev {
            for mut w in &mut windows {
                w.set_minimized(false);
                w.focused = true;
            }
        }
    }
    let mood = if store.badges.iter().any(|b| b.kind == BadgeKind::Error) {
        Mood::Fire
    } else if store
        .badges
        .iter()
        .any(|b| matches!(b.kind, BadgeKind::Waiting | BadgeKind::Approval | BadgeKind::Stalled))
    {
        Mood::Waiting
    } else {
        Mood::Clear
    };
    if mood != tray.mood {
        tray.mood = mood;
        let _ = tray.icon.set_icon(Some(icon(mood)));
        let tip = match mood {
            Mood::Clear => "Agent Globe: all clear",
            Mood::Waiting => "Agent Globe: a unit is waiting for you",
            Mood::Fire => "Agent Globe: a unit took fire",
        };
        let _ = tray.icon.set_tooltip(Some(tip));
    }
    store.tray_mood = Some(mood);
}

/// Windows toasts for new badges while the window is not focused. One per badge, never repeated.
pub fn notify(
    store: Res<Store>,
    settings: Res<Settings>,
    time: Res<Time>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut seen: Local<HashSet<String>>,
) {
    // Badges present right after launch are history, not news.
    let warming_up = time.elapsed_secs() < 8.0;
    let focused = windows.iter().next().is_some_and(|w| w.focused);
    for b in &store.badges {
        let session = store.sessions.get(&b.session_id);
        let key = format!("{:?}:{}:{}", b.kind, b.session_id, b.agent_id);
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        if focused || warming_up || !settings.desktop_notifications {
            continue;
        }
        let name = session
            .and_then(|s| s.agent(&b.agent_id).map(|a| (s, a)))
            .map(|(s, a)| {
                if a.depth == 0 {
                    format!("{} commander", s.project)
                } else {
                    format!("{} / {}", s.project, a.name)
                }
            })
            .unwrap_or_default();
        let (summary, body) = match b.kind {
            BadgeKind::Waiting => ("Waiting for your orders", format!("{name} finished its turn.")),
            BadgeKind::Done => ("Mission accomplished", format!("{name} is done.")),
            BadgeKind::Error => ("Taking fire", format!("A tool call failed in {name}.")),
            BadgeKind::Approval => ("Authorization needed", format!("{name} wants to run a command.")),
            BadgeKind::Stalled => ("Stalled", format!("A call in {name} has been running a long time.")),
        };
        std::thread::spawn(move || {
            let _ = notify_rust::Notification::new()
                .summary(summary)
                .body(&body)
                .appname("Agent Globe")
                .show();
        });
    }
    // Forget keys whose badge is gone, so the next occurrence notifies again.
    seen.retain(|k| {
        store
            .badges
            .iter()
            .any(|b| k == &format!("{:?}:{}:{}", b.kind, b.session_id, b.agent_id))
    });
}
