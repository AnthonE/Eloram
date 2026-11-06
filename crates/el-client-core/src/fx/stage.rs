//! The fx stage (`docs/FX.md`, "The stage"): one effect staged alone, fired twice at once in front
//! of a dark card and a light card, on a clock that steps 1/60 s and holds where a shooter asks,
//! so a strip of frames can be shot and measured (`e2e/fxstrip.mjs`, `scripts/fx.sh`). The
//! contract is Gates' (`AnthonE/Gates`, `crates/fx/src/stage.rs`), so the same shooter and the
//! same loop serve both games: step 0 the plate, the effect fired on step 1, `window.__fxStep`
//! drives, `window.__fx` says where the page stands.
//!
//! This is the part with no engine in it: the catalogue of what can be staged (every part of every
//! ability, the world's effects), the cue the stage fires for each, the clock, and the knobs that
//! retune a recipe without a rebuild. The browser client's `stage.rs` draws it.

use el_sim::content::champions::{BLINK_SLOT, CHAMPIONS, MARK_SLOT};
use el_sim::missile::Source;
use el_sim::units::Kind;
use glam::Vec2;

use super::director::{Cue, Status, What};
use super::look::{self, Look, Reach, Side};
use super::sound::Part;

/// The flipbook sheet the bursts play (`web/assets/fx/cloud_{a,b}.png`, `docs/FX.md`,
/// "Flipbooks"): its cut, which `art/tools/flipbooks.py` bakes and `fx_burst.wgsl` reads; a test
/// here holds the three to the same numbers.
pub mod sheet {
    /// Cells a side, and texels a cell.
    pub const CELLS: u32 = 8;
    pub const TEXELS: u32 = 128;
    /// A strip of frames: its first cell, its variants (each the next flipbook along), the frames
    /// in each.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Strip {
        pub name: &'static str,
        pub cell: u32,
        pub variants: u32,
        pub frames: u32,
    }
    /// A puff of smoke thrown up, swelling, rolling, thinning.
    pub const SMOKE: Strip = Strip { name: "smoke", cell: 0, variants: 2, frames: 16 };
    /// A fireball blown out, burning to soot.
    pub const BOOM: Strip = Strip { name: "boom", cell: 32, variants: 2, frames: 8 };
    /// A flash of petals, a hit's.
    pub const FLASH: Strip = Strip { name: "flash", cell: 48, variants: 4, frames: 4 };
    pub const STRIPS: [Strip; 3] = [SMOKE, BOOM, FLASH];
}

/// Steps a second.
pub const HZ: u32 = 60;
/// One step, seconds.
pub const DT: f32 = 1.0 / HZ as f32;
/// Frames drawn before the plate, so pipelines compile and the camera settles.
pub const WARM: u32 = 8;

/// Each part, in `Part`'s order.
pub const PARTS: [Part; Part::COUNT] =
    [Part::Cast, Part::Launch, Part::Impact, Part::Blast, Part::Windup, Part::Dash, Part::Buff, Part::Ult];

pub fn part_name(p: Part) -> &'static str {
    match p {
        Part::Cast => "cast",
        Part::Launch => "launch",
        Part::Impact => "impact",
        Part::Blast => "blast",
        Part::Windup => "windup",
        Part::Dash => "dash",
        Part::Buff => "buff",
        Part::Ult => "ult",
    }
}

fn part_named(s: &str) -> Option<Part> {
    PARTS.iter().copied().find(|p| part_name(*p) == s)
}

const STATUSES: [(Status, &str); 10] = [
    (Status::Stun, "stun"),
    (Status::Root, "root"),
    (Status::Silence, "silence"),
    (Status::Slow, "slow"),
    (Status::Airborne, "airborne"),
    (Status::Shove, "shove"),
    (Status::Reveal, "reveal"),
    (Status::Immune, "immune"),
    (Status::Empowered, "empowered"),
    (Status::Hasted, "hasted"),
];

const DEATHS: [(Kind, &str); 5] = [
    (Kind::Champion, "champion"),
    (Kind::Melee, "minion"),
    (Kind::Turret, "turret"),
    (Kind::Inhibitor, "inhibitor"),
    (Kind::Nexus, "nexus"),
];

