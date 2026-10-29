//! Bevy application. One file per concern; none of them parse files.

mod alerts;
mod camera;
mod gitinfo;
mod hotkeys;
mod probe;
mod raid;
mod scene;
mod servers;
mod settings;
mod sounds;
mod state;
mod terminal;
mod theme;
mod ui;
mod usage;
mod winjump;

use agent_globe::bridge::{self, BridgeConfig};
use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::picking::mesh_picking::MeshPickingPlugin;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::window::{MonitorSelection, WindowMode};
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};
use bevy_panorbit_camera::PanOrbitCameraPlugin;
use std::path::PathBuf;
use std::time::Duration;

pub struct Config {
    pub claude_dir: PathBuf,
    pub http_port: Option<u16>,
    pub session: Option<String>,
}

pub fn run(cfg: Config) {
    let bridge = bridge::start(BridgeConfig {
        claude_dir: cfg.claude_dir.clone(),
        http_port: cfg.http_port,
        poll_interval: Duration::from_millis(300),
    });

    App::new()
        .insert_resource(ClearColor(theme::BACKGROUND))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Agent Globe".into(),
                resolution: (1400, 860).into(),
                // No native title bar: the app draws its own chrome; F11 toggles full screen.
                decorations: false,
                ..default()
            }),
            ..default()
        }))
        .add_plugins((
            MeshPickingPlugin,
            EguiPlugin::default(),
            PanOrbitCameraPlugin,
            FrameTimeDiagnosticsPlugin::default(),
        ))
        .insert_resource(settings::load())
        .insert_resource(usage::UsageWatch::start(cfg.claude_dir.clone()))
        .insert_resource(state::Store::new(cfg.session, cfg.claude_dir, cfg.http_port))
        .insert_resource(state::BridgeRes(bridge))
        .init_resource::<scene::NodeEntities>()
        .init_resource::<scene::RingEntities>()
        .init_resource::<scene::SceneLayout>()
        .init_resource::<scene::FileEntities>()
        .init_resource::<scene::FileScene>()
        .init_resource::<scene::SatelliteRings>()
        .init_resource::<scene::NodeWorldPos>()
        .init_resource::<camera::CameraState>()
        .init_resource::<probe::Probe>()
        .init_resource::<servers::ServerWatch>()
        .init_resource::<terminal::Terminals>()
        .init_resource::<gitinfo::GitWatch>()
        .insert_resource(hotkeys::start())
        .add_systems(
            Startup,
            (camera::setup, scene::setup, sounds::setup, alerts::setup, raid::setup),
        )
        .add_systems(
            Update,
            // Bevy system tuples hold 20 at most: two chained halves keep the original order.
            (
                (
                    state::drain_bridge,
                    hotkeys::poll,
                    state::watch_stalls,
                    state::settle_turns,
                    scene::sync_nodes,
                    scene::sync_satellite_rings,
                    scene::sync_helmets,
                    scene::dress_rings,
                    scene::animate_nodes,
                    scene::animate_satellite_rings,
                    scene::update_halos,
                    scene::animate_dust,
                    scene::sync_files,
                    scene::draw_links,
                )
                    .chain(),
                (
                    scene::draw_file_beams,
                    camera::drive,
                    camera::apply_viewport,
                    scene::project_badges,
                    scene::project_files,
                    alerts::update,
                    alerts::notify,
                    sounds::play,
                    settings::persist,
                    maximize_window,
                    toggle_fullscreen,
                    apply_title_bar,
                    zoom_keys,
                    window_icon,
                )
                    .chain(),
                (raid::tick, raid::fx).chain(),
            )
                .chain(),
        )
        .add_systems(EguiPrimaryContextPass, ui::draw)
        .run();
}

/// Fit the screen whatever its size; a fixed resolution can hang off the bottom on small displays.
fn maximize_window(mut windows: Query<&mut Window, With<PrimaryWindow>>, mut done: Local<u32>) {
    if *done >= 3 {
        return;
    }
    *done += 1;
    for mut w in &mut windows {
        w.set_maximized(true);
        // Start full screen on the monitor the window opened on; F11 toggles back to a window.
        w.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
    }
}

/// F11 toggles borderless full screen on the monitor the window is on.
fn toggle_fullscreen(keys: Res<ButtonInput<KeyCode>>, mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    if !keys.just_pressed(KeyCode::F11) {
        return;
    }
    for mut w in &mut windows {
        w.mode = match w.mode {
            WindowMode::Windowed => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
            _ => WindowMode::Windowed,
        };
    }
}

/// The "title bar" setting drives the native window decorations.
fn apply_title_bar(settings: Res<settings::Settings>, mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    if !settings.is_changed() {
        return;
    }
    for mut w in &mut windows {
        if w.decorations != settings.title_bar {
            w.decorations = settings.title_bar;
        }
    }
}

/// Ctrl and + / - / 0 scale the whole UI, the same knob as the "ui scale" slider.
fn zoom_keys(keys: Res<ButtonInput<KeyCode>>, mut settings: ResMut<settings::Settings>) {
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if !ctrl {
        return;
    }
    let step = if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
        0.1
    } else if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
        -0.1
    } else if keys.just_pressed(KeyCode::Digit0) || keys.just_pressed(KeyCode::Numpad0) {
        1.0 - settings.ui_scale
    } else {
        return;
    };
    let next = ((settings.ui_scale + step) * 10.0).round() / 10.0;
    settings.ui_scale = next.clamp(0.6, 1.8);
}

/// Title-bar and taskbar icon for the running window (the exe resource covers Explorer).
fn window_icon(windows: Option<NonSend<bevy::winit::WinitWindows>>, mut done: Local<bool>) {
    if *done {
        return;
    }
    // Not reachable in every Bevy build; the exe's icon resource covers the window then.
    let Some(windows) = windows else { return };
    let Ok(img) = image::load_from_memory(include_bytes!("../../assets/icon/agent-globe-64.png")) else {
        *done = true;
        return;
    };
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let Ok(icon) = winit::window::Icon::from_rgba(rgba.into_raw(), w, h) else {
        *done = true;
        return;
    };
    let mut any = false;
    for window in windows.windows.values() {
        window.set_window_icon(Some(icon.clone()));
        any = true;
    }
    *done = any;
}
