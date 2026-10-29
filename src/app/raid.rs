//! The raid: a wave shooter that lives in cinematic mode, to pass the time while the units work.
//! Alien saucers spawn at the edge of the scene and fly at the units (the data). A click on a
//! saucer fires the turret under the camera; a saucer that reaches a unit drains data integrity.
//! Waves grow, every third wave is a new level with tougher ships. Pause writes the whole raid
//! to disk, so it survives closing the app; leaving cinematic mode pauses it too.

use super::scene::NodeWorldPos;
use super::settings;
use super::sounds::Sfx;
use super::state::Store;
use bevy::picking::events::PointerClick;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Running,
    Paused,
    Over,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Start,
    Pause,
    Resume,
    Replay,
    Stop,
}

/// A ship frozen by a pause.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ShipSave {
    pub pos: [f32; 3],
    pub target: [f32; 3],
    pub hp: u32,
    pub max_hp: u32,
    pub speed: f32,
}

#[derive(Resource, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Raid {
    pub phase: Phase,
    pub wave: u32,
    pub level: u32,
    pub score: u32,
    pub best: u32,
    pub kills: u32,
    /// Data integrity: 1.0 untouched, 0.0 lost.
    pub integrity: f32,
    pub to_spawn: u32,
    pub spawn_in: f32,
    pub wave_gap: f32,
    pub ships: Vec<ShipSave>,
    seed: u64,
    #[serde(skip)]
    pub alive: usize,
    #[serde(skip)]
    pub request: Option<Request>,
    #[serde(skip)]
    pub flash: f32,
    #[serde(skip)]
    pub message: Option<(String, f32)>,
}

impl Default for Raid {
    fn default() -> Self {
        Raid {
            phase: Phase::Idle,
            wave: 0,
            level: 1,
            score: 0,
            best: 0,
            kills: 0,
            integrity: 1.0,
            to_spawn: 0,
            spawn_in: 0.0,
            wave_gap: 0.0,
            ships: Vec::new(),
            seed: 0x9E37_79B9_7F4A_7C15,
            alive: 0,
            request: None,
            flash: 0.0,
            message: None,
        }
    }
}

impl Raid {
    /// A uniform number in [0, 1): xorshift, no crate needed.
    fn rnd(&mut self) -> f32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.seed = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }

    fn say(&mut self, text: &str) {
        self.message = Some((text.to_string(), 2.5));
    }

    /// Fresh raid, keeping the record and the random stream.
    fn fresh(&self) -> Raid {
        Raid {
            best: self.best,
            seed: self.seed,
            ..Raid::default()
        }
    }
}

fn path() -> PathBuf {
    settings::config_dir().join("raid.json")
}

fn save(raid: &Raid) {
    if let Ok(text) = serde_json::to_string_pretty(raid) {
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, text);
    }
}

/// The raid as it was left: a paused one comes back paused, a running one (the app died) too.
pub fn load() -> Raid {
    let mut raid: Raid = std::fs::read_to_string(path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if raid.phase == Phase::Running {
        raid.phase = Phase::Paused;
    }
    raid
}

#[derive(Component)]
pub struct Ship {
    hp: u32,
    max_hp: u32,
    speed: f32,
    target: Vec3,
    wobble: f32,
    draining: f32,
}

#[derive(Component)]
pub struct Debris {
    vel: Vec3,
    ttl: f32,
}

#[derive(Component)]
pub struct Beam {
    ttl: f32,
}

#[derive(Resource)]
pub struct RaidAssets {
    hull: Handle<Mesh>,
    dome: Handle<Mesh>,
    hull_mat: Handle<StandardMaterial>,
    hull_hurt: Handle<StandardMaterial>,
    dome_mat: Handle<StandardMaterial>,
    debris: Handle<Mesh>,
    debris_mat: Handle<StandardMaterial>,
    beam: Handle<Mesh>,
    beam_mat: Handle<StandardMaterial>,
}

fn glow(base: Color, strength: f32, unlit: bool) -> StandardMaterial {
    let lin: LinearRgba = base.into();
    StandardMaterial {
        base_color: base,
        emissive: LinearRgba::new(lin.red * strength, lin.green * strength, lin.blue * strength, 1.0),
        perceptual_roughness: 0.4,
        metallic: 0.2,
        unlit,
        ..default()
    }
}

pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(RaidAssets {
        hull: meshes.add(Sphere::new(0.42)),
        dome: meshes.add(Sphere::new(0.2)),
        hull_mat: materials.add(glow(Color::srgb(0.75, 0.2, 0.55), 1.2, false)),
        hull_hurt: materials.add(glow(Color::srgb(1.0, 0.5, 0.2), 3.0, false)),
        dome_mat: materials.add(glow(Color::srgb(0.4, 1.0, 0.9), 2.5, true)),
        debris: meshes.add(Cuboid::new(0.08, 0.08, 0.08)),
        debris_mat: materials.add(glow(Color::srgb(1.0, 0.6, 0.3), 2.0, true)),
        beam: meshes.add(Cylinder::new(0.025, 1.0)),
        beam_mat: materials.add(glow(Color::srgb(0.6, 1.0, 0.5), 4.0, true)),
    });
    commands.insert_resource(load());
}