/// What the stage fires.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spec {
    /// A part of a champion's ability (Q to R; Blink and Tag by their slots).
    Ability {
        champ: u8,
        slot: u8,
        part: Part,
    },
    /// A part of a champion's auto attack (its launch, where it flies; its impact).
    Auto {
        champ: u8,
        part: Part,
    },
    /// A turret's shot leaving, or landing.
    Turret(Part),
    Status(Status),
    Shield,
    Heal,
    /// A skillshot that ran out with no one in its way.
    Fizzle,
    Death(Kind),
    Respawn,
    LevelUp,
    Relic {
        beam: bool,
    },
}

/// One effect the stage can fire.
#[derive(Clone, Debug, PartialEq)]
pub struct Staged {
    pub name: String,
    /// What it is, in words.
    pub what: String,
    /// About how far it reaches from where it fires, metres: the stage frames it by this.
    pub radius: f32,
    /// How long it lasts, seconds: a strip's frames span this.
    pub span: f32,
    pub spec: Spec,
}

fn champ_slug(champ: u8) -> String {
    CHAMPIONS[usize::from(champ)].name.to_ascii_lowercase()
}

fn champ_named(s: &str) -> Option<u8> {
    CHAMPIONS.iter().position(|c| c.name.eq_ignore_ascii_case(s)).map(|i| i as u8)
}

fn slot_name(slot: u8) -> &'static str {
    match slot {
        0 => "q",
        1 => "w",
        2 => "e",
        3 => "r",
        BLINK_SLOT => "blink",
        _ => "tag",
    }
}

fn slot_named(s: &str) -> Option<u8> {
    Some(match s {
        "q" => 0,
        "w" => 1,
        "e" => 2,
        "r" => 3,
        "blink" => BLINK_SLOT,
        "tag" => MARK_SLOT,
        _ => return None,
    })
}

/// The parts an ability has, as its look says (the director cues these and no others).
pub fn parts_of(l: &Look) -> impl Iterator<Item = Part> + '_ {
    PARTS.into_iter().filter(move |p| l.sound(*p).is_some())
}

/// How far a part reaches (m) and how long it lasts (s) on the stage, from its look.
fn frame_of(part: Part, l: &Look) -> (f32, f32) {
    let area = match l.reach {
        Reach::Area { radius, .. } | Reach::Field { radius } => radius * 0.01,
        _ => 0.0,
    };
    match part {
        Part::Cast => (0.9, 0.7),
        Part::Ult => (2.6, 1.2),
        Part::Launch => (0.7, 0.5),
        Part::Impact => (0.9, 1.3),
        Part::Blast => ((area * 1.3).max(1.0) + 0.5, 1.6),
        Part::Windup => ((area * 1.2).max(1.0), 1.2),
        Part::Dash => (2.0, 0.8),
        Part::Buff => (1.2, 1.0),
    }
}

impl Staged {
    fn new(name: String, what: String, radius: f32, span: f32, spec: Spec) -> Self {
        Staged { name, what, radius, span, spec }
    }

    /// Where the effect's middle is drawn, as the recipes put it (`el-client/src/fx/burst.rs`):
    /// metres above the floor, and metres toward the camera (a hit is drawn on the body's near
    /// side). The stage looks there, so the effect sits in the middle of its card.
    pub fn lift(&self) -> (f32, f32) {
        let on_body = (1.0, 0.55);
        match self.spec {
            Spec::Ability { part, .. } | Spec::Auto { part, .. } => match part {
                Part::Impact => on_body,
                Part::Launch => on_body,
                Part::Cast => (0.15, 0.0),
                Part::Ult => (0.6, 0.0),
                Part::Blast => (0.2, 0.0),
                Part::Windup => (0.3, 0.0),
                Part::Dash => (0.5, 0.0),
                Part::Buff => (0.5, 0.0),
            },
            Spec::Turret(Part::Launch) => (1.1, 0.0),
            Spec::Turret(_) => (1.6, 1.3),
            Spec::Status(Status::Stun | Status::Silence | Status::Reveal) => (2.2, 0.0),
            Spec::Status(Status::Immune) => (1.0, 0.0),
            Spec::Status(_) => (0.2, 0.0),
            Spec::Shield | Spec::Heal => (0.5, 0.0),
            Spec::Fizzle => (1.1, 0.0),
            Spec::Death(Kind::Champion) => (0.8, 0.0),
            Spec::Death(k) if k.is_structure() => (1.6, 0.0),
            Spec::Death(_) => (0.3, 0.0),
            Spec::Respawn | Spec::LevelUp => (1.2, 0.0),
            Spec::Relic { beam: true } => (1.8, 0.0),
            Spec::Relic { beam: false } => (0.6, 0.0),
        }
    }

