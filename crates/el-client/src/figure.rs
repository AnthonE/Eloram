//! What the champions and the minions share in how they're drawn (`docs/CHAMPIONS.md`, "The
//! motion"):
//!
//! - **A model's clips in a graph twice over**, once for the upper body and once for the hips and
//!   legs (Bevy's masks), so the legs can run while the arms cast, and a flinch can take the upper
//!   body alone. Each bone goes in its group when the model's first figure is in: the graph is the
//!   model's, shared by its figures, and a bone's id is its path's, the same in every one.
//! - **A pose played** on it (`el_client_core::pose`): each layer as its upper body and its legs,
//!   weighted so the run has the legs it's given and a flinch its share of the upper body.
//! - **The motion** (`el_client_core::motion`) drawn on the figure, about its feet.
//! - **Its sockets**: its hands, its feet and the tip of what it holds, where effects come from
//!   (`trails.rs`, the dust a step kicks up); what it holds in hand drawn a little bigger.
//! - **What the figures ask of the effects**: [`Kicks`], drained by the burst pool
//!   (`fx/burst.rs`).

use bevy::animation::AnimationTargetId;
use bevy::asset::RenderAssetUsages;
use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use el_client_core::motion::Drawn as Moved;
use el_client_core::pose::{Clip, Layer, Pose};

/// The mask groups: the upper body's bones, and the hips' and the legs'.
const UPPER: u32 = 0;
const LOWER: u32 = 1;
/// The bones in the lower group (Meshy's rig: `art/tools/retarget.mjs`). The hips are the legs':
/// a cast walked through rides the run's hips.
const LOWER_BONES: [&str; 9] = [
    "Hips",
    "LeftUpLeg",
    "LeftLeg",
    "LeftFoot",
    "LeftToeBase",
    "RightUpLeg",
    "RightLeg",
    "RightFoot",
    "RightToeBase",
];
/// What a figure holds in hand is drawn this much bigger than it was made: Riot's "defining
/// primary characteristic" carries the silhouette, and a figure is some 60 px tall. Not past a
/// reach of [`REACH`] of its height, from the grip: a greatsword is already as tall as Vire.
pub const HELD: f32 = 1.25;
pub const REACH: f32 = 0.85;

/// How much bigger to draw a prop whose tip is `reach` (of the figure's height) from its grip.
pub fn grown(reach: f32) -> f32 {
    (REACH / reach.max(1e-3)).clamp(1.0, HELD)
}

/// A model's clips: each one's node for the upper body and for the legs, in `Clip::ALL`'s order.
pub type Nodes = [Option<[AnimationNodeIndex; 2]>; Clip::ALL.len()];

/// A model's clips in a graph, each twice: the upper body alone, then the legs alone.
pub fn graph(gltf: &Gltf) -> (AnimationGraph, Nodes) {
    let mut graph = AnimationGraph::new();
    let root = graph.root;
    let nodes = Clip::ALL.map(|c| {
        gltf.named_animations.get(c.name()).map(|clip| {
            [
                graph.add_clip_with_mask(clip.clone(), 1 << LOWER, 1.0, root),
                graph.add_clip_with_mask(clip.clone(), 1 << UPPER, 1.0, root),
            ]
        })
    });
    (graph, nodes)
}

/// Puts each of a figure's bones in its group. Returns how many it put.
pub fn mask<'a>(
    graph: &mut AnimationGraph,
    bones: impl IntoIterator<Item = (&'a Name, &'a AnimationTargetId)>,
) -> usize {
    let mut n = 0;
    for (name, id) in bones {
        let group = if LOWER_BONES.contains(&name.as_str()) { LOWER } else { UPPER };
        graph.add_target_to_mask_group(*id, group);
        n += 1;
    }
    n
}

