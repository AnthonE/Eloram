//! The frame's CPU time by part, and what the GPU was asked to build once a match had begun
//! (`docs/ROADMAP.md`, "The frame, measured where it can be").
//!
//! Riot's posts on League's frame rate are the brief. Time each part of the frame, not just the
//! whole: the client, the HUD, the scene, particles, fog, the wait for the GPU, each with its own
//! timer. Count the spikes, frame by frame, since an average hides them ("The Case of the Missing
//! Milliseconds", 2018). And "hitches and frame drops" are "more impactful than low average FPS":
//! they land on ability casts, the worst from what wasn't ready before the game began ("An Update
//! on In-Game Performance", 2018).
//!
//! **The parts.** On wasm everything runs on the page's one thread, the main world's schedules and
//! then the render world's (no pipelined rendering), so a mark at each boundary splits the frame's
//! CPU time exactly: `pre` (First, PreUpdate, the fixed loop), `update` (the game: the link, the
//! prediction, the view, effects, the HUD's systems), `post` (scenes, animation, transforms,
//! visibility, the UI's layout), `last`, `extract`, `prepare` (specialise, queue, sort, buffers and
//! bind groups), `pipelines` (building the pipelines queued this frame), `draw` (the render graph
//! encoded, submitted and presented) and `cleanup`. The rest of the interval between two frames is
//! the page waiting: for the display's refresh, or for the GPU. That split is real in headless
//! Chromium too, where only the rasterising is in software (`CLAUDE.md`): the parts are the wasm's
//! own work, the waiting isn't.
//!
//! **What's built late.** Bevy builds a render pipeline the first frame something visible needs it,
//! and on wasm it builds it there and then (`create_pipeline_task` blocks): the shader composed,
//! translated and handed to the browser inside that frame. A pipeline first needed mid-fight is a
//! hitch on the cast that needed it. So every pipeline built while a match is played or watched is
//! counted and named (`pipelines_late`, `pipelines_late_what`), with the frame time it took, and so
//! is every texture and mesh added to the page then (`images_late`, `meshes_late`). No pipeline
//! should be (`warm.rs` builds them before the match; `scripts/e2e.sh hitches` holds it); the
//! other team's models still come with the match, their textures and meshes with them.

use std::cell::{Cell, RefCell};

use bevy::app::MainScheduleOrder;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bevy::render::render_resource::{CachedPipelineState, PipelineCache, PipelineDescriptor};
use bevy::render::renderer::render_system;
use bevy::render::{Render, RenderApp, RenderSystems};

use crate::dev_hooks::DevStatus;
use crate::net::now_ms;
use crate::session::Session;

/// The parts of a frame, in the order they run.
pub const PARTS: [&str; 9] =
    ["pre", "update", "post", "last", "extract", "prepare", "pipelines", "draw", "cleanup"];
const N: usize = PARTS.len();
/// The parts' boundaries: one mark more than there are parts.
const MARKS: usize = N + 1;
/// The page's frames kept for the median and the 99th percentile (about ten seconds at 60).
const RING: usize = 600;
/// A spike: a frame whose CPU work alone missed 60 Hz.
pub const SPIKE_MS: f32 = 1000.0 / 60.0;
/// A hitch: a frame whose CPU work alone took three frames at 60.
pub const HITCH_MS: f32 = 50.0;
/// How many of the late pipelines are named.
const NAMED: usize = 24;

thread_local! {
    /// This frame's marks (ms on the page's clock), set by both worlds' mark systems.
    static AT: Cell<[f64; MARKS]> = const { Cell::new([0.0; MARKS]) };
    /// Pipelines that became ready this frame, and what each was.
    static BUILT: RefCell<(u32, Vec<String>)> = const { RefCell::new((0, Vec::new())) };
}

fn mark(i: usize) {
    let t = now_ms();
    AT.with(|at| {
        let mut a = at.get();
        a[i] = t;
        at.set(a);
    });
}