#[allow(clippy::too_many_arguments)]
fn spawn_ship(
    commands: &mut Commands,
    assets: &RaidAssets,
    pos: Vec3,
    target: Vec3,
    hp: u32,
    max_hp: u32,
    speed: f32,
    wobble: f32,
) {
    commands
        .spawn((
            Mesh3d(assets.hull.clone()),
            MeshMaterial3d(if hp < max_hp {
                assets.hull_hurt.clone()
            } else {
                assets.hull_mat.clone()
            }),
            Transform::from_translation(pos).with_scale(Vec3::new(1.0, 0.32, 1.0)),
            Pickable::default(),
            Ship {
                hp,
                max_hp,
                speed,
                target,
                wobble,
                draining: 0.0,
            },
        ))
        .with_children(|p| {
            p.spawn((
                Mesh3d(assets.dome.clone()),
                MeshMaterial3d(assets.dome_mat.clone()),
                // Undo the parent's squash so the dome stays round, sitting on top.
                Transform::from_xyz(0.0, 0.9, 0.0).with_scale(Vec3::new(1.0, 1.0 / 0.32, 1.0)),
                Pickable::IGNORE,
            ));
        })
        .observe(on_ship_click);
}

fn spawn_beam(commands: &mut Commands, assets: &RaidAssets, from: Vec3, to: Vec3) {
    let dir = to - from;
    let len = dir.length().max(0.01);
    commands.spawn((
        Mesh3d(assets.beam.clone()),
        MeshMaterial3d(assets.beam_mat.clone()),
        Transform::from_translation((from + to) * 0.5)
            .with_rotation(Quat::from_rotation_arc(Vec3::Y, dir / len))
            .with_scale(Vec3::new(1.0, len, 1.0)),
        Pickable::IGNORE,
        Beam { ttl: 0.12 },
    ));
}

fn despawn_all(commands: &mut Commands, ships: &Query<(Entity, &mut Transform, &mut Ship)>) {
    for (e, _, _) in ships.iter() {
        commands.entity(e).despawn();
    }
}