    /// The look the cue is drawn with.
    pub fn look(&self) -> Look {
        match self.spec {
            Spec::Ability { champ, slot, .. } => look::ability_look(champ, slot),
            Spec::Auto { champ, .. } => look::look(Source::Auto, Some(champ), 0),
            Spec::Turret(_) => look::look(Source::Turret, None, 1),
            Spec::Relic { .. } => look::look(Source::Relic, None, 0),
            Spec::Status(s) => {
                let mut l = look::look(Source::Auto, None, 0);
                l.palette = s.palette();
                l
            }
            Spec::Shield | Spec::Heal => look::look(Source::Item(0), None, 0),
            Spec::Fizzle | Spec::Death(_) | Spec::Respawn | Spec::LevelUp => {
                look::look(Source::Auto, Some(0), 0)
            }
        }
    }

    /// The cue the stage fires at `at` (map units), as `side` sees it. `right` is the screen's
    /// right on the map, for what moves (a dash comes in from the left); `size` is the cue's
    /// (a blast's radius in units, a hit's damage), or the ability's own where it has one.
    pub fn cue(&self, at: Vec2, right: Vec2, side: Side, size: Option<f32>) -> Cue {
        let look = self.look();
        let own = side == Side::Own;
        let (what, source, unit, from, size) = match self.spec {
            Spec::Ability { champ, slot, part } => {
                let source = Source::Ability { champ, slot };
                let area = match look.reach {
                    Reach::Area { radius, .. } | Reach::Field { radius } => radius,
                    _ => 0.0,
                };
                let (unit, size) = match part {
                    Part::Impact => (Some(1), size.unwrap_or(150.0)),
                    Part::Blast | Part::Windup => (None, size.unwrap_or(area)),
                    Part::Buff => (Some(1), 0.0),
                    _ => (None, 0.0),
                };
                let from = (part == Part::Dash).then(|| at - right * 150.0);
                (What::Part(part), source, unit, from, size)
            }
            Spec::Auto { part, .. } => (What::Part(part), Source::Auto, Some(1), None, size.unwrap_or(60.0)),
            Spec::Turret(part) => (What::Part(part), Source::Turret, Some(1), None, size.unwrap_or(200.0)),
            Spec::Status(s) => (What::Status(s), Source::Auto, Some(1), None, 0.0),
            Spec::Shield => (What::Shield, Source::Item(0), Some(1), None, 0.0),
            Spec::Heal => (What::Heal, Source::Item(0), Some(1), None, 0.0),
            Spec::Fizzle => (What::Fizzle, Source::Auto, None, None, 0.0),
            Spec::Death(k) => (What::Death(k), Source::Auto, None, None, 0.0),
            Spec::Respawn => (What::Respawn, Source::Auto, Some(1), None, 0.0),
            Spec::LevelUp => (What::LevelUp, Source::Auto, Some(1), None, 0.0),
            Spec::Relic { beam } => (What::Relic { beam }, Source::Relic, None, None, 0.0),
        };
        Cue {
            what,
            source,
            look,
            at,
            from,
            unit,
            side,
            rating: look.ratings.of(side),
            size,
            sound: None,
            rung: 0,
            own,
        }
    }
}

