//! Orbital layout: a pure function from a session to node positions in space.
//!
//! The root agent is the core at the origin. Each depth is an orbit ring around it; children are
//! placed near their parent's angle so a parent's cluster reads as one constellation. Links are
//! quadratic curves lifted out of the ring plane.

use super::model::Session;
use std::collections::HashMap;
use std::f32::consts::TAU;

pub type Vec3 = [f32; 3];

/// Distance between orbit rings.
pub const RING_STEP: f32 = 3.6;
/// Vertical rise per depth so rings read as layers, not one flat disc.
pub const RING_RISE: f32 = 0.55;

#[derive(Debug, Clone, PartialEq)]
pub struct NodePos {
    pub agent_id: String,
    pub pos: Vec3,
    pub depth: u32,
    /// Orbit angle in radians (0 for the root).
    pub angle: f32,
    /// Orbit radius (0 for the root).
    pub ring: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub nodes: Vec<NodePos>,
    pub max_depth: u32,
}

impl Layout {
    pub fn position(&self, agent_id: &str) -> Option<Vec3> {
        self.nodes.iter().find(|n| n.agent_id == agent_id).map(|n| n.pos)
    }

    pub fn node(&self, agent_id: &str) -> Option<&NodePos> {
        self.nodes.iter().find(|n| n.agent_id == agent_id)
    }
}

pub fn ring_radius(depth: u32) -> f32 {
    RING_STEP * depth as f32
}

pub fn ring_height(depth: u32) -> f32 {
    RING_RISE * depth as f32
}

pub fn orbit_point(depth: u32, angle: f32) -> Vec3 {
    let r = ring_radius(depth);
    [r * angle.cos(), ring_height(depth), r * angle.sin()]
}

pub fn layout(session: &Session) -> Layout {
    let mut nodes = Vec::with_capacity(session.agents.len());
    let mut angle_of: HashMap<String, f32> = HashMap::new();
    let max_depth = session.agents.iter().map(|a| a.depth).max().unwrap_or(0);

    for a in session.agents.iter().filter(|a| a.depth == 0) {
        nodes.push(NodePos {
            agent_id: a.id.clone(),
            pos: [0.0, 0.0, 0.0],
            depth: 0,
            angle: 0.0,
            ring: 0.0,
        });
        angle_of.insert(a.id.clone(), 0.0);
    }

    for depth in 1..=max_depth {
        let mut ring: Vec<&super::model::Agent> = session.agents.iter().filter(|a| a.depth == depth).collect();
        if ring.is_empty() {
            continue;
        }
        // Order by parent's angle, then spawn order, so siblings sit together under their parent.
        ring.sort_by(|a, b| {
            let pa = a.parent.as_ref().and_then(|p| angle_of.get(p)).copied().unwrap_or(0.0);
            let pb = b.parent.as_ref().and_then(|p| angle_of.get(p)).copied().unwrap_or(0.0);
            pa.partial_cmp(&pb).unwrap().then(a.order.cmp(&b.order))
        });
        let n = ring.len() as f32;
        let step = TAU / n;
        // Rotate the ring so the first group is centred on its parent's angle.
        let first_parent = ring[0]
            .parent
            .as_ref()
            .and_then(|p| angle_of.get(p))
            .copied()
            .unwrap_or(0.0);
        let first_group = ring.iter().take_while(|a| a.parent == ring[0].parent).count() as f32;
        let start = first_parent - step * (first_group - 1.0) / 2.0;
        for (i, a) in ring.iter().enumerate() {
            let angle = start + step * i as f32;
            angle_of.insert(a.id.clone(), angle);
            nodes.push(NodePos {
                agent_id: a.id.clone(),
                pos: orbit_point(depth, angle),
                depth,
                angle,
                ring: ring_radius(depth),
            });
        }
    }
    Layout { nodes, max_depth }
}

/// Points along a quadratic curve from `a` to `b`, lifted by `lift` at the middle.
pub fn curve(a: Vec3, b: Vec3, lift: f32, segments: usize) -> Vec<Vec3> {
    let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5 + lift, (a[2] + b[2]) * 0.5];
    (0..=segments)
        .map(|s| {
            let t = s as f32 / segments as f32;
            let u = 1.0 - t;
            [
                u * u * a[0] + 2.0 * u * t * mid[0] + t * t * b[0],
                u * u * a[1] + 2.0 * u * t * mid[1] + t * t * b[1],
                u * u * a[2] + 2.0 * u * t * mid[2] + t * t * b[2],
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::model::{Event, ROOT_AGENT};

    fn spawn(s: &mut Session, id: &str, parent: &str) {
        s.apply(Event::AgentSpawn {
            session_id: "s".into(),
            agent_id: id.into(),
            parent_id: Some(parent.into()),
            spawn_tool_use_id: None,
            name: String::new(),
            kind: String::new(),
            model: String::new(),
            prompt: String::new(),
            background: false,
            at: 2,
        });
    }

    #[test]
    fn root_at_core_children_on_rings_by_depth() {
        let mut s = Session::new("s", "p", 1);
        for i in 0..4 {
            spawn(&mut s, &format!("a{i}"), ROOT_AGENT);
        }
        spawn(&mut s, "b0", "a1");
        let l = layout(&s);
        assert_eq!(l.position(ROOT_AGENT).unwrap(), [0.0, 0.0, 0.0]);
        assert_eq!(l.max_depth, 2);
        let ring1: Vec<_> = l.nodes.iter().filter(|n| n.depth == 1).collect();
        assert_eq!(ring1.len(), 4);
        for n in &ring1 {
            let r = (n.pos[0].powi(2) + n.pos[2].powi(2)).sqrt();
            assert!((r - RING_STEP).abs() < 1e-3);
            assert!((n.pos[1] - RING_RISE).abs() < 1e-3);
        }
        let parent = l.node("a1").unwrap();
        let child = l.node("b0").unwrap();
        assert_eq!(child.depth, 2);
        let d = (child.angle - parent.angle).rem_euclid(TAU);
        assert!(
            d < 1e-3 || (TAU - d) < 1e-3,
            "child angle {} vs parent {}",
            child.angle,
            parent.angle
        );
    }

    #[test]
    fn curve_hits_both_endpoints_and_lifts_middle() {
        let pts = curve([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], 1.0, 8);
        assert_eq!(pts.len(), 9);
        assert_eq!(pts[0], [0.0, 0.0, 0.0]);
        assert_eq!(pts[8], [4.0, 0.0, 0.0]);
        assert!((pts[4][1] - 0.5).abs() < 1e-5);
    }
}