/// The raid's clock: requests from the HUD and the keyboard, waves, spawning, flight, drains.
#[allow(clippy::too_many_arguments)]
pub fn tick(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut raid: ResMut<Raid>,
    mut store: ResMut<Store>,
    world_pos: Res<NodeWorldPos>,
    assets: Option<Res<RaidAssets>>,
    mut ships: Query<(Entity, &mut Transform, &mut Ship)>,
) {
    let Some(assets) = assets else { return };
    let dt = time.delta_secs().min(0.1);
    if store.hide_ui && !store.typing && keys.just_pressed(KeyCode::KeyP) {
        raid.request = Some(match raid.phase {
            Phase::Running => Request::Pause,
            Phase::Paused => Request::Resume,
            _ => Request::Start,
        });
    }
    // Cinematic mode is the arena: leaving it freezes the raid.
    if !store.hide_ui && raid.phase == Phase::Running {
        raid.request = Some(Request::Pause);
    }
    if let Some(req) = raid.request.take() {
        match req {
            Request::Start | Request::Replay => {
                despawn_all(&mut commands, &ships);
                *raid = raid.fresh();
                raid.phase = Phase::Running;
                raid.wave_gap = 1.0;
                raid.say("RAID · they are coming for the data");
                let _ = std::fs::remove_file(path());
            }
            Request::Pause => {
                if raid.phase == Phase::Running {
                    raid.ships = ships
                        .iter()
                        .map(|(_, t, s)| ShipSave {
                            pos: t.translation.to_array(),
                            target: s.target.to_array(),
                            hp: s.hp,
                            max_hp: s.max_hp,
                            speed: s.speed,
                        })
                        .collect();
                    despawn_all(&mut commands, &ships);
                    raid.phase = Phase::Paused;
                    raid.say("paused");
                    save(&raid);
                }
            }
            Request::Resume => {
                if raid.phase == Phase::Paused {
                    let saved = std::mem::take(&mut raid.ships);
                    for (i, s) in saved.into_iter().enumerate() {
                        spawn_ship(
                            &mut commands,
                            &assets,
                            Vec3::from_array(s.pos),
                            Vec3::from_array(s.target),
                            s.hp,
                            s.max_hp,
                            s.speed,
                            i as f32,
                        );
                    }
                    raid.phase = Phase::Running;
                    raid.say("resumed");
                }
            }
            Request::Stop => {
                despawn_all(&mut commands, &ships);
                *raid = raid.fresh();
                save(&raid);
            }
        }
    }
    if let Some((_, ttl)) = raid.message.as_mut() {
        *ttl -= dt;
        if *ttl <= 0.0 {
            raid.message = None;
        }
    }
    raid.flash = (raid.flash - dt).max(0.0);
    raid.alive = ships.iter().count();
    if raid.phase != Phase::Running {
        return;
    }

    // ---- waves
    if raid.to_spawn == 0 && raid.alive == 0 {
        raid.wave_gap -= dt;
        if raid.wave_gap <= 0.0 {
            raid.wave += 1;
            raid.level = (raid.wave - 1) / 3 + 1;
            raid.to_spawn = 3 + 2 * raid.wave;
            raid.spawn_in = 0.3;
            raid.wave_gap = 3.0;
            let (w, l) = (raid.wave, raid.level);
            raid.say(&format!("WAVE {w} · LEVEL {l}"));
            if w > 1 {
                store.play(Sfx::Fanfare);
            }
        }
    } else if raid.to_spawn > 0 {
        raid.spawn_in -= dt;
        if raid.spawn_in <= 0.0 {
            let targets: Vec<Vec3> = world_pos.0.values().copied().collect();
            let center = if targets.is_empty() {
                Vec3::ZERO
            } else {
                targets.iter().sum::<Vec3>() / targets.len() as f32
            };
            let target = if targets.is_empty() {
                Vec3::ZERO
            } else {
                let i = (raid.rnd() * targets.len() as f32) as usize;
                targets[i.min(targets.len() - 1)]
            };
            let theta = raid.rnd() * std::f32::consts::TAU;
            let phi = (raid.rnd() - 0.3) * 1.2;
            let pos = center + Vec3::new(theta.cos() * phi.cos(), phi.sin(), theta.sin() * phi.cos()) * 16.0;
            let level = raid.level;
            let hp = level.min(5);
            let speed = 1.2 + 0.25 * level as f32 + raid.rnd() * 0.4;
            let wobble = raid.rnd() * 6.0;
            spawn_ship(&mut commands, &assets, pos, target, hp, hp, speed, wobble);
            raid.to_spawn -= 1;
            raid.spawn_in = (1.4 - 0.1 * level as f32).max(0.45);
        }
    }

    // ---- flight and drains
    let t = time.elapsed_secs();
    let mut drained = 0u32;
    for (e, mut tf, mut ship) in &mut ships {
        let to = ship.target - tf.translation;
        let dist = to.length();
        if dist > 0.7 {
            let dir = to / dist;
            let side = dir.cross(Vec3::Y).normalize_or_zero();
            tf.translation += dir * ship.speed * dt + side * (t * 3.0 + ship.wobble).sin() * 0.6 * dt;
            tf.rotation = Quat::from_rotation_y(t * 2.0 + ship.wobble);
        } else {
            ship.draining += dt;
            tf.rotation = Quat::from_rotation_y(t * 7.0);
            if ship.draining >= 1.5 {
                commands.entity(e).despawn();
                drained += 1;
            }
        }
    }
    if drained > 0 {
        raid.integrity = (raid.integrity - 0.1 * drained as f32).max(0.0);
        raid.flash = 0.6;
        store.play(Sfx::Casualties);
        if raid.integrity <= 0.001 {
            raid.phase = Phase::Over;
            raid.best = raid.best.max(raid.score);
            despawn_all(&mut commands, &ships);
            raid.say("DATA LOST · the raid is over");
            store.play(Sfx::Alarm);
            save(&raid);
        }
    }
}

