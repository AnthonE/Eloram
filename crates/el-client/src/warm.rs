//! What a match draws, drawn once before it, out of sight, so nothing is built in the middle of
//! one (`frame.rs` counts what is; `scripts/e2e.sh hitches` holds it at 0).
//!
//! Bevy builds a render pipeline the first frame something visible needs it, and on wasm in that
//! frame. Before this, a match built the effects' pipeline on its first cast and the champions'
//! and the portraits' as it came into view, and the minions' on the first wave: about a pipeline
//! a hitch. A pipeline is a material's shader for a mesh's layout in a view (its target, its
//! multisampling, its shadows), so the warm-up draws exactly those: the bursts' two materials and
//! a live quad's in the main view, each champion's and minion's model with the materials the match
//! gives it, in the main view (shadows and all, on the tier that has them) and a champion's in the
//! portrait's. In the lobby, the queue and champion select; the pieces go once the match is in
//! view. Each piece is far too small to cover a pixel, or a mesh with nothing in it.
//!
//! And what's in the page from its start but not in view before a match shows it: the units'
//! bodies, rings and bars and the relics (`view.rs`'s pools, hidden), and the map's far places
//! (out of the camera's frustum: a brazier's flame down the lane was built as the camera first
//! came to it). Each such thing's material on its mesh's layout, once, as a speck of its own
//! ([`unseen`]). And Bevy's gizmos (the move marker, the hover ring, an ability's indicator), whose
//! pipeline was built on the first one drawn ([`lines`]).
//!
//! What can't be drawn before the match: the other team's models, which come with it (champion
//! select shows a team only its own). Those are warmed as they come in, before they're seen.

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};

use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::gltf::GltfMaterialName;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use el_client_core::pose::MINIONS;
use el_sim::content::champions::CHAMPIONS;

use crate::camera::MainCamera;
use crate::champions::{self, RimMat};
use crate::fx::burst::{self, BurstMat, Bursts};
use crate::fx::live::{LiveMat, LiveParams};
use crate::ground::{BrushMat, FogTex, FoggedMat, GroundMat};
use crate::minions;
use crate::portrait::{self, PortraitSubject};
use crate::scenery::{FlameMat, SnowMat};
use crate::session::{Phase, Session};

/// How far in front of the camera the pieces stand (m), and how small (a champion's model is
/// about 2 m: this is a hundredth of a pixel at any zoom).
const NEAR: f32 = 4.0;
const TINY: f32 = 1e-4;
/// Where the portrait's piece stands: under the map, where no other camera looks.
const PORTRAIT_AT: Vec3 = Vec3::new(0.0, -400.0, 0.0);
/// Frames a piece warmed in a match stays (its scene in, its materials on, drawn, built).
const IN_MATCH_FRAMES: u32 = 8;
/// Frames between looks for hidden things not yet warmed.
const LOOK_EVERY: u32 = 20;

/// A piece of the warm-up.
#[derive(Component)]
struct Piece {
    /// Where it stands: in front of the camera, or under the map for the portrait's camera.
    near: bool,
    /// Made with a match in view, and the frames it has stood: one made before the match goes as
    /// the match comes into view, one made in it after [`IN_MATCH_FRAMES`].
    in_match: bool,
    frames: u32,
}

/// A model's piece, waiting for its scene to be in to put its materials on.
#[derive(Component, Clone, Copy)]
enum Model {
    Champion { portrait: bool },
    Minion(usize),
}

#[derive(Resource, Default)]
pub struct Warm {
    /// The pools' pieces are up.
    pools: bool,
    /// Each model warmed (its pieces spawned).
    champions: [bool; CHAMPIONS.len()],
    minions: [bool; MINIONS.len()],
    /// Pieces spawned in all, and those up now (`window.__el`).
    spawned: u32,
}

pub struct WarmPlugin;

impl Plugin for WarmPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().resource::<crate::net::LaunchConfigRes>().0.warm {
            return;
        }
        app.init_resource::<Warm>().add_systems(
            Update,
            (
                warm,
                unseen::<StandardMaterial>,
                unseen::<FoggedMat>,
                unseen::<GroundMat>,
                unseen::<BrushMat>,
                unseen::<SnowMat>,
                unseen::<FlameMat>,
                lines,
                place,
            )
                .chain()
                .after(crate::view::ViewSet),
        );
    }
}

