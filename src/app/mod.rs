//! Bevy application. One file per concern; none of them parse files.

mod alerts;
mod camera;
mod debrief;
mod gitinfo;
mod probe;
mod scene;
mod settings;
mod sounds;
mod state;
mod theme;
mod ui;

use agent_globe::bridge::{self, BridgeConfig};
use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::picking::mesh_picking::MeshPickingPlugin;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
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
        .insert_resource(state::Store::new(cfg.session, cfg.claude_dir, cfg.http_port))
        .insert_resource(state::BridgeRes(bridge))
        .init_resource::<scene::NodeEntities>()
        .init_resource::<scene::RingEntities>()
        .init_resource::<scene::CurrentLayout>()
        .init_resource::<scene::NodeWorldPos>()
        .init_resource::<camera::CameraState>()
        .init_resource::<probe::Probe>()
        .init_resource::<gitinfo::GitWatch>()
        .add_systems(Startup, (camera::setup, scene::setup, sounds::setup, alerts::setup))
        .add_systems(
            Update,
            (
                state::drain_bridge,
                state::watch_stalls,
                scene::sync_nodes,
                scene::sync_helmets,
                scene::dress_rings,
                scene::animate_nodes,
                scene::update_halos,
                scene::animate_dust,
                scene::draw_links,
                camera::drive,
                camera::apply_viewport,
                scene::project_badges,
                alerts::update,
                alerts::notify,
                sounds::play,
                settings::persist,
                maximize_window,
                window_icon,
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
    }
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
