//! Global approval hotkeys: Ctrl+Alt+Y allows and Ctrl+Alt+N denies the oldest pending request,
//! from any window. Windows only; elsewhere the channel simply never fires.

use super::state::{BridgeRes, Store, ToastKind};
use bevy::prelude::*;
use crossbeam_channel::{Receiver, unbounded};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hotkey {
    Allow,
    Deny,
}

#[derive(Resource)]
pub struct Hotkeys(pub Receiver<Hotkey>);

pub fn start() -> Hotkeys {
    let (tx, rx) = unbounded::<Hotkey>();
    #[cfg(windows)]
    std::thread::Builder::new()
        .name("hotkeys".into())
        .spawn(move || listen(tx))
        .expect("spawn hotkeys");
    #[cfg(not(windows))]
    drop(tx);
    Hotkeys(rx)
}

#[cfg(windows)]
fn listen(tx: crossbeam_channel::Sender<Hotkey>) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};
    // SAFETY: plain Win32 calls on this thread's own message queue; a null window binds the hotkeys
    // to the thread, so WM_HOTKEY lands in GetMessageW below.
    unsafe {
        let mods = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
        let ok_y = RegisterHotKey(std::ptr::null_mut(), 1, mods, u32::from(b'Y'));
        let ok_n = RegisterHotKey(std::ptr::null_mut(), 2, mods, u32::from(b'N'));
        if ok_y == 0 && ok_n == 0 {
            warn!("approval hotkeys unavailable: Ctrl+Alt+Y / Ctrl+Alt+N are taken by another program");
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.message == WM_HOTKEY {
                let key = match msg.wParam {
                    1 => Hotkey::Allow,
                    2 => Hotkey::Deny,
                    _ => continue,
                };
                if tx.send(key).is_err() {
                    return;
                }
            }
        }
    }
}

/// Apply pressed hotkeys to the oldest pending approval.
pub fn poll(hotkeys: Res<Hotkeys>, mut store: ResMut<Store>, bridge: Res<BridgeRes>) {
    while let Ok(key) = hotkeys.0.try_recv() {
        let Some(req) = store.approvals.first().cloned() else {
            store.toast(ToastKind::Info, "no approval is pending");
            continue;
        };
        let decision = match key {
            Hotkey::Allow => "allow",
            Hotkey::Deny => "deny",
        };
        let what = req.command.clone().unwrap_or_else(|| req.summary.clone());
        let short: String = what.lines().next().unwrap_or("").chars().take(60).collect();
        store.decide(&bridge.0, req.id, decision);
        store.toast(
            if decision == "allow" {
                ToastKind::Done
            } else {
                ToastKind::Warn
            },
            format!(
                "{} by hotkey: {} {short}",
                if decision == "allow" {
                    "✔ allowed"
                } else {
                    "✖ denied"
                },
                req.tool
            ),
        );
    }
}
