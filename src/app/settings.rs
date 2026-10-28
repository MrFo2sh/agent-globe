//! User settings: camera feel, motion, look. Persisted as JSON in the OS config dir.

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
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            orbit_sensitivity: 0.45,
            zoom_sensitivity: 0.35,
            pan_sensitivity: 0.6,
            idle_orbit: true,
            drift_speed: 1.0,
            bloom: 0.15,
            ui_scale: 1.0,
            scanlines: true,
        }
    }
}

pub fn path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("agent-globe")
        .join("settings.json")
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