/// The main world's marks, as schedules of their own between Bevy's.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct MarkFrame;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct MarkUpdate;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct MarkPost;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct MarkLast;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct MarkEnd;

/// Between the render world's Prepare and Render: the pipelines queued this frame are built here,
/// on their own clock, rather than at the start of Render (where Bevy would build them, and where
/// what's left for it is then nothing).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct BuildSet;

/// One frame's parts (ms).
pub type Parts = [f32; N];

/// The frames, summed up.
#[derive(Resource)]
pub struct Frames {
    /// The last [`RING`] frames' parts, and how many frames have been folded in.
    ring: Box<[Parts; RING]>,
    n: u64,
    /// The last frame folded in: its parts, its CPU work (their sum) and the interval since the
    /// one before it (ms).
    pub last: Parts,
    pub last_work: f32,
    pub last_interval: f32,
    /// Where the frame before began, and whether it was in a match (played or watched).
    began: f64,
    in_match: bool,
    /// In a match: frames, spikes and hitches, and which part was the largest in each spike.
    pub match_frames: u64,
    pub spikes: u32,
    pub hitches: u32,
    pub spike_by: [u32; N],
    pub worst_ms: f32,
    /// Pipelines built in all, and those built in a match, with the CPU time of the frames that
    /// built them and what they were.
    pub pipelines: u32,
    pub late: u32,
    pub late_ms: f32,
    pub late_what: Vec<String>,
    /// Textures and meshes added to the page in a match.
    pub images_late: u32,
    pub meshes_late: u32,
    /// The median and the 99th percentile of the ring's CPU work, and each part's mean over it,
    /// worked out now and then (`sum_up`).
    pub p50: f32,
    pub p99: f32,
    pub mean: Parts,
    scratch: Box<[f32; RING]>,
    summed_at: u64,
    summed_ms: f64,
}

impl Default for Frames {
    fn default() -> Self {
        Frames {
            ring: Box::new([[0.0; N]; RING]),
            n: 0,
            last: [0.0; N],
            last_work: 0.0,
            last_interval: 0.0,
            began: 0.0,
            in_match: false,
            match_frames: 0,
            spikes: 0,
            hitches: 0,
            spike_by: [0; N],
            worst_ms: 0.0,
            pipelines: 0,
            late: 0,
            late_ms: 0.0,
            late_what: Vec::new(),
            images_late: 0,
            meshes_late: 0,
            p50: 0.0,
            p99: 0.0,
            mean: [0.0; N],
            scratch: Box::new([0.0; RING]),
            summed_at: 0,
            summed_ms: 0.0,
        }
    }
}

impl Frames {
    /// Folds in the frame that just ended: its marks, and the pipelines its render world built.
    fn fold(&mut self, at: &[f64; MARKS], next_began: f64, built: u32, what: &mut Vec<String>) {
        if self.began <= 0.0 {
            // The first frame: no marks yet.
            self.began = next_began;
            self.pipelines += built;
            what.clear();
            return;
        }
        // The render world runs only once the renderer is ready (and the main world's last mark
        // is where it would have begun): a part not reached this frame is 0.
        let mut parts = [0.0f32; N];
        let mut prev = at[0];
        for (i, p) in parts.iter_mut().enumerate() {
            let t = at[i + 1];
            if t >= prev && prev >= self.began {
                *p = (t - prev) as f32;
                prev = t;
            }
        }
        let work: f32 = parts.iter().sum();
        self.last = parts;
        self.last_work = work;
        self.last_interval = (next_began - self.began) as f32;
        self.began = next_began;
        self.ring[(self.n % RING as u64) as usize] = parts;
        self.n += 1;
        self.pipelines += built;
        if self.in_match {
            self.match_frames += 1;
            self.worst_ms = self.worst_ms.max(work);
            if work > SPIKE_MS {
                self.spikes += 1;
                let largest = (0..N).max_by(|&a, &b| parts[a].total_cmp(&parts[b])).unwrap_or(0);
                self.spike_by[largest] += 1;
            }
            if work > HITCH_MS {
                self.hitches += 1;
            }
            if built > 0 {
                self.late += built;
                self.late_ms += work;
                // Named with the match's frame it was built in: the first frames' are the match
                // coming into view, a later one is what a cast or a wave first needed.
                for w in what.drain(..) {
                    if self.late_what.len() < NAMED {
                        self.late_what.push(format!("{} @{}", w, self.match_frames));
                    }
                }
            }
        }
        what.clear();
    }

