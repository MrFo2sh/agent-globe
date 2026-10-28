//! User settings: camera feel, motion, look, sound. Persisted as JSON in the OS config dir.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
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
