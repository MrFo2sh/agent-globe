//! Radio commander voice lines and cues. Embedded synthesized clips by default; any file at
//! `<config dir>/agent-globe/sounds/<event>.wav` (or .ogg) overrides the embedded one.

use super::settings::{self, Settings};
use super::state::Store;
use bevy::audio::{AudioSource, PlaybackSettings, Volume};
use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sfx {
    Prompt,
    Spawn,
    Done,
    Error,
    Complete,
    Stall,
    Loop,
    Ship,
    Blip,
    Alarm,
    Fanfare,
}

impl Sfx {
    pub const ALL: [Sfx; 11] = [
        Sfx::Prompt,
        Sfx::Spawn,
        Sfx::Done,
        Sfx::Error,
        Sfx::Complete,
        Sfx::Stall,
        Sfx::Loop,
        Sfx::Ship,
        Sfx::Blip,
        Sfx::Alarm,
        Sfx::Fanfare,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Sfx::Prompt => "prompt",
            Sfx::Spawn => "spawn",
            Sfx::Done => "done",
            Sfx::Error => "error",
            Sfx::Complete => "complete",
            Sfx::Stall => "stall",
            Sfx::Loop => "loop",
            Sfx::Ship => "ship",
            Sfx::Blip => "blip",
            Sfx::Alarm => "alarm",
            Sfx::Fanfare => "fanfare",
        }
    }

    /// What the embedded clip says, for the settings window.
    pub fn line(self) -> &'static str {
        match self {
            Sfx::Prompt => "Consider it done, boss.",
            Sfx::Spawn => "Unit ready. Moving out.",
            Sfx::Done => "Mission accomplished.",
            Sfx::Error => "We are taking fire!",
            Sfx::Complete => "Construction complete.",
            Sfx::Stall => "Unit is stuck, commander.",
            Sfx::Loop => "Same orders again, sir?",
            Sfx::Ship => "Objective secured.",
            Sfx::Blip => "(tool blip)",
            Sfx::Alarm => "(siren)",
            Sfx::Fanfare => "(fanfare)",
        }
    }

    fn embedded(self) -> &'static [u8] {
        match self {
            Sfx::Prompt => include_bytes!("../../assets/sounds/prompt.wav"),
            Sfx::Spawn => include_bytes!("../../assets/sounds/spawn.wav"),
            Sfx::Done => include_bytes!("../../assets/sounds/done.wav"),
            Sfx::Error => include_bytes!("../../assets/sounds/error.wav"),
            Sfx::Complete => include_bytes!("../../assets/sounds/complete.wav"),
            Sfx::Stall => include_bytes!("../../assets/sounds/stall.wav"),
            Sfx::Loop => include_bytes!("../../assets/sounds/loop.wav"),
            Sfx::Ship => include_bytes!("../../assets/sounds/ship.wav"),
            Sfx::Blip => include_bytes!("../../assets/sounds/blip.wav"),
            Sfx::Alarm => include_bytes!("../../assets/sounds/alarm.wav"),
            Sfx::Fanfare => include_bytes!("../../assets/sounds/fanfare.wav"),
        }
    }

    /// Minimum seconds between two plays of the same cue.
    fn cooldown(self) -> f32 {
        match self {
            Sfx::Blip => 0.15,
            Sfx::Prompt | Sfx::Complete => 4.0,
            _ => 2.5,
        }
    }
}

#[derive(Resource)]
pub struct Sounds {
    handles: HashMap<Sfx, Handle<AudioSource>>,
    pub overrides: Vec<String>,
}

pub fn setup(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let dir = settings::config_dir().join("sounds");
    let mut handles = HashMap::new();
    let mut overrides = Vec::new();
    for sfx in Sfx::ALL {
        let custom = ["wav", "ogg"]
            .iter()
            .map(|ext| dir.join(format!("{}.{ext}", sfx.name())))
            .find(|p| p.exists())
            .and_then(|p| std::fs::read(&p).ok());
        let bytes: Arc<[u8]> = match custom {
            Some(b) => {
                overrides.push(sfx.name().to_string());
                Arc::from(b)
            }
            None => Arc::from(sfx.embedded()),
        };
        handles.insert(sfx, sources.add(AudioSource { bytes }));
    }
    info!(
        "sounds ready ({} embedded, {} overridden from {})",
        Sfx::ALL.len() - overrides.len(),
        overrides.len(),
        dir.display()
    );
    commands.insert_resource(Sounds { handles, overrides });
}

/// Drain the store's sound queue, respecting mute, volume and per-cue cooldowns.
pub fn play(
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<Settings>,
    sounds: Res<Sounds>,
    mut store: ResMut<Store>,
    mut last: Local<HashMap<Sfx, f32>>,
) {
    let queued: Vec<Sfx> = std::mem::take(&mut store.sfx);
    if !settings.sound || queued.is_empty() {
        return;
    }
    let now = time.elapsed_secs();
    for sfx in queued {
        if sfx == Sfx::Blip && !settings.tool_blips {
            continue;
        }
        if last.get(&sfx).is_some_and(|t| now - t < sfx.cooldown()) {
            continue;
        }
        last.insert(sfx, now);
        if let Some(h) = sounds.handles.get(&sfx) {
            commands.spawn((
                AudioPlayer::new(h.clone()),
                PlaybackSettings {
                    volume: Volume::Linear(settings.volume.clamp(0.0, 1.0)),
                    ..PlaybackSettings::DESPAWN
                },
            ));
        }
    }
}