    /// The ring's median and 99th percentile, and each part's mean: a sort of [`RING`] numbers,
    /// so not every frame.
    fn sum_up(&mut self) {
        let n = (self.n as usize).min(RING);
        if n == 0 {
            return;
        }
        let mut mean = [0.0f32; N];
        for (i, parts) in self.ring[..n].iter().enumerate() {
            self.scratch[i] = parts.iter().sum();
            for (m, p) in mean.iter_mut().zip(parts) {
                *m += p;
            }
        }
        for m in &mut mean {
            *m /= n as f32;
        }
        self.mean = mean;
        let s = &mut self.scratch[..n];
        s.sort_unstable_by(|a, b| a.total_cmp(b));
        let at = |q: f32| s[((n as f32 - 1.0) * q).round() as usize];
        self.p50 = at(0.5);
        self.p99 = at(0.99);
        self.summed_at = self.n;
    }
}

/// `update 2.31, post 1.02, ...`: each part's mean (ms).
pub fn parts_line(parts: &Parts) -> String {
    PARTS.iter().zip(parts).map(|(k, v)| format!("{k} {v:.2}")).collect::<Vec<_>>().join(", ")
}

pub struct FramePlugin;

impl Plugin for FramePlugin {
    fn build(&self, app: &mut App) {
        {
            let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
            order.insert_before(First, MarkFrame);
            order.insert_before(Update, MarkUpdate);
            order.insert_before(SpawnScene, MarkPost);
            order.insert_before(Last, MarkLast);
            order.insert_after(Last, MarkEnd);
        }
        app.init_resource::<Frames>()
            .add_systems(MarkFrame, begin)
            .add_systems(MarkUpdate, || mark(1))
            .add_systems(MarkPost, || mark(2))
            .add_systems(MarkLast, || mark(3))
            .add_systems(MarkEnd, end);
        let Some(render) = app.get_sub_app_mut(RenderApp) else { return };
        render
            .configure_sets(Render, BuildSet.after(RenderSystems::Prepare).before(RenderSystems::Render))
            .add_systems(
                Render,
                (
                    (|| mark(5)).in_set(RenderSystems::ExtractCommands),
                    build.in_set(BuildSet),
                    (|| mark(8)).in_set(RenderSystems::Render).after(render_system),
                    (|| mark(9)).in_set(RenderSystems::PostCleanup),
                ),
            );
    }
}

/// The frame begins: the one before is folded in.
fn begin(mut frames: ResMut<Frames>) {
    let t = now_ms();
    let at = AT.with(Cell::get);
    BUILT.with(|b| {
        let mut b = b.borrow_mut();
        let (built, what) = &mut *b;
        frames.fold(&at, t, *built, what);
        *built = 0;
    });
    AT.with(|a| {
        let mut m = [0.0; MARKS];
        m[0] = t;
        a.set(m);
    });
    // Every 30 frames, or every second on a page that draws fewer (SwiftShader: about one).
    if frames.n >= frames.summed_at + 30 || (frames.n > frames.summed_at && t - frames.summed_ms > 1_000.0) {
        frames.sum_up();
        frames.summed_ms = t;
    }
}

