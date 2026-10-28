//! The constellation: glowing core with halos, orbit rings, agent nodes with status halos,
//! beams with comets, drifting dust, starfield.

use super::settings::Settings;
use super::state::Store;
use super::theme;
use agent_globe::data::layout::{self, Layout};
use agent_globe::data::model::{Status, now_millis};
use bevy::picking::events::{PointerClick, PointerOut, PointerOver};
use bevy::prelude::*;
use std::collections::HashMap;
use std::f32::consts::TAU;

#[derive(Component)]
pub struct NodeAgent {
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
    pub agent_id: String,
    pub tilt: f32,
    pub speed: f32,
    pub is_core: bool,
}

#[derive(Component)]
pub struct OrbitRing {
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

#[derive(Resource, Default)]
pub struct NodeEntities(pub HashMap<String, Entity>);

#[derive(Resource, Default)]
pub struct RingEntities(pub HashMap<u32, Entity>);

#[derive(Resource, Default)]
pub struct CurrentLayout {
    pub layout: Option<Layout>,
    pub session_id: Option<String>,
    pub agent_count: usize,
}

/// Where every node is right now, after orbit animation. Links, pulses and the camera read this.
#[derive(Resource, Default)]
pub struct NodeWorldPos(pub HashMap<String, Vec3>);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum MatKey {
    Root,
    Active,
    Idle,
    Done,
    Error,
    Selected,
    Hover,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum HaloKey {
    Selected,
    Hover,
    Active,
    Core,
}

#[derive(Resource)]
pub struct SceneAssets {
    node_mesh: Handle<Mesh>,
    core_mesh: Handle<Mesh>,
    halo_mesh: Handle<Mesh>,
    core_halo_mesh: Handle<Mesh>,
    materials: HashMap<MatKey, Handle<StandardMaterial>>,
    halos: HashMap<HaloKey, Handle<StandardMaterial>>,
    ring_material: Handle<StandardMaterial>,
}

/// Lit surface with a strong emissive term. Emissive is in nits, so values are large.
fn glow(base: Color, strength: f32) -> StandardMaterial {
    let lin: LinearRgba = base.into();
    StandardMaterial {
        base_color: base,
        emissive: LinearRgba::new(lin.red * strength, lin.green * strength, lin.blue * strength, 1.0),
        perceptual_roughness: 0.95,
        metallic: 0.0,
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

/// Tiny deterministic PRNG so the starfield is identical every launch without a rand dependency.
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

    let node_mesh = meshes.add(Cuboid::new(0.34, 0.34, 0.34));
    let core_mesh = meshes.add(Cuboid::new(0.85, 0.85, 0.85));
    let halo_mesh = meshes.add(Torus {
        minor_radius: 0.022,
        major_radius: 0.42,
    });
    let core_halo_mesh = meshes.add(Torus {
        minor_radius: 0.03,
        major_radius: 1.05,
    });

    let mut mats = HashMap::new();
    mats.insert(MatKey::Root, materials.add(glow(theme::NODE_ROOT, 220.0)));
    mats.insert(MatKey::Active, materials.add(glow(theme::NODE_ACTIVE, 180.0)));
    mats.insert(MatKey::Idle, materials.add(glow(theme::NODE_IDLE, 40.0)));
    mats.insert(MatKey::Done, materials.add(glow(theme::NODE_DONE, 6.0)));
    mats.insert(MatKey::Error, materials.add(glow(theme::NODE_ERROR, 160.0)));
    mats.insert(MatKey::Selected, materials.add(glow(theme::NODE_SELECTED, 200.0)));
    mats.insert(MatKey::Hover, materials.add(glow(theme::NODE_HOVER, 120.0)));

    let mut halos = HashMap::new();
    halos.insert(HaloKey::Selected, materials.add(light(theme::NODE_SELECTED, 160.0)));
    halos.insert(HaloKey::Hover, materials.add(light(theme::NODE_HOVER, 120.0)));
    halos.insert(HaloKey::Active, materials.add(light(theme::NODE_ACTIVE, 140.0)));
    halos.insert(HaloKey::Core, materials.add(light(theme::NODE_ROOT, 120.0)));

    let ring_material = materials.add(StandardMaterial {
        alpha_mode: AlphaMode::Blend,
        ..light(theme::RING_MESH, 30.0)
    });

    // Starfield: a far shell of unlit points. Bloom makes the bigger ones twinkle-bright.
    let star_mesh = meshes.add(Cuboid::new(0.16, 0.16, 0.16));
    let star_mat = materials.add(light(theme::STAR, 60.0));
    let mut rng = XorShift(0x9E37_79B9);
    for _ in 0..theme::STAR_COUNT {
        let u = rng.next_f32() * 2.0 - 1.0;
        let phi = rng.next_f32() * TAU;
        let r = 70.0 + rng.next_f32() * 60.0;
        let s = (1.0 - u * u).sqrt();
        let pos = Vec3::new(r * s * phi.cos(), r * u, r * s * phi.sin());
        let size = 0.6 + rng.next_f32() * 1.8;
        commands.spawn((
            Mesh3d(star_mesh.clone()),
            MeshMaterial3d(star_mat.clone()),
            Transform::from_translation(pos).with_scale(Vec3::splat(size)),
            Star,
            Pickable::IGNORE,
        ));
    }

    // Dust: slow motes drifting around the rings so the space never looks frozen.
    let dust_mesh = meshes.add(Cuboid::new(0.055, 0.055, 0.055));
    let dust_mat = materials.add(light(theme::DUST, 25.0));
    for _ in 0..theme::DUST_COUNT {
        let radius = 1.5 + rng.next_f32() * 13.0;
        let height = (rng.next_f32() - 0.5) * 2.4;
        let phase = rng.next_f32() * TAU;
        let speed = 0.015 + rng.next_f32() * 0.05;
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
        node_mesh,
        core_mesh,
        halo_mesh,
        core_halo_mesh,
        materials: mats,
        halos,
        ring_material,
    });
}

fn material_for(store: &Store, agent: &agent_globe::data::model::Agent, now: i64) -> MatKey {
    if store.selected.as_deref() == Some(agent.id.as_str()) {
        return MatKey::Selected;
    }
    if store.hovered.as_deref() == Some(agent.id.as_str()) {
        return MatKey::Hover;
    }
    if agent.calls.iter().any(|c| c.is_error) && agent.ended.is_some() {
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
    assets: Res<SceneAssets>,
    mut entities: ResMut<NodeEntities>,
    mut rings: ResMut<RingEntities>,
    mut current: ResMut<CurrentLayout>,
    mut nodes: Query<(&NodeAgent, &mut Orbit, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(session) = store.current_session() else {
        for (_, e) in entities.0.drain() {
            commands.entity(e).despawn();
        }
        for (_, e) in rings.0.drain() {
            commands.entity(e).despawn();
        }
        current.layout = None;
        current.session_id = None;
        return;
    };

    let switched = current.session_id.as_deref() != Some(session.id.as_str());
    if switched {
        for (_, e) in entities.0.drain() {
            commands.entity(e).despawn();
        }
        for (_, e) in rings.0.drain() {
            commands.entity(e).despawn();
        }
        current.session_id = Some(session.id.clone());
    }
    let relayout = store.layout_dirty || switched || current.agent_count != session.agents.len();
    if relayout {
        current.layout = Some(layout::layout(session));
        current.agent_count = session.agents.len();
    }
    let Some(lay) = current.layout.as_ref() else { return };

    // One torus per orbit depth.
    for depth in 1..=lay.max_depth {
        if rings.0.contains_key(&depth) {
            continue;
        }
        let mesh = commands.spawn_empty().id();
        commands.entity(mesh).despawn();
        let e = commands
            .spawn((
                OrbitRing { depth },
                Transform::from_translation(Vec3::new(0.0, layout::ring_height(depth), 0.0)),
                Visibility::default(),
                Pickable::IGNORE,
            ))
            .id();
        rings.0.insert(depth, e);
    }

    for n in &lay.nodes {
        if entities.0.contains_key(&n.agent_id) {
            continue;
        }
        let is_core = n.depth == 0;
        let mesh = if is_core {
            assets.core_mesh.clone()
        } else {
            assets.node_mesh.clone()
        };
        // New nodes are born at the core and fly out to their orbit.
        let mut ec = commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(assets.materials[&MatKey::Idle].clone()),
            Transform::from_translation(Vec3::ZERO).with_scale(Vec3::splat(0.01)),
            NodeAgent { id: n.agent_id.clone() },
            Orbit {
                depth: n.depth,
                base_angle: n.angle,
            },
        ));
        ec.observe(on_click).observe(on_over).observe(on_out);
        let id = n.agent_id.clone();
        ec.with_children(|p| {
            if is_core {
                for (tilt, speed) in [(0.55_f32, 0.45_f32), (-0.55, -0.35)] {
                    p.spawn((
                        Mesh3d(assets.core_halo_mesh.clone()),
                        MeshMaterial3d(assets.halos[&HaloKey::Core].clone()),
                        Transform::from_rotation(Quat::from_rotation_x(tilt)),
                        Halo {
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
                        agent_id: id.clone(),
                        tilt: 1.2,
                        speed: 1.6,
                        is_core: false,
                    },
                    Pickable::IGNORE,
                ));
            }
        });
        entities.0.insert(n.agent_id.clone(), ec.id());
    }

    let now = now_millis();
    for (node, mut orbit, mut mat) in nodes.iter_mut() {
        if relayout && let Some(n) = lay.node(&node.id) {
            orbit.depth = n.depth;
            orbit.base_angle = n.angle;
        }
        if let Some(agent) = session.agent(&node.id) {
            let handle = &assets.materials[&material_for(&store, agent, now)];
            if mat.0 != *handle {
                mat.0 = handle.clone();
            }
        }
    }
    store.layout_dirty = false;
}

/// Give freshly spawned ring entities their torus mesh (radius depends on depth).
pub fn dress_rings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    assets: Res<SceneAssets>,
    fresh: Query<(Entity, &OrbitRing), Without<Mesh3d>>,
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
}

/// Slow orbital drift per ring, so the constellation is never static.
fn orbit_angle(base: f32, depth: u32, t: f32, speed: f32) -> f32 {
    if depth == 0 {
        return 0.0;
    }
    base + t * 0.06 * speed / depth as f32
}

/// Move nodes toward their orbit position, pulse the busy ones, publish world positions.
pub fn animate_nodes(
    time: Res<Time>,
    store: Res<Store>,
    settings: Res<Settings>,
    mut world_pos: ResMut<NodeWorldPos>,
    mut nodes: Query<(&NodeAgent, &Orbit, &mut Transform)>,
) {
    let Some(session) = store.current_session() else { return };
    let now = now_millis();
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    world_pos.0.clear();
    for (node, orbit, mut tf) in nodes.iter_mut() {
        let angle = orbit_angle(orbit.base_angle, orbit.depth, t, settings.drift_speed);
        let target = Vec3::from(layout::orbit_point(orbit.depth, angle));
        tf.translation = tf.translation.lerp(target, (dt * 4.0).min(1.0));
        let busy = session
            .agent(&node.id)
            .map(|a| a.status(now, theme::ACTIVE_WINDOW_MS) == Status::Active && a.has_open_call())
            .unwrap_or(false);
        let base = if busy { 1.0 + 0.3 * (t * 6.0).sin().abs() } else { 1.0 };
        let s = tf.scale.x + (base - tf.scale.x) * (dt * 5.0).min(1.0);
        tf.scale = Vec3::splat(s);
        let spin = if orbit.depth == 0 { 0.25 } else { 0.6 };
        tf.rotation = Quat::from_rotation_y(t * spin + orbit.base_angle) * Quat::from_rotation_x(0.45);
        world_pos.0.insert(node.id.clone(), tf.translation);
    }
}

/// Halos spin; node halos show selection, hover, or activity; core halos are always on.
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
    let session = store.current_session();
    for (halo, child_of, mut tf, mut vis, mut mat) in halos.iter_mut() {
        let parent_rot = parents
            .get(child_of.parent())
            .map(|p| p.rotation)
            .unwrap_or(Quat::IDENTITY);
        tf.rotation = parent_rot.inverse() * Quat::from_rotation_y(t * halo.speed) * Quat::from_rotation_x(halo.tilt);
        if halo.is_core {
            continue;
        }
        let key = if store.selected.as_deref() == Some(halo.agent_id.as_str()) {
            Some(HaloKey::Selected)
        } else if store.hovered.as_deref() == Some(halo.agent_id.as_str()) {
            Some(HaloKey::Hover)
        } else if session
            .and_then(|s| s.agent(&halo.agent_id))
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

/// Beams between parent and child, and comets travelling down busy beams.
pub fn draw_links(mut gizmos: Gizmos, time: Res<Time>, store: Res<Store>, world_pos: Res<NodeWorldPos>) {
    let t = time.elapsed_secs();
    let Some(session) = store.current_session() else { return };
    let now = now_millis();
    for agent in &session.agents {
        let Some(parent) = agent.parent.as_deref() else {
            continue;
        };
        let (Some(&a), Some(&b)) = (world_pos.0.get(parent), world_pos.0.get(&agent.id)) else {
            continue;
        };
        let pts = layout::curve(a.into(), b.into(), 1.2, 32);
        let status = agent.status(now, theme::ACTIVE_WINDOW_MS);
        let color = match status {
            Status::Active => theme::LINK_ACTIVE,
            Status::Idle => theme::LINK,
            Status::Done => theme::LINK_DONE,
        };
        let v: Vec<Vec3> = pts.iter().map(|p| Vec3::from(*p)).collect();
        gizmos.linestrip(v.clone(), color);

        if status == Status::Active && agent.has_open_call() {
            let phase = (t * 0.7 + agent.order as f32 * 0.37).fract();
            let n = v.len();
            let head = ((phase * (n - 1) as f32) as usize).min(n - 1);
            let tail = head.saturating_sub(6);
            if head > tail {
                gizmos.linestrip(v[tail..=head].to_vec(), theme::PULSE);
                gizmos.sphere(Isometry3d::from_translation(v[head]), 0.1, theme::PULSE);
            }
        }
    }
}

fn on_click(ev: On<PointerClick>, nodes: Query<&NodeAgent>, mut store: ResMut<Store>) {
    if let Ok(n) = nodes.get(ev.entity) {
        debug!("click node {}", n.id);
        store.selected = Some(n.id.clone());
    }
}

fn on_over(ev: On<PointerOver>, nodes: Query<&NodeAgent>, mut store: ResMut<Store>) {
    if let Ok(n) = nodes.get(ev.entity) {
        store.hovered = Some(n.id.clone());
    }
}

fn on_out(ev: On<PointerOut>, nodes: Query<&NodeAgent>, mut store: ResMut<Store>) {
    if let Ok(n) = nodes.get(ev.entity)
        && store.hovered.as_deref() == Some(n.id.as_str())
    {
        store.hovered = None;
    }
}
