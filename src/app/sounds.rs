//! Radio commander voice lines and cues, played through our own rodio sink on a user-chosen
//! output device (Bevy's audio plugin only ever uses the system default, which on multi-device
//! machines is often a monitor nobody listens to). Embedded synthesized clips by default; any file
//! at `<config dir>/agent-globe/sounds/<event>.wav` (or .ogg) overrides the embedded one.

// cpal's `name()` is deprecated in favour of `description()`, but it is still the stable display string.
#![allow(deprecated)]

use super::settings::{self, Settings};
use super::state::Store;
use bevy::prelude::*;
use crossbeam_channel::{Receiver, Sender, unbounded};
use rodio::Source;
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

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

enum RadioCmd {
    Play { bytes: Arc<[u8]>, volume: f32 },
    SetDevice(String),
}

/// The radio: an audio thread owning the output sink, plus the clip bytes.
#[derive(Resource)]
pub struct Sounds {
    clips: HashMap<Sfx, Arc<[u8]>>,
    pub overrides: Vec<String>,
    tx: Sender<RadioCmd>,
    /// Output device names, refreshed by the audio thread.
    devices: Arc<Mutex<Vec<String>>>,
    /// What the audio thread last reported (device name or error).
    status: Arc<Mutex<String>>,
}

fn list_devices() -> Vec<String> {
    let host = rodio::cpal::default_host();
    let mut names: Vec<String> = host
        .output_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    names.dedup();
    names
}

fn open_sink(device_name: &str) -> Result<(rodio::MixerDeviceSink, String), String> {
    let host = rodio::cpal::default_host();
    let device = if device_name.is_empty() {
        host.default_output_device()
    } else {
        host.output_devices()
            .ok()
            .and_then(|mut it| it.find(|d| d.name().map(|n| n == device_name).unwrap_or(false)))
            .or_else(|| host.default_output_device())
    };
    let Some(device) = device else {
        return Err("no output device".into());
    };
    let name = device.name().unwrap_or_else(|_| "?".into());
    let sink = rodio::DeviceSinkBuilder::from_device(device)
        .and_then(|b| b.open_sink_or_fallback())
        .map_err(|e| format!("{name}: {e}"))?;
    Ok((sink, name))
}

fn audio_thread(rx: Receiver<RadioCmd>, devices: Arc<Mutex<Vec<String>>>, status: Arc<Mutex<String>>, initial: String) {
    *devices.lock().unwrap() = list_devices();
    let mut sink = match open_sink(&initial) {
        Ok((s, name)) => {
            *status.lock().unwrap() = format!("radio on {name}");
            Some(s)
        }
        Err(e) => {
            *status.lock().unwrap() = format!("radio off: {e}");
            None
        }
    };
    while let Ok(cmd) = rx.recv() {
        match cmd {
            RadioCmd::SetDevice(name) => {
                sink = None; // drop the old stream before opening the next one
                match open_sink(&name) {
                    Ok((s, n)) => {
                        *status.lock().unwrap() = format!("radio on {n}");
                        sink = Some(s);
                    }
                    Err(e) => *status.lock().unwrap() = format!("radio off: {e}"),
                }
                *devices.lock().unwrap() = list_devices();
            }
            RadioCmd::Play { bytes, volume } => {
                let Some(s) = sink.as_ref() else { continue };
                match rodio::Decoder::new(Cursor::new(bytes)) {
                    Ok(src) => s.mixer().add(src.amplify(volume)),
                    Err(e) => *status.lock().unwrap() = format!("radio: bad clip: {e}"),
                }
            }
        }
    }
}

pub fn setup(mut commands: Commands, settings: Res<Settings>) {
    let dir = settings::config_dir().join("sounds");
    let mut clips = HashMap::new();
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
        clips.insert(sfx, bytes);
    }
    let (tx, rx) = unbounded();
    let devices = Arc::new(Mutex::new(Vec::new()));
    let status = Arc::new(Mutex::new("radio starting".to_string()));
    {
        let (devices, status, initial) = (devices.clone(), status.clone(), settings.audio_device.clone());
        std::thread::Builder::new()
            .name("radio".into())
            .spawn(move || audio_thread(rx, devices, status, initial))
            .expect("spawn radio thread");
    }
    info!(
        "sounds ready ({} embedded, {} overridden from {})",
        Sfx::ALL.len() - overrides.len(),
        overrides.len(),
        dir.display()
    );
    commands.insert_resource(Sounds {
        clips,
        overrides,
        tx,
        devices,
        status,
    });
}

impl Sounds {
    pub fn set_device(&self, name: &str) {
        let _ = self.tx.send(RadioCmd::SetDevice(name.to_string()));
    }

    pub fn status(&self) -> String {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn device_names(&self) -> Vec<String> {
        self.devices.lock().map(|d| d.clone()).unwrap_or_default()
    }
}

/// Drain the store's sound queue, respecting mute, volume and per-cue cooldowns.
pub fn play(
    time: Res<Time>,
    settings: Res<Settings>,
    sounds: Res<Sounds>,
    mut store: ResMut<Store>,
    mut last: Local<HashMap<Sfx, f32>>,
    mut last_device: Local<Option<String>>,
) {
    if last_device.as_ref() != Some(&settings.audio_device) {
        if last_device.is_some() {
            sounds.set_device(&settings.audio_device);
        }
        *last_device = Some(settings.audio_device.clone());
    }
    let queued: Vec<Sfx> = std::mem::take(&mut store.sfx);
    if queued.is_empty() || !settings.sound {
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
        if let Some(bytes) = sounds.clips.get(&sfx) {
            debug!("radio: play {:?} at volume {:.2}", sfx, settings.volume);
            let _ = sounds.tx.send(RadioCmd::Play {
                bytes: bytes.clone(),
                volume: settings.volume.clamp(0.0, 1.0),
            });
        }
    }
}