/// The main world's end: whether this frame was in a match, what was added to the page in one,
/// and the numbers for `window.__el`.
fn end(
    session: Res<Session>,
    mut frames: ResMut<Frames>,
    mut images: MessageReader<AssetEvent<Image>>,
    mut meshes: MessageReader<AssetEvent<Mesh>>,
    mut status: ResMut<DevStatus>,
) {
    let in_match = crate::warm::in_match(&session);
    frames.in_match = in_match;
    let i = images.read().filter(|e| matches!(e, AssetEvent::Added { .. })).count();
    let m = meshes.read().filter(|e| matches!(e, AssetEvent::Added { .. })).count();
    if in_match {
        frames.images_late += i as u32;
        frames.meshes_late += m as u32;
    }
    status.set("cpu_p50", frames.p50);
    status.set("cpu_p99", frames.p99);
    status.set("cpu_last", frames.last_work);
    status.set("cpu_spikes", frames.spikes);
    status.set("cpu_hitches", frames.hitches);
    status.set("cpu_worst", frames.worst_ms);
    status.set("match_frames", frames.match_frames);
    status.set("pipelines", frames.pipelines);
    status.set("pipelines_late", frames.late);
    status.set("pipelines_late_ms", frames.late_ms);
    status.set("images_late", frames.images_late);
    status.set("meshes_late", frames.meshes_late);
    // The words only when they change: they're strings.
    if frames.summed_at == frames.n || frames.match_frames == 1 {
        status.set("cpu_parts", parts_line(&frames.mean));
        let by: Vec<String> = PARTS
            .iter()
            .zip(frames.spike_by)
            .filter(|(_, n)| *n > 0)
            .map(|(k, n)| format!("{k} {n}"))
            .collect();
        status.set("cpu_spike_parts", by.join(", "));
        status.set("pipelines_late_what", frames.late_what.join("; "));
    }
    mark(4);
}

/// The pipelines queued this frame, built on their own clock, and those that became ready named.
fn build(mut cache: ResMut<PipelineCache>, mut ready: Local<Vec<bool>>) {
    mark(6);
    cache.process_queue();
    let mut built = 0;
    let mut names = Vec::new();
    for (i, p) in cache.pipelines().enumerate() {
        if ready.len() <= i {
            ready.push(false);
        }
        if ready[i] || !matches!(p.state, CachedPipelineState::Ok(_)) {
            continue;
        }
        ready[i] = true;
        built += 1;
        names.push(describe(&p.descriptor));
    }
    if built > 0 {
        BUILT.with(|b| {
            let mut b = b.borrow_mut();
            b.0 += built;
            b.1.append(&mut names);
        });
    }
    mark(7);
}

/// A pipeline in a few words: its label, its fragment shader's file, and the flags of its key
/// that say what it's for.
fn describe(d: &PipelineDescriptor) -> String {
    let PipelineDescriptor::RenderPipelineDescriptor(d) = d else { return "compute".into() };
    let mut s = d.label.as_deref().unwrap_or("?").to_string();
    if let Some(f) = &d.fragment
        && let Some(path) = f.shader.path()
    {
        let p = path.path().to_string_lossy();
        s.push(' ');
        s.push_str(p.rsplit('/').next().unwrap_or(&p));
    }
    const FLAGS: [&str; 8] = [
        "SKINNED",
        "MORPH_TARGETS",
        "VERTEX_COLORS",
        "VERTEX_TANGENTS",
        "DEPTH_PREPASS",
        "PREPASS_PIPELINE",
        "BLEND_ALPHA",
        "BLEND_PREMULTIPLIED_ALPHA",
    ];
    for def in &d.vertex.shader_defs {
        let name = match def {
            bevy::shader::ShaderDefVal::Bool(n, true) => n.as_str(),
            bevy::shader::ShaderDefVal::Bool(..) => continue,
            bevy::shader::ShaderDefVal::Int(n, _) | bevy::shader::ShaderDefVal::UInt(n, _) => n.as_str(),
        };
        if FLAGS.contains(&name) {
            s.push(' ');
            s.push_str(&name.to_lowercase());
        }
    }
    if d.multisample.count > 1 {
        s.push_str(&format!(" msaa{}", d.multisample.count));
    }
    s
}