/// Plays `pose` on `player` at the times it says: the clock is the pose's, so each plays paused
/// at its time.
pub fn play(player: &mut AnimationPlayer, nodes: &Nodes, pose: &Pose) {
    const UP: usize = 0;
    const LEGS: usize = 1;
    let node = |c: Clip, part: usize| nodes[c.index()].map(|n| n[part]);
    // Every figure, every frame: in a fixed array, not a new vector.
    let mut want: [Option<(AnimationNodeIndex, f32, f32)>; 8] = [None; 8];
    let mut add = |n: Option<AnimationNodeIndex>, l: &Layer, w: f32| {
        let Some(n) = n else { return };
        if w <= 0.0 {
            return;
        }
        // The same node twice (a swing begun again as the last fades): the weightier.
        if let Some(slot) = want.iter_mut().flatten().find(|x| x.0 == n) {
            if w > slot.2 {
                *slot = (n, l.time, w);
            }
            return;
        }
        if let Some(slot) = want.iter_mut().find(|x| x.is_none()) {
            *slot = Some((n, l.time, w));
        }
    };
    let flinch = pose.flinch.filter(|f| node(f.clip, UP).is_some()).map_or(0.0, |f| f.weight.clamp(0.0, 1.0));
    let legs =
        pose.legs.filter(|_| node(Clip::Run, LEGS).is_some()).map_or(0.0, |l| l.weight.clamp(0.0, 1.0));
    // The upper body: what it does and what it's leaving, under the flinch.
    add(node(pose.main.clip, UP), &pose.main, pose.main.weight * (1.0 - flinch));
    if let Some(f) = &pose.fading {
        add(node(f.clip, UP), f, f.weight * (1.0 - flinch));
    }
    if let Some(f) = &pose.flinch {
        add(node(f.clip, UP), f, flinch);
    }
    // The legs: the run's share, and what the rest does with the others (an arm's fade alone
    // leaves them be).
    let mut lower = pose.main.weight * (1.0 - legs);
    let fading_legs = pose.fading.filter(|_| !pose.fading_upper).map_or(0.0, |f| f.weight * (1.0 - legs));
    if lower + fading_legs + legs < 1e-3 {
        // The frame a cast walked through ends: the legs were running, and are.
        lower = 1.0;
    }
    add(node(pose.main.clip, LEGS), &pose.main, lower);
    if let Some(f) = pose.fading.filter(|_| !pose.fading_upper) {
        add(node(f.clip, LEGS), &f, fading_legs);
    }
    if let Some(l) = &pose.legs {
        add(node(Clip::Run, LEGS), l, legs);
    }
    let wanted = |n: &AnimationNodeIndex| want.iter().flatten().any(|(w, _, _)| w == n);
    // What else plays stops (a few at a time, in the player's order, until none is left).
    loop {
        let mut stale = [None; 4];
        let mut k = 0;
        for (n, _) in player.playing_animations() {
            if k < stale.len() && !wanted(n) {
                stale[k] = Some(*n);
                k += 1;
            }
        }
        for n in stale.into_iter().flatten() {
            player.stop(n);
        }
        if k < stale.len() {
            break;
        }
    }
    for (n, time, w) in want.into_iter().flatten() {
        player.play(n).set_weight(w).seek_to(time).pause();
    }
}

/// The figure's place under its unit: turned, leant and squashed about its feet, `height` tall.
pub fn place(m: &Moved, height: f32) -> Transform {
    Transform {
        rotation: Quat::from_rotation_y(m.yaw)
            * Quat::from_rotation_x(m.pitch)
            * Quat::from_rotation_z(m.roll),
        scale: Vec3::new(height * m.width(), height * m.height, height * m.width()),
        ..default()
    }
}

/// The points of a figure effects come from.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sockets {
    /// Its left and right hands.
    pub hands: [Option<Entity>; 2],
    /// Its left and right feet (the toes, which touch first).
    pub feet: [Option<Entity>; 2],
    /// What it swings or shoots with: where it's gripped and its tip (the right hand's, or the
    /// left's).
    pub weapon: Option<(Entity, Entity)>,
}