/// Everything the stage can fire, in a fixed order: each champion's abilities part by part (Q to
/// R), its auto attack, then Blink and Tag, then the world's.
pub fn list() -> Vec<Staged> {
    let mut out = Vec::new();
    for (c, def) in CHAMPIONS.iter().enumerate() {
        let champ = c as u8;
        let slug = champ_slug(champ);
        for slot in 0..4u8 {
            let l = look::ability_look(champ, slot);
            let a = el_sim::content::champions::ability(champ, slot);
            for part in parts_of(&l) {
                let (radius, span) = frame_of(part, &l);
                out.push(Staged::new(
                    format!("{slug}-{}-{}", slot_name(slot), part_name(part)),
                    format!(
                        "{}'s {} ({}): its {}",
                        def.name,
                        a.name,
                        slot_name(slot).to_ascii_uppercase(),
                        part_name(part)
                    ),
                    radius,
                    span,
                    Spec::Ability { champ, slot, part },
                ));
            }
        }
        let auto = look::look(Source::Auto, Some(champ), 0);
        for part in parts_of(&auto) {
            let (radius, span) = frame_of(part, &auto);
            out.push(Staged::new(
                format!("{slug}-auto-{}", part_name(part)),
                format!("{}'s auto attack: its {}", def.name, part_name(part)),
                radius,
                span,
                Spec::Auto { champ, part },
            ));
        }
    }
    for slot in [BLINK_SLOT, MARK_SLOT] {
        let l = look::ability_look(0, slot);
        for part in parts_of(&l) {
            let (radius, span) = frame_of(part, &l);
            out.push(Staged::new(
                format!("{}-{}", slot_name(slot), part_name(part)),
                format!("{}: its {}", if slot == BLINK_SLOT { "Blink" } else { "Tag" }, part_name(part)),
                radius,
                span,
                Spec::Ability { champ: 0, slot, part },
            ));
        }
    }
    let turret = look::look(Source::Turret, None, 1);
    for part in parts_of(&turret) {
        let (radius, span) = frame_of(part, &turret);
        out.push(Staged::new(
            format!("turret-{}", part_name(part)),
            format!("a turret's shot: its {}", part_name(part)),
            radius,
            span,
            Spec::Turret(part),
        ));
    }
    for (s, n) in STATUSES {
        out.push(Staged::new(
            format!("status-{n}"),
            format!("the status {n} rising on a champion"),
            1.3,
            0.9,
            Spec::Status(s),
        ));
    }
    out.push(Staged::new("shield".into(), "a shield given".into(), 1.2, 1.0, Spec::Shield));
    out.push(Staged::new("heal".into(), "a heal given".into(), 1.2, 1.0, Spec::Heal));
    out.push(Staged::new("fizzle".into(), "a skillshot running out".into(), 0.8, 0.5, Spec::Fizzle));
    for (k, n) in DEATHS {
        let (radius, span) = match k {
            Kind::Champion => (2.0, 1.6),
            Kind::Nexus => (6.5, 3.0),
            Kind::Turret | Kind::Inhibitor => (4.0, 2.5),
            _ => (0.8, 0.8),
        };
        out.push(Staged::new(format!("death-{n}"), format!("a {n} falling"), radius, span, Spec::Death(k)));
    }
    out.push(Staged::new("respawn".into(), "a champion respawning".into(), 1.6, 1.1, Spec::Respawn));
    out.push(Staged::new("levelup".into(), "a champion levelling up".into(), 1.6, 1.1, Spec::LevelUp));
    out.push(Staged::new("relic".into(), "a relic taken".into(), 1.4, 1.0, Spec::Relic { beam: false }));
    out.push(Staged::new(
        "relic-beam".into(),
        "the relic's beam".into(),
        1.0,
        1.4,
        Spec::Relic { beam: true },
    ));
    out
}

/// The effect named `name` (`orrin-q-impact`, `liora-r-blast`, `death-turret`...), if there is one.
pub fn find(name: &str) -> Option<Staged> {
    let name = name.trim().to_ascii_lowercase();
    list().into_iter().find(|s| s.name == name)
}

/// The champion and slot a name like `orrin-q-impact` names, for a page that shows the champion.
pub fn ability_named(name: &str) -> Option<(u8, u8)> {
    let name = name.trim().to_ascii_lowercase();
    let mut it = name.split('-');
    let (c, s, p) = (it.next()?, it.next()?, it.next()?);
    part_named(p)?;
    Some((champ_named(c)?, slot_named(s)?))
}

