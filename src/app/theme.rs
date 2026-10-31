//! Every color and tuning constant in one place. Theme: green plastic army men on a night sandbox.

use bevy::prelude::*;

pub const BACKGROUND: Color = Color::srgb(0.025, 0.040, 0.018);
pub const FOG: Color = Color::srgba(0.025, 0.040, 0.018, 1.0);

pub const PLASTIC: Color = Color::srgb(0.36, 0.62, 0.27);
pub const PLASTIC_LIGHT: Color = Color::srgb(0.62, 0.86, 0.45);
pub const SAND: Color = Color::srgb(0.87, 0.78, 0.52);
pub const KHAKI: Color = Color::srgb(0.80, 0.68, 0.40);

pub const STAR: Color = Color::srgb(0.85, 0.88, 0.75);
pub const DUST: Color = Color::srgb(0.87, 0.78, 0.52);
pub const RING_MESH: Color = Color::srgba(0.87, 0.78, 0.52, 0.50);
pub const BASE: Color = Color::srgb(0.30, 0.50, 0.22);
pub const LINK: Color = Color::srgba(0.87, 0.78, 0.52, 0.30);
pub const LINK_ACTIVE: Color = Color::srgba(1.0, 0.92, 0.55, 0.95);
/// A file nobody touched lately: cool and quiet, apart from the warm dust.
pub const FILE_COLD: Color = Color::srgb(0.38, 0.62, 0.60);
/// A unit touching a file: the beam and the file's glow, cyan-green.
pub const TOUCH: Color = Color::srgb(0.45, 0.95, 0.85);
pub const LINK_DONE: Color = Color::srgba(0.45, 0.45, 0.35, 0.25);
/// Tracer rounds along busy beams.
pub const PULSE: Color = Color::srgb(3.0, 2.3, 0.8);

pub const NODE_ROOT: Color = KHAKI;
pub const NODE_ACTIVE: Color = PLASTIC_LIGHT;
pub const NODE_IDLE: Color = PLASTIC;
pub const NODE_DONE: Color = Color::srgb(0.24, 0.36, 0.20);
pub const NODE_ERROR: Color = Color::srgb(0.95, 0.30, 0.22);
pub const NODE_STALLED: Color = Color::srgb(1.0, 0.70, 0.20);
pub const NODE_SELECTED: Color = Color::srgb(0.95, 1.0, 0.90);
pub const NODE_HOVER: Color = Color::srgb(0.78, 0.96, 0.62);

/// An agent counts as active if it did something within this window.
pub const ACTIVE_WINDOW_MS: i64 = 20_000;
/// A finished subagent stays this long, so its end is seen, then leaves the scene (it stays in
/// the sidebar tree and its history; selecting it there brings it back while selected).
pub const RETIRE_MS: i64 = 45_000;
/// An open tool call older than this is a stall.
pub const STALL_AFTER_MS: i64 = 120_000;
/// Sessions modified within this window are tailed alongside the current one.
pub const WATCH_RECENT_MS: i64 = 30 * 60_000;
/// How many sessions to tail at once (the current one included).
pub const WATCH_MAX: usize = 6;
/// Other sessions with activity this recent appear as satellite constellations.
pub const SATELLITE_RECENT_MS: i64 = 10 * 60_000;
/// Distance of satellite constellations from the current one, and their scale.
pub const SATELLITE_RADIUS: f32 = 17.0;
pub const SATELLITE_SCALE: f32 = 0.5;
/// Context size that triggers the "context bloat" alarm, by model family.
/// 1M-window models (Fable, Mythos, Opus 5+, Sonnet 5+) get a higher bar than 200k-window ones.
pub fn context_alarm_tokens(model: &str) -> u64 {
    let m = model.to_ascii_lowercase();
    if m.contains("fable")
        || m.contains("mythos")
        || m.contains("opus-5")
        || m.contains("sonnet-5")
        || m.contains("haiku-5")
    {
        750_000
    } else {
        160_000
    }
}
/// Idle seconds before the camera starts its slow cinematic orbit.
pub const IDLE_ORBIT_AFTER_S: f32 = 8.0;
pub const STAR_COUNT: usize = 900;
pub const DUST_COUNT: usize = 500;