/// A click on a saucer: the turret under the camera fires; one hit per click, debris on a kill.
fn on_ship_click(
    ev: On<PointerClick>,
    mut commands: Commands,
    mut raid: ResMut<Raid>,
    mut store: ResMut<Store>,
    assets: Res<RaidAssets>,
    mut ships: Query<(&Transform, &mut Ship, &mut MeshMaterial3d<StandardMaterial>)>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
) {
    if raid.phase != Phase::Running {
        return;
    }
    let Ok((tf, mut ship, mut mat)) = ships.get_mut(ev.entity) else {
        return;
    };
    let hit = tf.translation;
    if let Ok(c) = cam.single() {
        let from = c.translation() + c.forward() * 1.5 - c.up() * 0.9;
        spawn_beam(&mut commands, &assets, from, hit);
    }
    store.play(Sfx::Blip);
    ship.hp = ship.hp.saturating_sub(1);
    if ship.hp == 0 {
        commands.entity(ev.entity).despawn();
        raid.kills += 1;
        raid.score += 10 * raid.level;
        for i in 0..8 {
            let a = i as f32 / 8.0 * std::f32::consts::TAU;
            let vel = Vec3::new(a.cos() * 2.5, 1.5 + raid.rnd() * 2.0, a.sin() * 2.5);
            commands.spawn((
                Mesh3d(assets.debris.clone()),
                MeshMaterial3d(assets.debris_mat.clone()),
                Transform::from_translation(hit),
                Pickable::IGNORE,
                Debris { vel, ttl: 0.7 },
            ));
        }
    } else {
        mat.0 = assets.hull_hurt.clone();
    }
}

/// Beams fade in a few frames; debris flies out, falls and shrinks away.
pub fn fx(
    mut commands: Commands,
    time: Res<Time>,
    mut debris: Query<(Entity, &mut Transform, &mut Debris)>,
    mut beams: Query<(Entity, &mut Beam)>,
) {
    let dt = time.delta_secs().min(0.1);
    for (e, mut b) in &mut beams {
        b.ttl -= dt;
        if b.ttl <= 0.0 {
            commands.entity(e).despawn();
        }
    }
    for (e, mut tf, mut d) in &mut debris {
        d.ttl -= dt;
        if d.ttl <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        d.vel.y -= 4.0 * dt;
        tf.translation += d.vel * dt;
        tf.scale = Vec3::splat((d.ttl / 0.7).clamp(0.05, 1.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waves_grow_and_levels_step_every_three() {
        let mut r = Raid::default();
        for wave in 1..=7u32 {
            r.wave = wave;
            r.level = (r.wave - 1) / 3 + 1;
            r.to_spawn = 3 + 2 * r.wave;
            match wave {
                1..=3 => assert_eq!(r.level, 1),
                4..=6 => assert_eq!(r.level, 2),
                _ => assert_eq!(r.level, 3),
            }
        }
        assert_eq!(r.to_spawn, 17);
        let a = r.rnd();
        let b = r.rnd();
        assert!((0.0..1.0).contains(&a) && (0.0..1.0).contains(&b) && a != b);
    }

    #[test]
    fn a_saved_raid_round_trips_and_comes_back_paused() {
        let mut r = Raid {
            phase: Phase::Running,
            wave: 4,
            score: 120,
            ..Raid::default()
        };
        r.ships.push(ShipSave {
            pos: [1.0, 2.0, 3.0],
            target: [0.0; 3],
            hp: 2,
            max_hp: 2,
            speed: 1.5,
        });
        let text = serde_json::to_string(&r).unwrap();
        let mut back: Raid = serde_json::from_str(&text).unwrap();
        if back.phase == Phase::Running {
            back.phase = Phase::Paused;
        }
        assert_eq!(back.phase, Phase::Paused);
        assert_eq!(back.wave, 4);
        assert_eq!(back.ships.len(), 1);
        assert_eq!(back.request, None, "requests are not persisted");
    }
}
