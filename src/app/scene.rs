//! The sandbox: a commander in the middle, plastic soldiers on orbit rings, radio beams with
//! tracer rounds while a call runs, sand dust, a night sky.

use super::settings::Settings;
use super::sounds::Sfx;
use super::state::Store;
use super::theme;
use agent_globe::data::files::{self, FileStat};
use agent_globe::data::layout::{self, Layout};
use agent_globe::data::model::{ROOT_AGENT, Status, now_millis};
use bevy::picking::events::{PointerClick, PointerOut, PointerOver};
use bevy::prelude::*;
use std::collections::HashMap;
use std::f32::consts::TAU;

#[derive(Component)]
pub struct NodeAgent {
    pub session: String,
    pub id: String,
}

/// Orbit parameters of a node; the animated angle is derived from these each frame.
#[derive(Component)]
pub struct Orbit {
    pub depth: u32,
    pub base_angle: f32,
}

/// A rotating ring around a node. Node halos show state; core halos are always on.
#[derive(Component)]
pub struct Halo {
    pub session: String,
    pub agent_id: String,
    pub tilt: f32,
    pub speed: f32,
    pub is_core: bool,
}

/// One of the session's hottest files, on the ring outside the deepest orbit.
#[derive(Component)]
pub struct FileNode {
    pub path: String,
    pub angle: f32,
}

#[derive(Resource, Default)]
pub struct FileEntities(pub HashMap<String, Entity>);

/// The hot-file list of the current session, rebuilt once a second, and where each node is.
#[derive(Resource, Default)]
pub struct FileScene {
    pub files: Vec<FileStat>,
    pub depth: u32,
    pub positions: HashMap<String, Vec3>,
    next_rebuild: f32,
}

/// How many files the ring shows, how long a touch keeps its beam, how long a node glows after an edit.
pub const FILE_RING_MAX: usize = 24;
pub const BEAM_MS: i64 = 45_000;
pub const FILE_GLOW_MS: i64 = 20_000;

/// One of the two clickable circles around a satellite constellation: click to go there.
#[derive(Component)]
pub struct SatelliteRing {
    pub session: String,
    pub radius: f32,
    pub tilt: f32,
    /// The invisible sphere over the whole constellation: the click target. Rings are the visual.
    pub hull: bool,
}

#[derive(Resource, Default)]
pub struct SatelliteRings(pub HashMap<String, Vec<Entity>>);

#[derive(Component)]
pub struct OrbitRing {
    pub session: String,
    pub depth: u32,
}

#[derive(Component)]
pub struct Dust {
    radius: f32,
    height: f32,
    phase: f32,
    speed: f32,
}

#[derive(Component)]
pub struct Star;

/// Unit entities keyed by (session id, agent id).
#[derive(Resource, Default)]
pub struct NodeEntities(pub HashMap<(String, String), Entity>);

/// Ring entities keyed by (session id, depth).
#[derive(Resource, Default)]
pub struct RingEntities(pub HashMap<(String, u32), Entity>);

/// Where one shown session sits in the world and how it is laid out.
pub struct SessionScene {
    pub layout: Layout,
    pub agent_count: usize,
    pub offset: Vec3,
    pub scale: f32,
    dirty: bool,
}

#[derive(Resource, Default)]
pub struct SceneLayout {
    pub per: HashMap<String, SessionScene>,
}

/// World position of every unit, keyed by (session id, agent id).
#[derive(Resource, Default)]
pub struct NodeWorldPos(pub HashMap<(String, String), Vec3>);

/// Where the i-th satellite constellation of `count` sits around the current session.
pub fn satellite_offset(index: usize, count: usize) -> Vec3 {
    let a = TAU * index as f32 / count.max(1) as f32 + 0.6;
    Vec3::new(
        theme::SATELLITE_RADIUS * a.cos(),
        -0.8,
        theme::SATELLITE_RADIUS * a.sin(),
    )
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum MatKey {
    Root,
    Active,
    Idle,
    Done,
    Error,
    Stalled,
    Selected,
    Hover,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum HaloKey {
    Selected,
    Hover,
    Active,
    Stalled,
    Core,
}

#[derive(Resource)]
pub struct SceneAssets {
    soldier_mesh: Handle<Mesh>,
    helmet_mesh: Handle<Mesh>,
    base_mesh: Handle<Mesh>,
    commander_mesh: Handle<Mesh>,
    commander_base_mesh: Handle<Mesh>,
    halo_mesh: Handle<Mesh>,
    core_halo_mesh: Handle<Mesh>,
    materials: HashMap<MatKey, Handle<StandardMaterial>>,
    halos: HashMap<HaloKey, Handle<StandardMaterial>>,
    base_material: Handle<StandardMaterial>,
    ring_material: Handle<StandardMaterial>,
    file_mesh: Handle<Mesh>,
    file_material: Handle<StandardMaterial>,
    file_hot_material: Handle<StandardMaterial>,
    hull_material: Handle<StandardMaterial>,
    hull_hover_material: Handle<StandardMaterial>,
}

/// Toy plastic: glossy, lit, with an emissive push for the glowing states.
fn plastic(base: Color, glow: f32) -> StandardMaterial {
    let lin: LinearRgba = base.into();
    StandardMaterial {
        base_color: base,
        emissive: LinearRgba::new(lin.red * glow, lin.green * glow, lin.blue * glow, 1.0),
        perceptual_roughness: 0.38,
        metallic: 0.0,
        reflectance: 0.45,
        ..default()
    }
}

/// Pure light: unlit, emissive only. Bloom turns it into a glow.
fn light(base: Color, strength: f32) -> StandardMaterial {
    let lin: LinearRgba = base.into();
    StandardMaterial {
        base_color: base,
        emissive: LinearRgba::new(lin.red * strength, lin.green * strength, lin.blue * strength, 1.0),
        unlit: true,
        ..default()
    }
}

/// Tiny deterministic PRNG so the sky is identical every launch without a rand dependency.
struct XorShift(u32);
impl XorShift {
    fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32) / (u32::MAX as f32)
    }
}

pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut gizmo_store: ResMut<GizmoConfigStore>,
) {
    let (gizmo_cfg, _) = gizmo_store.config_mut::<DefaultGizmoConfigGroup>();
    gizmo_cfg.line.width = 2.2;

    // Soldier: capsule body, helmet, flat oval base like the toy.
    let soldier_mesh = meshes.add(Capsule3d {
        radius: 0.13,
        half_length: 0.15,
    });
    let helmet_mesh = meshes.add(Sphere::new(0.12).mesh().ico(3).unwrap());
    let base_mesh = meshes.add(Cylinder {
        radius: 0.30,
        half_height: 0.025,
    });
    let commander_mesh = meshes.add(Capsule3d {
        radius: 0.24,
        half_length: 0.30,
    });
    let commander_base_mesh = meshes.add(Cylinder {
        radius: 0.62,
        half_height: 0.04,
    });
    let halo_mesh = meshes.add(Torus {
        minor_radius: 0.022,
        major_radius: 0.46,
    });
    let core_halo_mesh = meshes.add(Torus {
        minor_radius: 0.03,
        major_radius: 1.05,
    });

    let mut mats = HashMap::new();
    mats.insert(MatKey::Root, materials.add(plastic(theme::NODE_ROOT, 4.0)));
    mats.insert(MatKey::Active, materials.add(plastic(theme::NODE_ACTIVE, 40.0)));
    mats.insert(MatKey::Idle, materials.add(plastic(theme::NODE_IDLE, 1.5)));
    mats.insert(MatKey::Done, materials.add(plastic(theme::NODE_DONE, 0.0)));
    mats.insert(MatKey::Error, materials.add(plastic(theme::NODE_ERROR, 70.0)));
    mats.insert(MatKey::Stalled, materials.add(plastic(theme::NODE_STALLED, 60.0)));
    mats.insert(MatKey::Selected, materials.add(plastic(theme::NODE_SELECTED, 80.0)));
    mats.insert(MatKey::Hover, materials.add(plastic(theme::NODE_HOVER, 40.0)));

    let mut halos = HashMap::new();
    halos.insert(HaloKey::Selected, materials.add(light(theme::NODE_SELECTED, 140.0)));
    halos.insert(HaloKey::Hover, materials.add(light(theme::NODE_HOVER, 110.0)));
    halos.insert(HaloKey::Active, materials.add(light(theme::NODE_ACTIVE, 120.0)));
    halos.insert(HaloKey::Stalled, materials.add(light(theme::NODE_STALLED, 120.0)));
    halos.insert(HaloKey::Core, materials.add(light(theme::SAND, 110.0)));

    let base_material = materials.add(plastic(theme::BASE, 0.5));
    let ring_material = materials.add(StandardMaterial {
        alpha_mode: AlphaMode::Blend,
        ..light(theme::RING_MESH, 25.0)
    });

    // Night sky: small faint cubes far away.
    let file_mesh = meshes.add(Cuboid::new(0.22, 0.22, 0.22));
    let file_material = materials.add(plastic(theme::KHAKI, 6.0));
    let file_hot_material = materials.add(plastic(theme::PLASTIC_LIGHT, 60.0));
    // Fully transparent but still picked by the pointer; a faint glow while hovered.
    let hull_material = materials.add(StandardMaterial {
        base_color: Color::NONE,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let hull_hover_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.87, 0.78, 0.52, 0.06),
        emissive: LinearRgba::new(0.6, 0.5, 0.2, 1.0),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let star_mesh = meshes.add(Cuboid::new(0.14, 0.14, 0.14));
    let star_mat = materials.add(light(theme::STAR, 28.0));
    let mut rng = XorShift(0x9E37_79B9);
    for _ in 0..theme::STAR_COUNT {
        let u = rng.next_f32() * 2.0 - 1.0;
        let phi = rng.next_f32() * TAU;
        let r = 70.0 + rng.next_f32() * 60.0;
        let s = (1.0 - u * u).sqrt();
        let pos = Vec3::new(r * s * phi.cos(), r * u, r * s * phi.sin());
        let size = 0.6 + rng.next_f32() * 1.6;
        commands.spawn((
            Mesh3d(star_mesh.clone()),
            MeshMaterial3d(star_mat.clone()),
            Transform::from_translation(pos).with_scale(Vec3::splat(size)),
            Star,
            Pickable::IGNORE,
        ));
    }

    // Sand grains drifting around the rings.
    let dust_mesh = meshes.add(Cuboid::new(0.05, 0.05, 0.05));
    let dust_mat = materials.add(light(theme::DUST, 14.0));
    for _ in 0..theme::DUST_COUNT {
        let radius = 1.5 + rng.next_f32() * 13.0;
        let height = (rng.next_f32() - 0.5) * 2.0;
        let phase = rng.next_f32() * TAU;
        let speed = 0.012 + rng.next_f32() * 0.04;
        commands.spawn((
            Mesh3d(dust_mesh.clone()),
            MeshMaterial3d(dust_mat.clone()),
            Transform::from_translation(Vec3::new(radius * phase.cos(), height, radius * phase.sin())),
            Dust {
                radius,
                height,
                phase,
                speed,
            },
            Pickable::IGNORE,
        ));
    }

    commands.insert_resource(SceneAssets {
        soldier_mesh,
        helmet_mesh,
        base_mesh,
        commander_mesh,
        commander_base_mesh,
        halo_mesh,
        core_halo_mesh,
        materials: mats,
        halos,
        base_material,
        ring_material,
        file_mesh,
        file_material,
        file_hot_material,
        hull_material,
        hull_hover_material,
    });
}

/// Keep the file ring in step with what the current session's agents are touching.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn sync_files(
    mut commands: Commands,
    time: Res<Time>,
    store: Res<Store>,
    settings: Res<Settings>,
    assets: Res<SceneAssets>,
    scene: Res<SceneLayout>,
    mut fscene: ResMut<FileScene>,
    mut fents: ResMut<FileEntities>,
    mut nodes: Query<(&mut FileNode, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    let now = now_millis();
    let session = if settings.files_ring {
        store.current_session()
    } else {
        None
    };
    let Some(session) = session else {
        for (_, e) in fents.0.drain() {
            commands.entity(e).despawn();
        }
        fscene.files.clear();
        fscene.positions.clear();
        return;
    };
    if t >= fscene.next_rebuild {
        fscene.next_rebuild = t + 1.0;
        fscene.files = files::hot_files(session, FILE_RING_MAX, now, BEAM_MS);
        let max_depth = scene.per.get(&session.id).map(|e| e.layout.max_depth).unwrap_or(0);
        fscene.depth = max_depth + 1;
        let keep: std::collections::HashSet<&str> = fscene.files.iter().map(|f| f.path.as_str()).collect();
        fents.0.retain(|path, e| {
            let k = keep.contains(path.as_str());
            if !k {
                commands.entity(*e).despawn();
            }
            k
        });
        // Angles follow the alphabetical order of the chosen set, so nodes do not jump as heat shifts.
        let mut order: Vec<&str> = fscene.files.iter().map(|f| f.path.as_str()).collect();
        order.sort_unstable();
        let n = order.len().max(1) as f32;
        for (i, path) in order.iter().enumerate() {
            let angle = TAU * i as f32 / n;
            match fents.0.get(*path) {
                Some(&e) => {
                    if let Ok((mut node, _, _)) = nodes.get_mut(e) {
                        node.angle = angle;
                    }
                }
                None => {
                    let mut ec = commands.spawn((
                        Mesh3d(assets.file_mesh.clone()),
                        MeshMaterial3d(assets.file_material.clone()),
                        Transform::from_translation(Vec3::ZERO).with_scale(Vec3::splat(0.01)),
                        FileNode {
                            path: path.to_string(),
                            angle,
                        },
                    ));
                    ec.observe(on_file_click);
                    fents.0.insert(path.to_string(), ec.id());
                }
            }
        }
    }
    let depth = fscene.depth;
    let heat_of: HashMap<&str, (u32, i64)> = fscene
        .files
        .iter()
        .map(|f| (f.path.as_str(), (f.heat(), f.last_at)))
        .collect();
    let mut positions = HashMap::new();
    for (node, mut tf, mut mat) in nodes.iter_mut() {
        let (heat, last_at) = heat_of.get(node.path.as_str()).copied().unwrap_or((1, 0));
        let angle = orbit_angle(node.angle, depth, t, settings.drift_speed);
        let target = Vec3::from(layout::orbit_point(depth, angle));
        tf.translation = tf.translation.lerp(target, (dt * 4.0).min(1.0));
        let want = 0.7 + (heat.min(24) as f32 / 24.0) * 0.9;
        let sc = tf.scale.x + (want - tf.scale.x) * (dt * 4.0).min(1.0);
        tf.scale = Vec3::splat(sc);
        tf.rotation = Quat::from_rotation_y(t * 0.4 + node.angle) * Quat::from_rotation_x(0.6);
        let hot = now - last_at <= FILE_GLOW_MS;
        let handle = if hot {
            &assets.file_hot_material
        } else {
            &assets.file_material
        };
        if mat.0 != *handle {
            mat.0 = handle.clone();
        }
        positions.insert(node.path.clone(), tf.translation);
    }
    fscene.positions = positions;
}

/// Beams from units to the files they touched in the last `BEAM_MS`: bright for edits, faint for reads, fading with age.
pub fn draw_file_beams(mut gizmos: Gizmos, store: Res<Store>, fscene: Res<FileScene>, world_pos: Res<NodeWorldPos>) {
    let Some(current) = store.current.clone() else { return };
    let now = now_millis();
    for f in &fscene.files {
        let Some(&to) = fscene.positions.get(&f.path) else {
            continue;
        };
        for touch in &f.touches {
            let Some(&from) = world_pos.0.get(&(current.clone(), touch.agent_id.clone())) else {
                continue;
            };
            let age = ((now - touch.at) as f32 / BEAM_MS as f32).clamp(0.0, 1.0);
            let alpha = (1.0 - age) * if touch.edit { 0.9 } else { 0.35 };
            let base = if touch.edit { theme::LINK_ACTIVE } else { theme::LINK };
            let c = base.to_srgba();
            let color = Color::srgba(c.red, c.green, c.blue, alpha);
            let pts = layout::curve(from.into(), to.into(), 0.8, 20);
            let v: Vec<Vec3> = pts.iter().map(|p| Vec3::from(*p)).collect();
            gizmos.linestrip(v, color);
        }
    }
}

/// Screen positions of the file nodes, for their labels.
pub fn project_files(
    mut store: ResMut<Store>,
    settings: Res<Settings>,
    fscene: Res<FileScene>,
    cams: Query<(&Camera, &GlobalTransform), With<bevy_panorbit_camera::PanOrbitCamera>>,
    nodes: Query<(&FileNode, &GlobalTransform)>,
) {
    let Ok((cam, cam_tf)) = cams.single() else { return };
    let zoom = settings.ui_scale.max(0.1);
    let mut out = Vec::new();
    for (node, tf) in &nodes {
        let Some(f) = fscene.files.iter().find(|f| f.path == node.path) else {
            continue;
        };
        if let Ok(p) = cam.world_to_viewport(cam_tf, tf.translation() - Vec3::Y * 0.2) {
            out.push((
                f.path.clone(),
                f.label.clone(),
                p.x / zoom,
                p.y / zoom,
                f.reads,
                f.edits,
            ));
        }
    }
    store.file_screen = out;
}

fn on_file_click(ev: On<PointerClick>, nodes: Query<&FileNode>, mut store: ResMut<Store>) {
    if let Ok(n) = nodes.get(ev.entity) {
        store.pending_open_file = Some(n.path.clone());
    }
}

fn is_stalled(store: &Store, agent: &agent_globe::data::model::Agent) -> bool {
    agent
        .calls
        .iter()
        .any(|c| c.ended.is_none() && store.stalled.contains(&c.id))
}

fn material_for(store: &Store, agent: &agent_globe::data::model::Agent, now: i64, is_current: bool) -> MatKey {
    if is_current && store.selected.as_deref() == Some(agent.id.as_str()) {
        return MatKey::Selected;
    }
    if is_current && store.hovered.as_deref() == Some(agent.id.as_str()) {
        return MatKey::Hover;
    }
    if is_stalled(store, agent) {
        return MatKey::Stalled;
    }
    if agent.last_call_failed() && agent.ended.is_some() {
        return MatKey::Error;
    }
    if agent.depth == 0 {
        return MatKey::Root;
    }
    match agent.status(now, theme::ACTIVE_WINDOW_MS) {
        Status::Active => MatKey::Active,
        Status::Idle => MatKey::Idle,
        Status::Done => MatKey::Done,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn sync_nodes(
    mut commands: Commands,
    mut store: ResMut<Store>,
    settings: Res<Settings>,
    assets: Res<SceneAssets>,
    mut entities: ResMut<NodeEntities>,
    mut rings: ResMut<RingEntities>,
    mut scene: ResMut<SceneLayout>,
    mut nodes: Query<(&NodeAgent, &mut Orbit, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let now = now_millis();
    let shown = store.shown_sessions(now, settings.satellites);
    let dirty = store.layout_dirty;

    // Sessions that left the scene take their units and rings with them.
    entities.0.retain(|(sid, _), e| {
        let keep = shown.contains(sid);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
    rings.0.retain(|(sid, _), e| {
        let keep = shown.contains(sid);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
    scene.per.retain(|sid, _| shown.contains(sid));

    let satellites = shown.len().saturating_sub(1);
    for (i, sid) in shown.iter().enumerate() {
        let Some(session) = store.sessions.get(sid) else {
            continue;
        };
        let (offset, scale) = if i == 0 {
            (Vec3::ZERO, 1.0)
        } else {
            (satellite_offset(i - 1, satellites), theme::SATELLITE_SCALE)
        };
        let entry = scene.per.entry(sid.clone()).or_insert_with(|| SessionScene {
            layout: layout::layout(session),
            agent_count: session.agents.len(),
            offset,
            scale,
            dirty: true,
        });
        if dirty || entry.agent_count != session.agents.len() {
            entry.layout = layout::layout(session);
            entry.agent_count = session.agents.len();
            entry.dirty = true;
        }
        entry.offset = offset;
        entry.scale = scale;

        for depth in 1..=entry.layout.max_depth {
            let key = (sid.clone(), depth);
            if rings.0.contains_key(&key) {
                continue;
            }
            let e = commands
                .spawn((
                    OrbitRing {
                        session: sid.clone(),
                        depth,
                    },
                    Transform::from_translation(offset + Vec3::Y * layout::ring_height(depth) * scale)
                        .with_scale(Vec3::splat(scale)),
                    Visibility::default(),
                    Pickable::IGNORE,
                ))
                .id();
            rings.0.insert(key, e);
        }

        for n in &entry.layout.nodes {
            let key = (sid.clone(), n.agent_id.clone());
            if entities.0.contains_key(&key) {
                continue;
            }
            let is_core = n.depth == 0;
            let (body, base, base_y, helmet_y) = if is_core {
                (
                    assets.commander_mesh.clone(),
                    assets.commander_base_mesh.clone(),
                    -0.56,
                    0.62,
                )
            } else {
                (assets.soldier_mesh.clone(), assets.base_mesh.clone(), -0.30, 0.34)
            };
            // New units deploy from their commander and march out to their ring.
            let mut ec = commands.spawn((
                Mesh3d(body),
                MeshMaterial3d(assets.materials[&MatKey::Idle].clone()),
                Transform::from_translation(offset).with_scale(Vec3::splat(0.01)),
                NodeAgent {
                    session: sid.clone(),
                    id: n.agent_id.clone(),
                },
                Orbit {
                    depth: n.depth,
                    base_angle: n.angle,
                },
            ));
            ec.observe(on_click).observe(on_over).observe(on_out);
            let id = n.agent_id.clone();
            let session_id = sid.clone();
            ec.with_children(|p| {
                p.spawn((
                    Mesh3d(base),
                    MeshMaterial3d(assets.base_material.clone()),
                    Transform::from_xyz(0.0, base_y, 0.0),
                    Pickable::IGNORE,
                ));
                if !is_core {
                    p.spawn((
                        Mesh3d(assets.helmet_mesh.clone()),
                        MeshMaterial3d(assets.materials[&MatKey::Idle].clone()),
                        Transform::from_xyz(0.0, helmet_y, 0.0).with_scale(Vec3::new(1.15, 0.7, 1.15)),
                        Pickable::IGNORE,
                    ));
                }
                if is_core {
                    for (tilt, speed) in [(0.55_f32, 0.45_f32), (-0.55, -0.35)] {
                        p.spawn((
                            Mesh3d(assets.core_halo_mesh.clone()),
                            MeshMaterial3d(assets.halos[&HaloKey::Core].clone()),
                            Transform::from_rotation(Quat::from_rotation_x(tilt)),
                            Halo {
                                session: session_id.clone(),
                                agent_id: id.clone(),
                                tilt,
                                speed,
                                is_core: true,
                            },
                            Pickable::IGNORE,
                        ));
                    }
                } else {
                    p.spawn((
                        Mesh3d(assets.halo_mesh.clone()),
                        MeshMaterial3d(assets.halos[&HaloKey::Active].clone()),
                        Transform::from_rotation(Quat::from_rotation_x(1.2)),
                        Visibility::Hidden,
                        Halo {
                            session: session_id.clone(),
                            agent_id: id.clone(),
                            tilt: 1.2,
                            speed: 1.6,
                            is_core: false,
                        },
                        Pickable::IGNORE,
                    ));
                }
            });
            entities.0.insert(key, ec.id());
        }
    }

    for (node, mut orbit, mut mat) in nodes.iter_mut() {
        let Some(entry) = scene.per.get(&node.session) else {
            continue;
        };
        if entry.dirty
            && let Some(n) = entry.layout.node(&node.id)
        {
            orbit.depth = n.depth;
            orbit.base_angle = n.angle;
        }
        let is_current = store.current.as_deref() == Some(node.session.as_str());
        if let Some(agent) = store.sessions.get(&node.session).and_then(|s| s.agent(&node.id)) {
            let handle = &assets.materials[&material_for(&store, agent, now, is_current)];
            if mat.0 != *handle {
                mat.0 = handle.clone();
            }
        }
    }
    for e in scene.per.values_mut() {
        e.dirty = false;
    }
    store.layout_dirty = false;
}

#[allow(clippy::type_complexity)]
pub fn sync_helmets(
    bodies: Query<
        (&MeshMaterial3d<StandardMaterial>, &Children),
        (With<NodeAgent>, Changed<MeshMaterial3d<StandardMaterial>>),
    >,
    mut parts: Query<&mut MeshMaterial3d<StandardMaterial>, (Without<NodeAgent>, Without<Halo>)>,
    assets: Res<SceneAssets>,
) {
    for (mat, children) in &bodies {
        for child in children.iter() {
            if let Ok(mut m) = parts.get_mut(child)
                && m.0 != assets.base_material
                && m.0 != mat.0
            {
                m.0 = mat.0.clone();
            }
        }
    }
}

/// Give freshly spawned ring entities their torus mesh (radius depends on depth).
pub fn dress_rings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    assets: Res<SceneAssets>,
    fresh: Query<(Entity, &OrbitRing), Without<Mesh3d>>,
    fresh_sat: Query<(Entity, &SatelliteRing), Without<Mesh3d>>,
) {
    for (e, ring) in &fresh {
        let mesh = meshes.add(Torus {
            minor_radius: 0.012,
            major_radius: layout::ring_radius(ring.depth),
        });
        commands
            .entity(e)
            .insert((Mesh3d(mesh), MeshMaterial3d(assets.ring_material.clone())));
    }
    for (e, ring) in &fresh_sat {
        if ring.hull {
            let mesh = meshes.add(Sphere::new(ring.radius).mesh().ico(2).unwrap());
            commands
                .entity(e)
                .insert((Mesh3d(mesh), MeshMaterial3d(assets.hull_material.clone())));
        } else {
            let mesh = meshes.add(Torus {
                minor_radius: 0.05,
                major_radius: ring.radius,
            });
            commands.entity(e).insert((
                Mesh3d(mesh),
                MeshMaterial3d(assets.ring_material.clone()),
                Pickable::IGNORE,
            ));
        }
    }
}

/// Two circles around every satellite constellation (never around the current one), spawned when a
/// satellite appears and removed when it becomes current or leaves the scene.
pub fn sync_satellite_rings(
    mut commands: Commands,
    store: Res<Store>,
    settings: Res<Settings>,
    scene: Res<SceneLayout>,
    mut rings: ResMut<SatelliteRings>,
) {
    let now = now_millis();
    let shown = store.shown_sessions(now, settings.satellites);
    let satellites: Vec<&String> = shown.iter().skip(1).collect();
    rings.0.retain(|sid, ents| {
        let keep = satellites.contains(&sid);
        if !keep {
            for e in ents.drain(..) {
                commands.entity(e).despawn();
            }
        }
        keep
    });
    for sid in satellites {
        if rings.0.contains_key(sid) {
            continue;
        }
        let Some(sc) = scene.per.get(sid) else { continue };
        let base = layout::ring_radius(sc.layout.max_depth.max(1)) + 1.4;
        let mut ents = Vec::new();
        // two visible circles plus one invisible hull that catches clicks anywhere on the constellation
        for (radius, tilt, hull) in [
            (base, 0.0_f32, false),
            (base + 0.7, 0.35, false),
            (base + 0.9, 0.0, true),
        ] {
            let mut ec = commands.spawn((
                SatelliteRing {
                    session: sid.clone(),
                    radius,
                    tilt,
                    hull,
                },
                Transform::from_translation(sc.offset).with_scale(Vec3::splat(0.01)),
                Visibility::default(),
            ));
            ec.observe(on_satellite_click)
                .observe(on_satellite_over)
                .observe(on_satellite_out);
            ents.push(ec.id());
        }
        rings.0.insert(sid.clone(), ents);
    }
}

/// Keep satellite rings on their constellation, spinning slowly, brighter while hovered.
pub fn animate_satellite_rings(
    time: Res<Time>,
    store: Res<Store>,
    assets: Res<SceneAssets>,
    scene: Res<SceneLayout>,
    mut rings: Query<(
        &SatelliteRing,
        &mut Transform,
        Option<&mut MeshMaterial3d<StandardMaterial>>,
    )>,
) {
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    for (ring, mut tf, mat) in rings.iter_mut() {
        let Some(sc) = scene.per.get(&ring.session) else {
            continue;
        };
        tf.translation = tf.translation.lerp(sc.offset, (dt * 4.0).min(1.0));
        let s = tf.scale.x + (sc.scale - tf.scale.x) * (dt * 5.0).min(1.0);
        tf.scale = Vec3::splat(s);
        tf.rotation = Quat::from_rotation_y(t * 0.15) * Quat::from_rotation_x(ring.tilt);
        if let Some(mut mat) = mat {
            let hovered = store.hovered_session.as_deref() == Some(ring.session.as_str());
            let want = match (ring.hull, hovered) {
                (true, true) => &assets.hull_hover_material,
                (true, false) => &assets.hull_material,
                (false, true) => &assets.halos[&HaloKey::Core],
                (false, false) => &assets.ring_material,
            };
            if mat.0 != *want {
                mat.0 = want.clone();
            }
        }
    }
}

fn on_satellite_click(ev: On<PointerClick>, rings: Query<&SatelliteRing>, mut store: ResMut<Store>) {
    if let Ok(r) = rings.get(ev.entity) {
        store.hide_ui = false;
        store.pending_open = Some(r.session.clone());
        store.pending_select = None;
    }
}

fn on_satellite_over(ev: On<PointerOver>, rings: Query<&SatelliteRing>, mut store: ResMut<Store>) {
    if let Ok(r) = rings.get(ev.entity) {
        store.hovered_session = Some(r.session.clone());
    }
}

fn on_satellite_out(ev: On<PointerOut>, rings: Query<&SatelliteRing>, mut store: ResMut<Store>) {
    if let Ok(r) = rings.get(ev.entity)
        && store.hovered_session.as_deref() == Some(r.session.as_str())
    {
        store.hovered_session = None;
    }
}

/// Slow orbital drift per ring, so the sandbox is never static.
fn orbit_angle(base: f32, depth: u32, t: f32, speed: f32) -> f32 {
    if depth == 0 {
        return 0.0;
    }
    base + t * 0.06 * speed / depth as f32
}

/// March nodes toward their orbit position, bob the busy ones, publish world positions.
#[allow(clippy::type_complexity)]
pub fn animate_nodes(
    time: Res<Time>,
    store: Res<Store>,
    settings: Res<Settings>,
    scene: Res<SceneLayout>,
    mut world_pos: ResMut<NodeWorldPos>,
    mut nodes: Query<(&NodeAgent, &Orbit, &mut Transform)>,
    mut rings: Query<(&OrbitRing, &mut Transform), Without<NodeAgent>>,
) {
    let now = now_millis();
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    world_pos.0.clear();
    for (node, orbit, mut tf) in nodes.iter_mut() {
        let Some(sc) = scene.per.get(&node.session) else {
            continue;
        };
        let angle = orbit_angle(orbit.base_angle, orbit.depth, t, settings.drift_speed);
        let mut target = sc.offset + Vec3::from(layout::orbit_point(orbit.depth, angle)) * sc.scale;
        let busy = store
            .sessions
            .get(&node.session)
            .and_then(|s| s.agent(&node.id))
            .map(|a| a.status(now, theme::ACTIVE_WINDOW_MS) == Status::Active && a.has_open_call())
            .unwrap_or(false);
        if busy {
            target.y += 0.12 * sc.scale * (t * 7.0 + orbit.base_angle).sin().abs();
        }
        tf.translation = tf.translation.lerp(target, (dt * 4.0).min(1.0));
        let s = tf.scale.x + (sc.scale - tf.scale.x) * (dt * 5.0).min(1.0);
        tf.scale = Vec3::splat(s);
        let facing = if orbit.depth == 0 {
            t * 0.2
        } else {
            -angle + std::f32::consts::FRAC_PI_2
        };
        tf.rotation = Quat::from_rotation_y(facing);
        world_pos
            .0
            .insert((node.session.clone(), node.id.clone()), tf.translation);
    }
    for (ring, mut tf) in rings.iter_mut() {
        let Some(sc) = scene.per.get(&ring.session) else {
            continue;
        };
        let target = sc.offset + Vec3::Y * layout::ring_height(ring.depth) * sc.scale;
        tf.translation = tf.translation.lerp(target, (dt * 4.0).min(1.0));
        let s = tf.scale.x + (sc.scale - tf.scale.x) * (dt * 5.0).min(1.0);
        tf.scale = Vec3::splat(s);
    }
}

pub fn update_halos(
    time: Res<Time>,
    store: Res<Store>,
    assets: Res<SceneAssets>,
    parents: Query<&Transform, Without<Halo>>,
    mut halos: Query<(
        &Halo,
        &ChildOf,
        &mut Transform,
        &mut Visibility,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let t = time.elapsed_secs();
    let now = now_millis();
    for (halo, child_of, mut tf, mut vis, mut mat) in halos.iter_mut() {
        let parent_rot = parents
            .get(child_of.parent())
            .map(|p| p.rotation)
            .unwrap_or(Quat::IDENTITY);
        tf.rotation = parent_rot.inverse() * Quat::from_rotation_y(t * halo.speed) * Quat::from_rotation_x(halo.tilt);
        if halo.is_core {
            continue;
        }
        let is_current = store.current.as_deref() == Some(halo.session.as_str());
        let agent = store.sessions.get(&halo.session).and_then(|s| s.agent(&halo.agent_id));
        let key = if is_current && store.selected.as_deref() == Some(halo.agent_id.as_str()) {
            Some(HaloKey::Selected)
        } else if is_current && store.hovered.as_deref() == Some(halo.agent_id.as_str()) {
            Some(HaloKey::Hover)
        } else if agent.is_some_and(|a| is_stalled(&store, a)) {
            Some(HaloKey::Stalled)
        } else if agent
            .map(|a| a.status(now, theme::ACTIVE_WINDOW_MS) == Status::Active && a.has_open_call())
            .unwrap_or(false)
        {
            Some(HaloKey::Active)
        } else {
            None
        };
        match key {
            Some(k) => {
                let handle = &assets.halos[&k];
                if mat.0 != *handle {
                    mat.0 = handle.clone();
                }
                if *vis != Visibility::Inherited {
                    *vis = Visibility::Inherited;
                }
            }
            None => {
                if *vis != Visibility::Hidden {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
}

pub fn animate_dust(time: Res<Time>, settings: Res<Settings>, mut dust: Query<(&Dust, &mut Transform)>) {
    let t = time.elapsed_secs() * settings.drift_speed.max(0.2);
    for (d, mut tf) in dust.iter_mut() {
        let a = d.phase + t * d.speed;
        tf.translation = Vec3::new(
            d.radius * a.cos(),
            d.height + 0.15 * (t * 0.7 + d.phase).sin(),
            d.radius * a.sin(),
        );
    }
}

/// Radio beams between commander and units, and tracer rounds down busy beams.
pub fn draw_links(mut gizmos: Gizmos, time: Res<Time>, store: Res<Store>, world_pos: Res<NodeWorldPos>) {
    let t = time.elapsed_secs();
    let now = now_millis();
    for session in store.sessions.values() {
        for agent in &session.agents {
            let Some(parent) = agent.parent.as_deref() else {
                continue;
            };
            let (Some(&a), Some(&b)) = (
                world_pos.0.get(&(session.id.clone(), parent.to_string())),
                world_pos.0.get(&(session.id.clone(), agent.id.clone())),
            ) else {
                continue;
            };
            let lift = if store.current.as_deref() == Some(session.id.as_str()) {
                1.2
            } else {
                0.6
            };
            let pts = layout::curve(a.into(), b.into(), lift, 32);
            let status = agent.status(now, theme::ACTIVE_WINDOW_MS);
            let color = match status {
                Status::Active => theme::LINK_ACTIVE,
                Status::Idle => theme::LINK,
                Status::Done => theme::LINK_DONE,
            };
            let v: Vec<Vec3> = pts.iter().map(|p| Vec3::from(*p)).collect();
            gizmos.linestrip(v.clone(), color);

            if status == Status::Active && agent.has_open_call() {
                let n = v.len();
                for k in 0..2 {
                    let phase = (t * 0.9 + agent.order as f32 * 0.37 + k as f32 * 0.5).fract();
                    let head = ((phase * (n - 1) as f32) as usize).min(n - 1);
                    let tail = head.saturating_sub(4);
                    if head > tail {
                        gizmos.linestrip(v[tail..=head].to_vec(), theme::PULSE);
                        gizmos.sphere(Isometry3d::from_translation(v[head]), 0.07, theme::PULSE);
                    }
                }
            }
        }
    }
}

pub fn project_badges(
    mut store: ResMut<Store>,
    settings: Res<Settings>,
    cams: Query<(&Camera, &GlobalTransform), With<bevy_panorbit_camera::PanOrbitCamera>>,
    nodes: Query<(&NodeAgent, &GlobalTransform)>,
) {
    let Ok((cam, cam_tf)) = cams.single() else { return };
    let zoom = settings.ui_scale.max(0.1);
    let mut badges = Vec::new();
    let mut satellites = Vec::new();
    let mut unit_screen = std::collections::HashMap::new();
    for (node, tf) in &nodes {
        let Some(session) = store.sessions.get(&node.session) else {
            continue;
        };
        let Some(agent) = session.agent(&node.id) else { continue };
        let is_current = store.current.as_deref() == Some(session.id.as_str());
        let scale = tf.scale().x.max(0.05);
        if let Ok(p) = cam.world_to_viewport(cam_tf, tf.translation()) {
            if is_current {
                unit_screen.insert(agent.id.clone(), (p.x / zoom, p.y / zoom));
            } else if agent.depth == 0 {
                satellites.push(super::state::Satellite {
                    session_id: session.id.clone(),
                    x: p.x / zoom,
                    y: p.y / zoom,
                });
            }
        }
        let pending_approval = store
            .approvals
            .iter()
            .any(|a| a.session_id == session.id && a.agent_id == agent.id);
        let err_key = format!("err:{}:{}", session.id, agent.id);
        let done_key = format!("done:{}:{}", session.id, agent.id);
        let kind = if pending_approval {
            Some(super::state::BadgeKind::Approval)
        } else if agent.last_call_failed() && !store.acked.contains(&err_key) {
            Some(super::state::BadgeKind::Error)
        } else if agent
            .calls
            .iter()
            .any(|c| c.ended.is_none() && store.stalled.contains(&c.id))
        {
            Some(super::state::BadgeKind::Stalled)
        } else if agent.depth == 0 && agent.waiting_for_user {
            Some(super::state::BadgeKind::Waiting)
        } else if agent.depth > 0 && agent.ended.is_some() && !store.acked.contains(&done_key) {
            Some(super::state::BadgeKind::Done)
        } else {
            None
        };
        let Some(kind) = kind else { continue };
        // Satellites only flag what needs the human; finished units there are not news.
        if !is_current && kind == super::state::BadgeKind::Done {
            continue;
        }
        let head = tf.translation() + Vec3::Y * scale * if agent.depth == 0 { 1.05 } else { 0.62 };
        if let Ok(p) = cam.world_to_viewport(cam_tf, head) {
            badges.push(super::state::Badge {
                session_id: session.id.clone(),
                agent_id: agent.id.clone(),
                kind,
                x: p.x / zoom,
                y: p.y / zoom,
            });
        }
    }
    store.badges = badges;
    store.satellites = satellites;
    store.unit_screen = unit_screen;
}

fn on_click(ev: On<PointerClick>, nodes: Query<&NodeAgent>, mut store: ResMut<Store>) {
    if let Ok(n) = nodes.get(ev.entity) {
        debug!("click node {}/{}", n.session, n.id);
        // A click in cinematic mode brings the console back.
        store.hide_ui = false;
        store.acked.insert(format!("done:{}:{}", n.session, n.id));
        store.acked.insert(format!("err:{}:{}", n.session, n.id));
        if store.current.as_deref() == Some(n.session.as_str()) {
            if n.id == ROOT_AGENT && store.selected.as_deref() != Some(n.id.as_str()) {
                store.play(Sfx::Select);
            }
            store.selected = Some(n.id.clone());
        } else {
            // Another constellation: go there first; nothing is selected until you click again.
            store.pending_open = Some(n.session.clone());
            store.pending_select = None;
        }
    }
}

fn on_over(ev: On<PointerOver>, nodes: Query<&NodeAgent>, mut store: ResMut<Store>) {
    if let Ok(n) = nodes.get(ev.entity) {
        if store.current.as_deref() == Some(n.session.as_str()) {
            store.hovered = Some(n.id.clone());
        } else {
            store.hovered_session = Some(n.session.clone());
        }
    }
}

fn on_out(ev: On<PointerOut>, nodes: Query<&NodeAgent>, mut store: ResMut<Store>) {
    if let Ok(n) = nodes.get(ev.entity) {
        if store.hovered.as_deref() == Some(n.id.as_str()) {
            store.hovered = None;
        }
        if store.hovered_session.as_deref() == Some(n.session.as_str()) {
            store.hovered_session = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn satellites_sit_on_a_ring_around_the_current_session() {
        let a = satellite_offset(0, 3);
        let b = satellite_offset(1, 3);
        let r = theme::SATELLITE_RADIUS;
        assert!((a.xz().length() - r).abs() < 1e-3);
        assert!((b.xz().length() - r).abs() < 1e-3);
        assert!(a.distance(b) > r, "neighbours are well apart");
    }
}