/// A match in view: played (its first snapshot in) or watched. `frame.rs` counts what's built
/// from then on.
pub fn in_match(session: &Session) -> bool {
    match session.phase {
        Phase::Playing => session.client.as_deref().is_some_and(|c| c.started()),
        Phase::Watching => true,
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn warm(
    mut commands: Commands,
    session: Res<Session>,
    mut warm: ResMut<Warm>,
    champs: Res<champions::Models>,
    mins: Res<minions::Models>,
    bursts: Option<Res<Bursts>>,
    fog: Res<FogTex>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut lives: ResMut<Assets<LiveMat>>,
    mut pieces: Query<(Entity, &mut Piece)>,
    mut status: ResMut<crate::dev_hooks::DevStatus>,
) {
    // The fx stage draws one effect alone, and has no match to come.
    if session.phase == Phase::Stage {
        return;
    }
    let playing = in_match(&session);
    let mut up = 0;
    for (e, mut p) in &mut pieces {
        p.frames += 1;
        if (playing && !p.in_match) || (p.in_match && p.frames > IN_MATCH_FRAMES) {
            commands.entity(e).despawn();
        } else {
            up += 1;
        }
    }
    status.set("warm_pieces", up);
    status.set("warm_spawned", warm.spawned);
    // The pools from the page's first frames (a practice match or a record watched comes into view
    // at once: there, as it does).
    if !warm.pools
        && let Some(b) = bursts
    {
        // The bursts: each pool's material on a mesh with nothing in it (`burst::empty_mesh`).
        let mesh = meshes.add(burst::empty_mesh());
        for mat in &b.mats {
            let piece = (Mesh3d(mesh.clone()), MeshMaterial3d::<BurstMat>(mat.clone()));
            spawn_piece(&mut commands, &mut warm, playing, piece);
        }
        // A live quad (missiles' heads and trails, telegraphs, fields, marks: one material).
        let quad = meshes.add(Rectangle::new(1.0, 1.0));
        let mat = lives.add(LiveMat { live: LiveParams::default(), fog: fog.0.clone() });
        spawn_piece(&mut commands, &mut warm, playing, (Mesh3d(quad), MeshMaterial3d(mat)));
        warm.pools = true;
    }
    // Each model as it comes in: before the match, or in it (the other team's) before it's seen.
    for (d, art) in champs.ready.iter().enumerate() {
        let Some(art) = art else { continue };
        if warm.champions[d] {
            continue;
        }
        warm.champions[d] = true;
        spawn_model(
            &mut commands,
            &mut warm,
            playing,
            art.scene.clone(),
            Model::Champion { portrait: false },
        );
        // The portrait's camera draws the player's own and the allies, whose models are in before
        // the match. In one, a piece there would be framed instead of the player's own.
        if !playing {
            spawn_model(
                &mut commands,
                &mut warm,
                playing,
                art.scene.clone(),
                Model::Champion { portrait: true },
            );
        }
    }
    for (m, art) in mins.ready.iter().enumerate() {
        let Some(art) = art else { continue };
        if warm.minions[m] {
            continue;
        }
        warm.minions[m] = true;
        spawn_model(&mut commands, &mut warm, playing, art.scene.clone(), Model::Minion(m));
    }
}

/// Each thing with material `M` not in view (hidden, or out of the camera's frustum), its material
/// on its mesh's layout once, as a speck: what a match shows and nothing before it did. Shadows
/// as the thing has them, and its render layers.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn unseen<M: Material>(
    mut commands: Commands,
    session: Res<Session>,
    mut warm: ResMut<Warm>,
    things: Query<
        (&Mesh3d, &MeshMaterial3d<M>, &ViewVisibility, Has<NotShadowCaster>, Option<&RenderLayers>),
        Without<Piece>,
    >,
    meshes: Res<Assets<Mesh>>,
    mut seen: Local<HashSet<(AssetId<M>, u64)>>,
    mut every: Local<u32>,
) {
    *every += 1;
    if *every % LOOK_EVERY != 1 || session.phase == Phase::Stage || in_match(&session) {
        return;
    }
    for (mesh, mat, vis, no_shadow, layers) in &things {
        if vis.get() {
            // In view last frame: drawn, so built.
            continue;
        }
        let Some(m) = meshes.get(&mesh.0) else { continue };
        let Ok(attributes) = m.try_attributes() else { continue };
        let mut h = DefaultHasher::new();
        m.primitive_topology().hash(&mut h);
        for (a, _) in attributes {
            a.id.hash(&mut h);
        }
        no_shadow.hash(&mut h);
        layers.map(|l| l.iter().collect::<Vec<_>>()).hash(&mut h);
        if !seen.insert((mat.0.id(), h.finish())) {
            continue;
        }
        let mut e = commands.spawn((
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(mat.0.clone()),
            Transform::default(),
            Visibility::Visible,
            NoFrustumCulling,
            Piece { near: true, in_match: false, frames: 0 },
        ));
        if no_shadow {
            e.insert(NotShadowCaster);
        }
        if let Some(l) = layers {
            e.insert(l.clone());
        }
        warm.spawned += 1;
    }
}

fn spawn_piece(commands: &mut Commands, warm: &mut Warm, in_match: bool, bundle: impl Bundle) {
    commands.spawn((
        bundle,
        Transform::default(),
        Visibility::Visible,
        NoFrustumCulling,
        NotShadowCaster,
        Piece { near: true, in_match, frames: 0 },
    ));
    warm.spawned += 1;
}

fn spawn_model(
    commands: &mut Commands,
    warm: &mut Warm,
    in_match: bool,
    scene: Handle<WorldAsset>,
    model: Model,
) {
    let portrait = matches!(model, Model::Champion { portrait: true });
    let mut e = commands.spawn((
        WorldAssetRoot(scene),
        Transform::from_translation(PORTRAIT_AT),
        Visibility::Visible,
        Piece { near: !portrait, in_match, frames: 0 },
        model,
    ));
    e.observe(dress);
    if portrait {
        // The portrait's camera frames it, and draws it (`portrait.rs`).
        e.insert(PortraitSubject { view: 0, still: false });
    }
    warm.spawned += 1;
}

/// A model's scene is in: the materials the match gives it, and the portrait's layer for its
/// portrait piece (that camera alone sees it).
#[allow(clippy::too_many_arguments)]
fn dress(
    event: On<WorldInstanceReady>,
    mut commands: Commands,
    models: Query<&Model>,
    children: Query<&Children>,
    shapes: Query<(), With<Mesh3d>>,
    standard: Query<&MeshMaterial3d<StandardMaterial>>,
    names: Query<&GltfMaterialName>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rims: ResMut<Assets<RimMat>>,
    mins: Res<minions::Models>,
) {
    let e = event.event_target();
    let Ok(&model) = models.get(e) else { return };
    for c in children.iter_descendants(e) {
        if let Model::Champion { portrait: true } = model
            && shapes.contains(c)
        {
            commands.entity(c).insert(RenderLayers::layer(portrait::layer(0)));
        }
        let Ok(m) = standard.get(c) else { continue };
        let Some(own) = materials.get(&m.0) else { continue };
        match model {
            Model::Champion { .. } => {
                let h = rims.add(champions::rim_material(own, 0));
                commands.entity(c).remove::<MeshMaterial3d<StandardMaterial>>().insert(MeshMaterial3d(h));
            }
            Model::Minion(k) => {
                let band = names.get(c).is_ok_and(|n| &**n == "team");
                let team = minions::team_mat(own, 0, band, mins.dusk[k].as_ref());
                let h = materials.add(team);
                commands.entity(c).insert(MeshMaterial3d(h));
            }
        }
    }
}

/// Bevy's gizmos (`view.rs`: the marker a click on the ground leaves, the hover ring, an ability's
/// indicator) have pipelines of their own, a line's and a strip's: a speck of each, unseen.
fn lines(mut gizmos: Gizmos, session: Res<Session>, cam: Single<&GlobalTransform, With<MainCamera>>) {
    if session.phase == Phase::Stage || in_match(&session) {
        return;
    }
    let at = cam.translation() + cam.forward() * NEAR;
    gizmos.line(at, at + Vec3::splat(TINY), Color::NONE);
    gizmos.circle(Isometry3d::from_translation(at), TINY, Color::NONE);
}

/// The near pieces in front of the camera, a speck.
fn place(cam: Single<&GlobalTransform, With<MainCamera>>, mut pieces: Query<(&Piece, &mut Transform)>) {
    let at = cam.translation() + cam.forward() * NEAR;
    for (p, mut tf) in &mut pieces {
        if p.near {
            let want = Transform::from_translation(at).with_scale(Vec3::splat(TINY));
            if *tf != want {
                *tf = want;
            }
        }
    }
}
