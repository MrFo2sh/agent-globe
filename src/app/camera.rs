//! HDR orbit camera (MSAA, bloom, distance fog) rendering into the area the UI leaves free,
//! a separate camera for the UI layer, fly-to-selection, idle cinematic orbit, R to reset,
//! and user-tunable sensitivity. Disabled while the pointer is over a UI panel.

use super::scene::NodeWorldPos;
use super::settings::Settings;
use super::state::Store;
use super::theme;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, Hdr, Viewport};
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;
use bevy::post_process::effect_stack::ChromaticAberration;
use bevy::prelude::*;
use bevy::render::render_resource::BlendState;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiGlobalSettings, PrimaryEguiContext};
use bevy_panorbit_camera::PanOrbitCamera;

pub const CAMERA_HOME: Vec3 = Vec3::new(0.0, 9.0, 20.0);
const FOCUS_RADIUS: f32 = 6.5;

#[derive(Resource, Default)]
pub struct CameraState {
    last_input_s: f32,
    last_selected: Option<String>,
}

pub fn setup(mut commands: Commands, settings: Res<Settings>, mut egui_settings: ResMut<EguiGlobalSettings>) {
    // The UI gets its own camera so clipping the 3D camera to the free area never clips the UI.
    egui_settings.auto_create_primary_context = false;

    commands.spawn((
        Camera3d::default(),
        Hdr,
        Msaa::Sample4,
        Bloom {
            intensity: settings.bloom,
            ..Bloom::NATURAL
        },
        ChromaticAberration {
            intensity: 0.012,
            ..default()
        },
        DistanceFog {
            color: theme::FOG,
            falloff: FogFalloff::Linear {
                start: 45.0,
                end: 170.0,
            },
            ..default()
        },
        Transform::from_translation(CAMERA_HOME).looking_at(Vec3::ZERO, Vec3::Y),
        PanOrbitCamera {
            focus: Vec3::ZERO,
            zoom_lower_limit: 2.5,
            zoom_upper_limit: Some(140.0),
            orbit_smoothness: 0.12,
            zoom_smoothness: 0.2,
            pan_smoothness: 0.15,
            ..default()
        },
    ));

    commands.spawn((
        PrimaryEguiContext,
        Camera2d,
        RenderLayers::none(),
        Camera {
            order: 1,
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 500.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(8.0, 14.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.55, 0.70, 1.0),
        brightness: 40.0,
        ..default()
    });
}

fn go_home(cam: &mut PanOrbitCamera) {
    cam.target_focus = Vec3::ZERO;
    cam.target_radius = CAMERA_HOME.length();
    cam.target_yaw = 0.0;
    cam.target_pitch = (CAMERA_HOME.y / CAMERA_HOME.length()).asin();
    cam.force_update = true;
}

#[allow(clippy::too_many_arguments)]
pub fn drive(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    mut store: ResMut<Store>,
    settings: Res<Settings>,
    world_pos: Res<NodeWorldPos>,
    mut state: ResMut<CameraState>,
    mut cams: Query<(&mut PanOrbitCamera, &mut Bloom)>,
) {
    let now = time.elapsed_secs();
    if buttons.get_pressed().next().is_some() || scroll.delta != Vec2::ZERO || keys.get_just_pressed().next().is_some()
    {
        state.last_input_s = now;
    }
    let Ok((mut cam, mut bloom)) = cams.single_mut() else {
        return;
    };

    cam.orbit_sensitivity = settings.orbit_sensitivity;
    cam.zoom_sensitivity = settings.zoom_sensitivity;
    cam.pan_sensitivity = settings.pan_sensitivity;
    if (bloom.intensity - settings.bloom).abs() > 1e-4 {
        bloom.intensity = settings.bloom;
    }
    cam.enabled = !store.pointer_over_ui;

    if keys.just_pressed(KeyCode::KeyR) || store.reset_view {
        store.reset_view = false;
        go_home(&mut cam);
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        store.selected = None;
    }

    if store.selected != state.last_selected {
        state.last_selected = store.selected.clone();
        match store.selected.as_deref().and_then(|id| world_pos.0.get(id)) {
            Some(&p) => {
                cam.target_focus = p;
                cam.target_radius = FOCUS_RADIUS;
                cam.force_update = true;
            }
            None => go_home(&mut cam),
        }
        return;
    }

    let idle = now - state.last_input_s > theme::IDLE_ORBIT_AFTER_S;
    if settings.idle_orbit && idle && store.selected.is_none() && !store.pointer_over_ui {
        cam.target_yaw += time.delta_secs() * 0.07;
        cam.force_update = true;
    }
}

/// Render the 3D scene only into the area between the panels, so the camera's centre (and every
/// fly-to target) lands in the visible middle rather than behind a panel.
pub fn apply_viewport(
    store: Res<Store>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cams: Query<&mut Camera, With<PanOrbitCamera>>,
) {
    let (Some((left, top, right, bottom)), Ok(window)) = (store.scene_insets, windows.single()) else {
        return;
    };
    let scale = window.scale_factor();
    let (pw, ph) = (window.physical_width(), window.physical_height());
    if pw == 0 || ph == 0 {
        return;
    }
    let l = ((left * scale) as u32).min(pw - 1);
    let t = ((top * scale) as u32).min(ph - 1);
    let r = (right * scale) as u32;
    let b = (bottom * scale) as u32;
    let w = pw.saturating_sub(l + r).max(1);
    let h = ph.saturating_sub(t + b).max(1);
    for mut cam in &mut cams {
        let wanted = Viewport {
            physical_position: UVec2::new(l, t),
            physical_size: UVec2::new(w, h),
            ..default()
        };
        let same = cam.viewport.as_ref().is_some_and(|v| {
            v.physical_position == wanted.physical_position && v.physical_size == wanted.physical_size
        });
        if !same {
            cam.viewport = Some(wanted);
        }
    }
}

/// Logs window size, viewport and where the world origin lands, a few times after start.
pub fn diagnose(
    time: Res<Time>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&Camera, &GlobalTransform, &PanOrbitCamera)>,
    mut next: Local<f32>,
) {
    let t = time.elapsed_secs();
    if t < *next || t > 12.0 {
        return;
    }
    *next = t + 2.0;
    let Ok(w) = windows.single() else { return };
    for (cam, gt, orbit) in &cams {
        let origin = cam.world_to_viewport(gt, Vec3::ZERO).ok();
        debug!(
            "diag window {}x{} (scale {:.2}) viewport {:?} origin_on_screen {:?} radius {:?} focus {:?}",
            w.physical_width(),
            w.physical_height(),
            w.scale_factor(),
            cam.viewport.as_ref().map(|v| (v.physical_position, v.physical_size)),
            origin,
            orbit.radius,
            orbit.focus
        );
    }
}
