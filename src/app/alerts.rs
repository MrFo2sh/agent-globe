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

/// A 32x32 rounded square with a dark base plate, in the mood's colour.
fn icon(mood: Mood) -> Icon {
    let (r, g, b) = match mood {
        Mood::Clear => (120u8, 210u8, 90u8),
        Mood::Waiting => (255, 190, 70),
        Mood::Fire => (240, 80, 60),
    };
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            let dx = x as i32 - 16;
            let dy = y as i32 - 16;
            let in_disc = dx * dx + dy * dy <= 13 * 13;
            let in_body = (6..=25).contains(&x) && (4..=22).contains(&y);
            if in_body {
                rgba[i..i + 4].copy_from_slice(&[r, g, b, 255]);
            } else if in_disc && y >= 20 {
                rgba[i..i + 4].copy_from_slice(&[40, 70, 35, 255]);
            }
        }
    }
    Icon::from_rgba(rgba, size, size).expect("tray icon")
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
        .any(|b| matches!(b.kind, BadgeKind::Waiting | BadgeKind::Approval))
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
    windows: Query<&Window, With<PrimaryWindow>>,
    mut seen: Local<HashSet<String>>,
) {
    let focused = windows.iter().next().is_some_and(|w| w.focused);
    let session = store.current_session();
    for b in &store.badges {
        let key = format!("{:?}:{}", b.kind, b.agent_id);
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        if focused || !settings.desktop_notifications {
            continue;
        }
        let name = session
            .and_then(|s| s.agent(&b.agent_id))
            .map(|a| {
                if a.depth == 0 {
                    "commander".to_string()
                } else {
                    a.name.clone()
                }
            })
            .unwrap_or_default();
        let (summary, body) = match b.kind {
            BadgeKind::Waiting => ("Waiting for your orders", format!("{name} finished its turn.")),
            BadgeKind::Done => ("Mission accomplished", format!("{name} is done.")),
            BadgeKind::Error => ("Taking fire", format!("A tool call failed in {name}.")),
            BadgeKind::Approval => ("Authorization needed", format!("{name} wants to run a command.")),
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
            .any(|b| k == &format!("{:?}:{}", b.kind, b.agent_id))
    });
}
