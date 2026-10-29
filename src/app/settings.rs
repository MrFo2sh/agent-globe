//! User settings: camera feel, motion, look, sound. Persisted as JSON in the OS config dir.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Resource, Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    pub orbit_sensitivity: f32,
    pub zoom_sensitivity: f32,
    pub pan_sensitivity: f32,
    pub idle_orbit: bool,
    pub drift_speed: f32,
    pub bloom: f32,
    /// The warm key light and the cool fill, in lux; the ambient in nits; fog as a multiplier.
    pub key_light: f32,
    pub fill_light: f32,
    pub ambient_light: f32,
    pub fog: f32,
    pub ui_scale: f32,
    pub scanlines: bool,
    /// Master switch for all audio.
    pub sound: bool,
    /// 0.0 .. 1.0
    pub volume: f32,
    /// A short blip on every tool call (busy sessions make this chatty).
    pub tool_blips: bool,
    /// Narrator line under each tool call.
    pub narrator: bool,
    /// Output device name; empty = system default.
    pub audio_device: String,
    /// Progress rings and hints under units.
    pub progress_rings: bool,
    /// Other active sessions as small constellations around the current one.
    pub satellites: bool,
    /// Outer ring of the session's hottest files, with beams from the units touching them.
    pub files_ring: bool,
    /// Lines between connected files (imports, co-changes, same-turn touches) for the hovered or open file.
    pub file_links: bool,
    /// Every connection, faint, all the time (a lot of lines on a big project).
    pub file_links_all: bool,
    /// The file index admits dotfiles and dot-folders / files the gitignore excludes.
    pub files_show_hidden: bool,
    pub files_show_ignored: bool,
    /// Force the compact layout (it also turns on by itself under 1100 px of width).
    pub compact: bool,
    /// Windows notifications when the window is not focused (off by default: the radio is the alert).
    pub desktop_notifications: bool,
    /// Native window title bar in windowed mode (off: the app draws its own chrome).
    pub title_bar: bool,
    /// One borderless window across every monitor at start (off: full screen on one monitor).
    pub span_monitors: bool,
    /// The dock grid: cells across and down the whole window. Two columns per monitor reads well.
    pub grid_cols: u8,
    pub grid_rows: u8,
    /// The agent panel's width, as last dragged (0: the default).
    pub agent_panel_width: f32,
    /// Where each card is docked, by card id. Floating cards are absent.
    pub panes: HashMap<String, Pane>,
}

/// A docked card's rectangle as fractions of the dock area, so it survives any window size.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Pane {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            orbit_sensitivity: 0.45,
            zoom_sensitivity: 0.35,
            pan_sensitivity: 0.6,
            idle_orbit: true,
            drift_speed: 1.0,
            bloom: 0.12,
            ui_scale: 1.0,
            scanlines: false,
            sound: true,
            volume: 0.7,
            tool_blips: false,
            narrator: true,
            audio_device: String::new(),
            progress_rings: true,
            satellites: true,
            files_ring: true,
            file_links: false,
            file_links_all: false,
            files_show_hidden: false,
            files_show_ignored: false,
            compact: false,
            desktop_notifications: false,
            title_bar: false,
            key_light: 2_600.0,
            fill_light: 700.0,
            ambient_light: 160.0,
            fog: 1.0,
            span_monitors: true,
            grid_cols: 4,
            grid_rows: 2,
            agent_panel_width: 0.0,
            panes: HashMap::new(),
        }
    }
}

pub fn path() -> PathBuf {
    config_dir().join("settings.json")
}

/// `<OS config dir>/agent-globe`, also where user sound overrides live (`sounds/<event>.wav`).
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("agent-globe")
}

pub fn load() -> Settings {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

impl Settings {
    pub fn save(&self) {
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, json);
        }
    }
}

/// Write the file whenever a value changed since the last frame.
pub fn persist(settings: Res<Settings>, mut last: Local<Option<Settings>>) {
    if last.as_ref() != Some(&*settings) {
        if last.is_some() {
            settings.save();
        }
        *last = Some(settings.clone());
    }
}
