//! Every color and tuning constant in one place. Theme: retro phosphor green, CRT glow, voxels.

use bevy::prelude::*;

pub const BACKGROUND: Color = Color::srgb(0.004, 0.012, 0.006);

pub const STAR: Color = Color::srgb(0.55, 1.0, 0.65);
pub const DUST: Color = Color::srgb(0.25, 0.95, 0.45);
pub const RING_MESH: Color = Color::srgba(0.20, 0.95, 0.45, 0.55);
pub const FOG: Color = Color::srgba(0.004, 0.02, 0.008, 1.0);
pub const LINK: Color = Color::srgba(0.25, 0.85, 0.40, 0.40);
pub const LINK_ACTIVE: Color = Color::srgba(0.55, 1.0, 0.65, 0.95);
pub const LINK_DONE: Color = Color::srgba(0.18, 0.40, 0.24, 0.30);
pub const PULSE: Color = Color::srgb(1.6, 3.0, 1.8);

pub const NODE_ROOT: Color = Color::srgb(1.0, 0.80, 0.25);
pub const NODE_ACTIVE: Color = Color::srgb(0.35, 1.0, 0.50);
pub const NODE_IDLE: Color = Color::srgb(0.20, 0.70, 0.35);
pub const NODE_DONE: Color = Color::srgb(0.16, 0.38, 0.22);
pub const NODE_ERROR: Color = Color::srgb(1.0, 0.30, 0.25);
pub const NODE_SELECTED: Color = Color::srgb(0.85, 1.0, 0.85);
pub const NODE_HOVER: Color = Color::srgb(0.60, 1.0, 0.70);

/// An agent counts as active if it did something within this window.
pub const ACTIVE_WINDOW_MS: i64 = 20_000;
/// Idle seconds before the camera starts its slow cinematic orbit.
pub const IDLE_ORBIT_AFTER_S: f32 = 8.0;
pub const STAR_COUNT: usize = 1400;
pub const DUST_COUNT: usize = 450;