impl Sockets {
    /// A figure's sockets from its nodes (each with its name and its parent), once its scene is
    /// in. What it holds on an arm (not on its back or its head) is passed to `grow`, with its
    /// tip, to be drawn bigger; its weapon is the right hand's, else the left's.
    pub fn find<'a>(
        nodes: impl IntoIterator<Item = (Entity, &'a Name, Option<Entity>)>,
        mut grow: impl FnMut(Entity, Entity),
    ) -> Sockets {
        let all: Vec<(Entity, &str, Option<Entity>)> =
            nodes.into_iter().map(|(e, n, p)| (e, n.as_str(), p)).collect();
        let of = |e: Entity| all.iter().find(|x| x.0 == e);
        let mut s = Sockets::default();
        let mut best: Option<(u8, Entity, Entity)> = None;
        for &(e, name, parent) in &all {
            match name {
                "LeftHand" => s.hands[0] = Some(e),
                "RightHand" => s.hands[1] = Some(e),
                "LeftToeBase" => s.feet[0] = Some(e),
                "RightToeBase" => s.feet[1] = Some(e),
                n if n.ends_with(".tip") => {
                    let Some(prop) = parent else { continue };
                    let bone = of(prop).and_then(|p| p.2).and_then(of).map(|b| b.1);
                    let rank = match bone {
                        Some("RightHand") => 0,
                        Some("LeftHand") => 1,
                        Some("LeftForeArm") => 2,
                        _ => continue,
                    };
                    grow(prop, e);
                    if rank < 2 && best.is_none_or(|b| rank < b.0) {
                        best = Some((rank, prop, e));
                    }
                }
                _ => {}
            }
        }
        s.weapon = best.map(|(_, p, t)| (p, t));
        s
    }
}

/// A foot is down when its toe is this low (of the figure's height).
const FOOT_DOWN: f32 = 0.07;

/// A figure's toes, frame to frame: a foot comes down where it was falling and now isn't, low.
#[derive(Clone, Copy, Debug, Default)]
pub struct Feet {
    /// Each toe's height last frame, and whether it was coming down.
    last: [(f32, bool); 2],
}

impl Feet {
    /// The toes where they are now (`None` where the figure has none); where one came down this
    /// frame, while it runs.
    pub fn step(&mut self, toes: [Option<Vec3>; 2], running: bool, height: f32) -> [Option<Vec3>; 2] {
        let mut out = [None; 2];
        for (k, toe) in toes.into_iter().enumerate() {
            let Some(p) = toe else { continue };
            let (was, down) = self.last[k];
            let falling = p.y < was - 1e-4;
            if running && down && !falling && was < FOOT_DOWN * height {
                out[k] = Some(Vec3::new(p.x, 0.02, p.z));
            }
            self.last[k] = (p.y, falling);
        }
        out
    }
}

/// Where the figures are posed and moved each frame (`champions.rs`, `minions.rs`): what draws
/// from their sockets comes after.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FiguresSet;

/// What a figure asks of the effects (`fx/burst.rs` makes each and lets the list go).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kick {
    /// A foot came down running: a little dust where it touched.
    Step { at: Vec3 },
    /// It came down from being thrown up: a ring of dust.
    Land { at: Vec3, size: f32 },
    /// A dash began: a puff left behind it, `back` the way it was left.
    Dash { at: Vec3, back: Vec3 },
}

#[derive(Resource, Default)]
pub struct Kicks {
    pub list: Vec<Kick>,
}

/// A soft round light: white, falling off to nothing at its edge (a hand's light, `trails.rs`; a
/// champion's blob of shadow on the low tier, `champions.rs`).
pub fn disc(images: &mut Assets<Image>) -> Handle<Image> {
    const N: usize = 64;
    let mut data = Vec::with_capacity(N * N * 4);
    for y in 0..N {
        for x in 0..N {
            let d = Vec2::new(x as f32 + 0.5 - N as f32 / 2.0, y as f32 + 0.5 - N as f32 / 2.0).length()
                / (N as f32 / 2.0);
            let a = (1.0 - d).clamp(0.0, 1.0).powf(1.6);
            let v = (a * 255.0) as u8;
            data.extend([v, v, v, v]);
        }
    }
    images.add(Image::new(
        Extent3d { width: N as u32, height: N as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    ))
}