/// One frame of the stage's clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tick {
    /// Seconds to advance the picture by this frame: [`DT`] or zero.
    pub dt: f32,
    /// The step the clock stands at after this frame.
    pub step: u32,
    /// Fire the effect this frame (the clock just reached step 1).
    pub fire: bool,
    /// Clear what's left of the last run (a looping clock went back to 0).
    pub clear: bool,
}

/// The stage's clock (Gates' `fx::stage::Clock`): one call to [`Clock::frame`] a frame drawn. It
/// steps once a frame until it stands where a shooter asked (`window.__fxStep`), then holds;
/// with no shooter it runs free, replaying the effect every `loop_steps`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Clock {
    frames: u32,
    step: u32,
    held: u32,
    /// Free-running, back to the plate after this many steps (0: never).
    pub loop_steps: u32,
}

impl Clock {
    pub fn looping(steps: u32) -> Self {
        Self { loop_steps: steps, ..Self::default() }
    }

    pub fn frame(&mut self, want: Option<u32>) -> Tick {
        self.frames = self.frames.saturating_add(1);
        let mut tick = Tick { dt: 0.0, step: self.step, fire: false, clear: false };
        if self.frames <= WARM {
            return tick;
        }
        let go = match want {
            Some(w) => self.step < w,
            None => true,
        };
        if !go {
            self.held = self.held.saturating_add(1);
            return tick;
        }
        if want.is_none() && self.loop_steps > 0 && self.step >= self.loop_steps {
            self.step = 0;
            self.held = 0;
            tick.step = 0;
            tick.clear = true;
            return tick;
        }
        self.step += 1;
        self.held = 0;
        tick.dt = DT;
        tick.step = self.step;
        tick.fire = self.step == 1;
        tick
    }

    /// The warm-up is over and the plate is on screen.
    pub fn ready(&self) -> bool {
        self.frames >= WARM
    }

    pub fn step(&self) -> u32 {
        self.step
    }

    /// Frames drawn since the clock last moved: a shooter takes its frame once this is 2 or more.
    pub fn held(&self) -> u32 {
        self.held
    }

    /// Seconds since the effect fired, at the step the clock stands at.
    pub fn t(&self) -> f32 {
        self.step.saturating_sub(1) as f32 * DT
    }
}

/// Knobs: named multipliers on a recipe's numbers (`impact.star=1.5;size=0.8`), read where the
/// recipe is made, so a look is retuned on the stage without a rebuild and the numbers that work
/// are then written into the recipe. In a match every knob is 1.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Knobs {
    pub list: Vec<(String, f32)>,
}

/// Every knob the recipes read, and what it scales.
pub const KNOBS: &[(&str, &str)] = &[
    ("count", "how many particles every burst throws"),
    ("size", "every particle's size"),
    ("life", "every particle's life"),
    ("speed", "how fast particles are thrown"),
    ("bright", "what glows: its colour's brightness"),
    ("smoke", "what's laid over (smoke, dust, rubble, scorch): its alpha"),
    ("hot", "how white-hot light is born before it cools to its colour (the shader)"),
    ("night", "how brightly the night lights smoke and dust (the shader)"),
    ("puff", "a puff's alpha (the shader)"),
    ("flash", "a ground flash's and a star's brightness (the shader)"),
    ("impact.glow", "a hit's glow"),
    ("impact.star", "a hit's star of rays"),
    ("impact.sparks", "what flies off a hit"),
    ("blast.flash", "a blast's flash on the ground"),
    ("blast.ring", "a blast's shockwave"),
    ("blast.puff", "a blast's smoke and dust"),
    ("blast.scorch", "the mark a blast leaves"),
    ("cast.glow", "a cast's motes"),
    ("cast.flash", "a cast's flash on the ground"),
    ("buff.ring", "a buff's ring"),
    ("buff.glow", "a buff's rising motes"),
    ("dash.flash", "a dash's flash where it lands"),
];

impl Knobs {
    /// `a=1.2;b.c=0.5`, or an error naming the first that isn't a knob or a number.
    pub fn parse(s: &str) -> Result<Knobs, String> {
        let mut list = Vec::new();
        for item in s.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let (k, v) = item.split_once('=').ok_or_else(|| format!("{item}: not key=value"))?;
            let k = k.trim();
            if !KNOBS.iter().any(|(n, _)| *n == k) {
                return Err(format!("{k}: no such knob"));
            }
            let v: f32 = v.trim().parse().map_err(|_| format!("{item}: not a number"))?;
            list.retain(|(n, _): &(String, f32)| n != k);
            list.push((k.to_string(), v));
        }
        Ok(Knobs { list })
    }

    /// The knob's multiplier, 1 where it isn't set.
    pub fn k(&self, name: &str) -> f32 {
        self.list.iter().find(|(n, _)| n == name).map_or(1.0, |(_, v)| *v)
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// As `parse` takes it.
    pub fn text(&self) -> String {
        self.list.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(";")
    }
}

/// The steps a strip shoots for an effect of `span_s` seconds, `n` frames of it: step 0 (the
/// plate), then `n` steps from 1 to the end packed toward the start, where an impact happens
/// (`fxstrip.mjs` makes the same list for a page it drives).
pub fn steps(span_s: f32, n: usize) -> Vec<u32> {
    let last = ((span_s.max(DT) * HZ as f32).round() as u32).max(1);
    let mut out = vec![0];
    let n = n.max(1);
    for i in 0..n {
        let f = if n == 1 { 1.0 } else { i as f32 / (n - 1) as f32 };
        let s = 1 + (f * f * (last - 1) as f32).round() as u32;
        if out.last() != Some(&s) {
            out.push(s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ability_of_every_champion_is_staged_with_its_parts() {
        let all = list();
        for (c, def) in CHAMPIONS.iter().enumerate() {
            let slug = def.name.to_ascii_lowercase();
            for slot in ["q", "w", "e", "r"] {
                let n = format!("{slug}-{slot}-cast");
                assert!(all.iter().any(|s| s.name == n), "{n} isn't staged");
            }
            // A champion with a skillshot has its impact; one with an area has its blast.
            let l = look::ability_look(c as u8, 0);
            for p in parts_of(&l) {
                let n = format!("{slug}-q-{}", part_name(p));
                assert!(all.iter().any(|s| s.name == n), "{n} isn't staged");
            }
        }
        for n in ["blink-dash", "tag-impact", "turret-impact", "status-stun", "death-turret", "relic-beam"] {
            assert!(all.iter().any(|s| s.name == n), "{n} isn't staged");
        }
    }

    #[test]
    fn names_are_unique_and_found() {
        let all = list();
        let mut names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
        names.sort_unstable();
        let n = names.len();
        names.dedup();
        assert_eq!(n, names.len(), "a name twice");
        for s in &all {
            let f = find(&s.name).unwrap_or_else(|| panic!("{} not found", s.name));
            assert_eq!(f.spec, s.spec, "{}", s.name);
            assert_eq!(f.what, s.what, "{}", s.name);
            assert!(f.radius > 0.0 && f.span > 0.0);
            let (up, out) = f.lift();
            assert!(
                (0.0..=3.0).contains(&up) && (0.0..=2.0).contains(&out),
                "{}: lifted {up}, {out}",
                s.name
            );
        }
        assert!(find("orrin-q-nothing").is_none());
        assert!(find("nobody-q-cast").is_none());
        assert_eq!(find("Orrin-Q-Cast").map(|s| s.name), Some("orrin-q-cast".into()));
    }

    #[test]
    fn a_cue_is_what_the_director_would_say() {
        let s = find("orrin-q-impact").unwrap();
        let q = s.cue(Vec2::new(100.0, 200.0), Vec2::X, Side::Own, None);
        assert_eq!(q.what, What::Part(Part::Impact));
        assert_eq!(q.source, Source::Ability { champ: 1, slot: 0 });
        assert_eq!(q.unit, Some(1));
        assert!(q.own && q.sound.is_none());
        assert_eq!(q.rating, q.look.ratings.own);
        let e = s.cue(Vec2::ZERO, Vec2::X, Side::Enemy, Some(300.0));
        assert_eq!(e.rating, e.look.ratings.enemy);
        assert_eq!(e.size, 300.0);
        assert!(!e.own);
        let d = find("blink-dash").unwrap().cue(Vec2::new(500.0, 0.0), Vec2::X, Side::Own, None);
        assert_eq!(d.from, Some(Vec2::new(350.0, 0.0)));
        // A blast's size is its area's radius unless given.
        let b =
            list().into_iter().find(|s| matches!(s.spec, Spec::Ability { part: Part::Blast, .. })).unwrap();
        let q = b.cue(Vec2::ZERO, Vec2::X, Side::Own, None);
        assert!(q.size > 0.0, "{}: a blast with no radius", b.name);
    }

    #[test]
    fn the_clock_warms_up_steps_once_a_frame_and_holds() {
        let mut c = Clock::default();
        for _ in 0..WARM {
            let t = c.frame(Some(0));
            assert_eq!((t.dt, t.step, t.fire), (0.0, 0, false));
        }
        assert!(c.ready());
        // Held at the plate: frames count up, nothing moves.
        c.frame(Some(0));
        c.frame(Some(0));
        assert_eq!((c.step(), c.held()), (0, 2));
        // Asked for 3: one step a frame, the effect fired on the first.
        let t = c.frame(Some(3));
        assert_eq!((t.dt, t.step, t.fire), (DT, 1, true));
        assert_eq!(c.held(), 0);
        let t = c.frame(Some(3));
        assert_eq!((t.step, t.fire), (2, false));
        c.frame(Some(3));
        let t = c.frame(Some(3));
        assert_eq!((t.dt, t.step, c.held()), (0.0, 3, 1));
        assert!((c.t() - 2.0 * DT).abs() < 1e-6);
        // Looping, undriven: round to the plate after its steps.
        let mut l = Clock::looping(2);
        for _ in 0..WARM {
            l.frame(None);
        }
        assert_eq!(l.frame(None).step, 1);
        assert_eq!(l.frame(None).step, 2);
        let t = l.frame(None);
        assert!(t.clear && t.step == 0);
        assert!(l.frame(None).fire);
    }

    #[test]
    fn knobs_parse_and_read() {
        let k = Knobs::parse("impact.star=1.5; size=0.8;impact.star=2").unwrap();
        assert_eq!(k.k("impact.star"), 2.0);
        assert_eq!(k.k("size"), 0.8);
        assert_eq!(k.k("life"), 1.0);
        assert_eq!(k.text(), "size=0.8;impact.star=2");
        assert!(Knobs::parse("").unwrap().is_empty());
        assert!(Knobs::parse("nothing=1").is_err());
        assert!(Knobs::parse("size=big").is_err());
        assert!(Knobs::parse("size").is_err());
    }

    #[test]
    fn the_sheets_strips_tile_it_as_the_baker_and_the_shader_have_them() {
        use sheet::*;
        let mut cell = 0;
        for s in STRIPS {
            assert_eq!(s.cell, cell, "{}", s.name);
            cell += s.variants * s.frames;
        }
        assert_eq!(cell, CELLS * CELLS);
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let baker = std::fs::read_to_string(format!("{root}/art/tools/flipbooks.py")).unwrap();
        assert!(
            baker.contains(&format!("CELLS = {CELLS}\n")) && baker.contains(&format!("TEX = {TEXELS}\n"))
        );
        let shader =
            std::fs::read_to_string(format!("{root}/crates/el-client/src/shaders/fx_burst.wgsl")).unwrap();
        assert!(shader.contains(&format!("const SHEET_CELLS: f32 = {CELLS}.0;")));
        for s in STRIPS {
            let line = format!("(\"{}\", {}, {}, ", s.name, s.variants, s.frames);
            assert!(baker.contains(&line), "the baker's STRIPS lack {line}");
            let v = format!("vec3({}.0, {}.0, {}.0)", s.cell, s.variants, s.frames);
            assert!(shader.contains(&v), "the shader's strip() lacks {v} for {}", s.name);
        }
    }

    #[test]
    fn the_strips_steps_start_dense_and_end_at_the_span() {
        let s = steps(1.0, 11);
        assert_eq!(s[0], 0);
        assert_eq!(s[1], 1);
        assert_eq!(*s.last().unwrap(), 60);
        assert!(s[5] < 20);
        assert!(s.windows(2).all(|w| w[0] < w[1]));
    }
}
