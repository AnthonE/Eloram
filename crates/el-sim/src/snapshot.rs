//! A whole match as bytes: a checkpoint (`docs/NETCODE.md`, "The record").
//!
//! - A checkpoint is a cache of the record, never a truth of its own: the server's referee
//!   re-steps the record to a checkpoint's tick and compares before anyone resumes from it.
//! - Every field of every slot, the dead ones too, so a match read back steps exactly as the one
//!   written (the hash covers less than this does; `tests/snapshot.rs` steps both to show it).
//! - Little-endian, floats as their bits, enums padded to their widest: the same bytes native and
//!   wasm, and the same length for every match on a map (`len`).
//! - Never the offer secret (offers stay out of a checkpoint's reach as they stay out of the
//!   record's), never the tick's events or its scratch (`keys_drawn`, `path_budget`), which a step
//!   starts by clearing.
//! - Each type's fields are destructured, so a field added to the match is a compile error here
//!   until the snapshot has it.

use core::fmt;

use glam::Vec2;

use crate::content::{Filter, StatMods};
use crate::game::{Casting, Champ, Dash, Intent, Match, PATH_BUDGET, Relic, Screen, Vision};
use crate::handle::Handle;
use crate::hash::Fnv;
use crate::math::Rng;
use crate::missile::{Missile, Source};
use crate::path::Path;
use crate::record::CONTENT_VERSION;
use crate::storage::{BitSet, FreeList};
use crate::units::{Buff, Cc, Kind, Stats, Unit};
use crate::voice::Vote;
use crate::zone::Zone;

pub const MAGIC: [u8; 4] = *b"ELS1";
/// Bumped whenever the layout changes (a field added, a width changed).
pub const VERSION: u16 = 1;
/// Magic, version, content version, tick, and the three pools' sizes.
const HEAD: usize = 4 + 2 + 4 + 4 + 3 * 2;
/// The FNV-1a digest of everything before it.
const TAIL: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapError {
    Short,
    Magic,
    Version {
        snap: u16,
        content: u32,
    },
    /// The pools' sizes aren't this match's.
    Shape,
    Checksum,
    /// A field out of its range (a bool that isn't 0 or 1, an enum's unknown tag).
    Bad,
}

impl fmt::Display for SnapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapError::Short => f.write_str("cut short"),
            SnapError::Magic => f.write_str("not a checkpoint"),
            SnapError::Version { snap, content } => {
                write!(f, "layout {snap} content {content}, this build {VERSION} and {CONTENT_VERSION}")
            }
            SnapError::Shape => f.write_str("another match's shape"),
            SnapError::Checksum => f.write_str("checksum wrong"),
            SnapError::Bad => f.write_str("a field out of range"),
        }
    }
}

/// Bytes being written. Past the end of the buffer it only counts, so [`len`] is a write into
/// nothing.
pub struct Out<'a> {
    buf: &'a mut [u8],
    at: usize,
}

impl Out<'_> {
    #[inline]
    fn bytes(&mut self, b: &[u8]) {
        if let Some(d) = self.buf.get_mut(self.at..self.at + b.len()) {
            d.copy_from_slice(b);
        }
        self.at += b.len();
    }
    fn fits(&self) -> bool {
        self.at <= self.buf.len()
    }
}

/// Bytes being read.
pub struct In<'a> {
    buf: &'a [u8],
    at: usize,
}

impl In<'_> {
    #[inline]
    fn take<const N: usize>(&mut self) -> Result<[u8; N], SnapError> {
        let b = self.buf.get(self.at..self.at + N).ok_or(SnapError::Short)?;
        self.at += N;
        let mut out = [0u8; N];
        out.copy_from_slice(b);
        Ok(out)
    }
}

/// Where two matches first differ: the names from the outside in (`stats`, `max_hp`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Field {
    names: [&'static str; 4],
    n: u8,
}

impl Field {
    /// Names are pushed innermost first, as the comparison unwinds.
    fn push(&mut self, name: &'static str) {
        if usize::from(self.n) < self.names.len() {
            self.names[usize::from(self.n)] = name;
            self.n += 1;
        }
    }
    /// The names, outermost first.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.names[..usize::from(self.n)].iter().rev().copied()
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (k, n) in self.names().enumerate() {
            if k > 0 {
                f.write_str(".")?;
            }
            f.write_str(n)?;
        }
        Ok(())
    }
}

/// A type with a fixed-width encoding, read back in place.
pub trait Snap {
    fn put(&self, o: &mut Out<'_>);
    fn get(&mut self, i: &mut In<'_>) -> Result<(), SnapError>;
    /// Whether `other` differs, naming the field into `at` if so.
    fn differs(&self, other: &Self, at: &mut Field) -> bool;
}

macro_rules! snap_int {
    ($($t:ty),*) => {$(
        impl Snap for $t {
            #[inline]
            fn put(&self, o: &mut Out<'_>) {
                o.bytes(&self.to_le_bytes());
            }
            #[inline]
            fn get(&mut self, i: &mut In<'_>) -> Result<(), SnapError> {
                *self = <$t>::from_le_bytes(i.take()?);
                Ok(())
            }
            #[inline]
            fn differs(&self, other: &Self, _: &mut Field) -> bool {
                self != other
            }
        }
    )*};
}
snap_int!(u8, u16, u32, u64);

impl Snap for f32 {
    #[inline]
    fn put(&self, o: &mut Out<'_>) {
        self.to_bits().put(o);
    }
    #[inline]
    fn get(&mut self, i: &mut In<'_>) -> Result<(), SnapError> {
        *self = f32::from_bits(u32::from_le_bytes(i.take()?));
        Ok(())
    }
    #[inline]
    fn differs(&self, other: &Self, _: &mut Field) -> bool {
        self.to_bits() != other.to_bits()
    }
}

impl Snap for bool {
    #[inline]
    fn put(&self, o: &mut Out<'_>) {
        u8::from(*self).put(o);
    }
    #[inline]
    fn get(&mut self, i: &mut In<'_>) -> Result<(), SnapError> {
        *self = match i.take::<1>()?[0] {
            0 => false,
            1 => true,
            _ => return Err(SnapError::Bad),
        };
        Ok(())
    }
    #[inline]
    fn differs(&self, other: &Self, _: &mut Field) -> bool {
        self != other
    }
}

impl Snap for Vec2 {
    #[inline]
    fn put(&self, o: &mut Out<'_>) {
        self.x.put(o);
        self.y.put(o);
    }
    #[inline]
    fn get(&mut self, i: &mut In<'_>) -> Result<(), SnapError> {
        self.x.get(i)?;
        self.y.get(i)
    }
    #[inline]
    fn differs(&self, other: &Self, _: &mut Field) -> bool {
        self.x.to_bits() != other.x.to_bits() || self.y.to_bits() != other.y.to_bits()
    }
}

impl<T: Snap, const N: usize> Snap for [T; N] {
    fn put(&self, o: &mut Out<'_>) {
        for v in self {
            v.put(o);
        }
    }
    fn get(&mut self, i: &mut In<'_>) -> Result<(), SnapError> {
        for v in self {
            v.get(i)?;
        }
        Ok(())
    }
    fn differs(&self, other: &Self, at: &mut Field) -> bool {
        self.iter().zip(other).any(|(a, b)| a.differs(b, at))
    }
}

/// A struct, field by field. The destructuring names every field, so one left out doesn't compile.
macro_rules! snap_struct {
    ($t:ident { $($f:ident),* $(,)? }) => {
        impl Snap for $t {
            fn put(&self, o: &mut Out<'_>) {
                let $t { $($f),* } = self;
                $( $f.put(o); )*
            }
            fn get(&mut self, i: &mut In<'_>) -> Result<(), SnapError> {
                let $t { $($f),* } = self;
                $( $f.get(i)?; )*
                Ok(())
            }
            fn differs(&self, other: &Self, at: &mut Field) -> bool {
                let $t { $($f),* } = self;
                $(
                    if $f.differs(&other.$f, at) {
                        at.push(stringify!($f));
                        return true;
                    }
                )*
                false
            }
        }
    };
}

snap_struct!(Handle { idx, generation });
snap_struct!(Stats {
    max_hp,
    hp_regen,
    max_mana,
    mana_regen,
    ad,
    ap,
    armor,
    mr,
    attack_speed,
    range,
    move_speed,
    haste,
    lifesteal,
});
snap_struct!(Cc { stun, root, silence, airborne, slow_until, slow, immune });
snap_struct!(Buff { kind, until, value });
snap_struct!(Path { points, len, next });
snap_struct!(Unit {
    alive,
    kind,
    team,
    generation,
    pos,
    moved,
    facing,
    radius,
    hp,
    mana,
    shield,
    shield_until,
    stats,
    cc,
    buffs,
    target,
    attack_ready,
    windup,
    path,
    repath_at,
    last_hit_by,
    last_hit_at,
    hurt_by,
    hit_champion_at,
    streak,
    revealed,
    spawned,
});
snap_struct!(Missile {
    alive,
    generation,
    team,
    owner,
    source,
    rank,
    homing,
    target,
    dir,
    range,
    width,
    pierce,
    origin,
    pos,
    speed,
    t0,
    damage,
    hits,
});
snap_struct!(Zone {
    alive,
    generation,
    team,
    owner,
    source,
    rank,
    pos,
    radius,
    filter,
    follows,
    next,
    end,
    period
});
snap_struct!(StatMods {
    hp,
    mana,
    ad,
    ap,
    armor,
    mr,
    attack_speed,
    move_speed,
    move_share,
    haste,
    lifesteal,
    hp_regen,
});
snap_struct!(Screen { open, tier, offers, rerolled });
snap_struct!(Casting { slot, left, at, target, rank });
snap_struct!(Dash { active, to, target, speed, radius, source, rank, until });
snap_struct!(Champ {
    def,
    level,
    xp,
    gold,
    ranks,
    points,
    ready,
    items,
    augments,
    n_aug,
    screen,
    screens_earned,
    screens_done,
    aug_ready,
    auto_hits,
    intent,
    cast,
    dash,
    steer,
    dead_until,
    kills,
    deaths,
    assists,
    last_hits,
    immobile,
    immobile_n,
    ping_at,
    mods,
});
snap_struct!(Relic { pos, up, next, beam_at });
snap_struct!(Vote { until, yes, no, next });

/// An enum, as a tag byte and its widest variant's fields, padded: compared by its bytes.
macro_rules! snap_enum_bytes {
    ($t:ty, $n:expr) => {
        impl Snap for $t {
            fn put(&self, o: &mut Out<'_>) {
                o.bytes(&self.encode());
            }
            fn get(&mut self, i: &mut In<'_>) -> Result<(), SnapError> {
                *self = <$t>::decode(i.take::<$n>()?)?;
                Ok(())
            }
            fn differs(&self, other: &Self, _: &mut Field) -> bool {
                self.encode() != other.encode()
            }
        }
    };
}

impl Kind {
    fn encode(&self) -> [u8; 1] {
        [*self as u8]
    }
    fn decode(b: [u8; 1]) -> Result<Self, SnapError> {
        let k = Kind::from_u8(b[0]);
        if k as u8 == b[0] { Ok(k) } else { Err(SnapError::Bad) }
    }
}
snap_enum_bytes!(Kind, 1);

impl Filter {
    fn encode(&self) -> [u8; 1] {
        [match self {
            Filter::Enemies => 0,
            Filter::Allies => 1,
        }]
    }
    fn decode(b: [u8; 1]) -> Result<Self, SnapError> {
        match b[0] {
            0 => Ok(Filter::Enemies),
            1 => Ok(Filter::Allies),
            _ => Err(SnapError::Bad),
        }
    }
}
snap_enum_bytes!(Filter, 1);

impl Source {
    fn encode(&self) -> [u8; 3] {
        match *self {
            Source::Auto => [0, 0, 0],
            Source::Turret => [1, 0, 0],
            Source::Fountain => [2, 0, 0],
            Source::Ability { champ, slot } => [3, champ, slot],
            Source::Relic => [4, 0, 0],
            Source::Augment(a) => [5, a, 0],
        }
    }
    fn decode(b: [u8; 3]) -> Result<Self, SnapError> {
        Ok(match b[0] {
            0 => Source::Auto,
            1 => Source::Turret,
            2 => Source::Fountain,
            3 => Source::Ability { champ: b[1], slot: b[2] },
            4 => Source::Relic,
            5 => Source::Augment(b[1]),
            _ => return Err(SnapError::Bad),
        })
    }
}
snap_enum_bytes!(Source, 3);

impl Intent {
    fn encode(&self) -> [u8; 9] {
        let mut b = [0u8; 9];
        let (tag, body): (u8, [u8; 8]) = match *self {
            Intent::Idle => (0, [0; 8]),
            Intent::Move(p) => (1, vec2_bytes(p)),
            Intent::AttackMove(p) => (2, vec2_bytes(p)),
            Intent::Attack(h) => (3, handle_bytes(h, 0)),
            Intent::Hold => (4, [0; 8]),
            Intent::CastAt { slot, target } => (5, handle_bytes(target, slot)),
        };
        b[0] = tag;
        b[1..].copy_from_slice(&body);
        b
    }
    fn decode(b: [u8; 9]) -> Result<Self, SnapError> {
        let p = Vec2::new(
            f32::from_bits(u32::from_le_bytes([b[1], b[2], b[3], b[4]])),
            f32::from_bits(u32::from_le_bytes([b[5], b[6], b[7], b[8]])),
        );
        let h =
            Handle { idx: u16::from_le_bytes([b[1], b[2]]), generation: u16::from_le_bytes([b[3], b[4]]) };
        Ok(match b[0] {
            0 => Intent::Idle,
            1 => Intent::Move(p),
            2 => Intent::AttackMove(p),
            3 => Intent::Attack(h),
            4 => Intent::Hold,
            5 => Intent::CastAt { slot: b[5], target: h },
            _ => return Err(SnapError::Bad),
        })
    }
}
snap_enum_bytes!(Intent, 9);

fn vec2_bytes(p: Vec2) -> [u8; 8] {
    let mut b = [0u8; 8];
    b[..4].copy_from_slice(&p.x.to_bits().to_le_bytes());
    b[4..].copy_from_slice(&p.y.to_bits().to_le_bytes());
    b
}

fn handle_bytes(h: Handle, extra: u8) -> [u8; 8] {
    let mut b = [0u8; 8];
    b[..2].copy_from_slice(&h.idx.to_le_bytes());
    b[2..4].copy_from_slice(&h.generation.to_le_bytes());
    b[4] = extra;
    b
}

impl Snap for Option<u8> {
    fn put(&self, o: &mut Out<'_>) {
        o.bytes(&match *self {
            None => [0, 0],
            Some(v) => [1, v],
        });
    }
    fn get(&mut self, i: &mut In<'_>) -> Result<(), SnapError> {
        *self = match i.take::<2>()? {
            [0, _] => None,
            [1, v] => Some(v),
            _ => return Err(SnapError::Bad),
        };
        Ok(())
    }
    fn differs(&self, other: &Self, _: &mut Field) -> bool {
        self != other
    }
}

impl Snap for Rng {
    fn put(&self, o: &mut Out<'_>) {
        self.state().put(o);
    }
    fn get(&mut self, i: &mut In<'_>) -> Result<(), SnapError> {
        let mut s = 0u64;
        s.get(i)?;
        *self = Rng::from_state(s);
        Ok(())
    }
    fn differs(&self, other: &Self, _: &mut Field) -> bool {
        self != other
    }
}

impl Snap for BitSet {
    fn put(&self, o: &mut Out<'_>) {
        for w in self.words() {
            w.put(o);
        }
    }
    fn get(&mut self, i: &mut In<'_>) -> Result<(), SnapError> {
        for w in self.words_mut() {
            w.get(i)?;
        }
        Ok(())
    }
    fn differs(&self, other: &Self, _: &mut Field) -> bool {
        self.words() != other.words()
    }
}

impl Snap for FreeList {
    fn put(&self, o: &mut Out<'_>) {
        let (stack, len) = self.parts();
        (len as u32).put(o);
        for s in stack {
            s.put(o);
        }
    }
    fn get(&mut self, i: &mut In<'_>) -> Result<(), SnapError> {
        let mut len = 0u32;
        len.get(i)?;
        let (stack, n) = self.parts_mut();
        if len as usize > stack.len() {
            return Err(SnapError::Bad);
        }
        *n = len as usize;
        for s in stack {
            s.get(i)?;
        }
        Ok(())
    }
    fn differs(&self, other: &Self, _: &mut Field) -> bool {
        self.parts() != other.parts()
    }
}

snap_struct!(Vision { grid, units, missiles });

/// A pool of slots: each slot in turn.
fn put_slots<T: Snap>(slots: &[T], o: &mut Out<'_>) {
    for s in slots {
        s.put(o);
    }
}

fn get_slots<T: Snap>(slots: &mut [T], i: &mut In<'_>) -> Result<(), SnapError> {
    for s in slots {
        s.get(i)?;
    }
    Ok(())
}

fn head(m: &Match, o: &mut Out<'_>) {
    o.bytes(&MAGIC);
    VERSION.put(o);
    CONTENT_VERSION.put(o);
    m.tick.put(o);
    (m.units.len() as u16).put(o);
    (m.missiles.len() as u16).put(o);
    (m.zones.len() as u16).put(o);
}

fn body(m: &Match, o: &mut Out<'_>) {
    let Match {
        map: _,
        seed,
        tick,
        rng,
        units,
        champs,
        missiles,
        zones,
        minion_slots,
        next_wave,
        waves,
        relics,
        inhib_back,
        vision,
        aug_tiers,
        headliner,
        offer_secret: _,
        offer_keys,
        offer_known,
        keys_drawn: _,
        keys_drawn_n: _,
        events: _,
        events_dropped,
        outcome,
        seated,
        surrender,
        path_budget: _,
    } = m;
    seed.put(o);
    tick.put(o);
    rng.put(o);
    put_slots(units, o);
    champs.put(o);
    put_slots(missiles, o);
    put_slots(zones, o);
    minion_slots.put(o);
    next_wave.put(o);
    waves.put(o);
    relics.put(o);
    inhib_back.put(o);
    vision.put(o);
    aug_tiers.put(o);
    headliner.put(o);
    offer_keys.put(o);
    offer_known.put(o);
    events_dropped.put(o);
    outcome.put(o);
    seated.put(o);
    surrender.put(o);
}

/// The bytes a checkpoint of `m` takes: the same for every match on its map.
pub fn len(m: &Match) -> usize {
    let mut o = Out { buf: &mut [], at: 0 };
    head(m, &mut o);
    body(m, &mut o);
    o.at + TAIL
}

/// Writes `m` into `out`; returns the bytes written, or 0 if `out` is shorter than [`len`].
pub fn write(m: &Match, out: &mut [u8]) -> usize {
    let mut o = Out { buf: out, at: 0 };
    head(m, &mut o);
    body(m, &mut o);
    if !o.fits() || o.at + TAIL > o.buf.len() {
        return 0;
    }
    let n = o.at;
    let sum = digest(&o.buf[..n]);
    o.buf[n..n + TAIL].copy_from_slice(&sum.to_le_bytes());
    n + TAIL
}

fn digest(b: &[u8]) -> u64 {
    let mut h = Fnv::default();
    let (words, rest) = b.as_chunks::<4>();
    for w in words {
        h.u32(u32::from_le_bytes(*w));
    }
    for &r in rest {
        h.u32(u32::from(r));
    }
    h.0
}

/// The tick a checkpoint was taken at, if `b` looks like one (unchecked beyond its head).
pub fn tick_of(b: &[u8]) -> Option<u32> {
    (b.len() >= HEAD && b[..4] == MAGIC).then(|| u32::from_le_bytes([b[10], b[11], b[12], b[13]]))
}

/// Checks a checkpoint's head, shape and checksum against `m`'s, without reading it in.
pub fn check(b: &[u8], m: &Match) -> Result<(), SnapError> {
    if b.len() < HEAD + TAIL {
        return Err(SnapError::Short);
    }
    if b[..4] != MAGIC {
        return Err(SnapError::Magic);
    }
    let mut i = In { buf: b, at: 4 };
    let (mut snap, mut content) = (0u16, 0u32);
    snap.get(&mut i)?;
    content.get(&mut i)?;
    if snap != VERSION || content != CONTENT_VERSION {
        return Err(SnapError::Version { snap, content });
    }
    let (mut tick, mut nu, mut nm, mut nz) = (0u32, 0u16, 0u16, 0u16);
    tick.get(&mut i)?;
    nu.get(&mut i)?;
    nm.get(&mut i)?;
    nz.get(&mut i)?;
    if usize::from(nu) != m.units.len()
        || usize::from(nm) != m.missiles.len()
        || usize::from(nz) != m.zones.len()
    {
        return Err(SnapError::Shape);
    }
    if b.len() != len(m) {
        return Err(SnapError::Shape);
    }
    let n = b.len() - TAIL;
    let mut sum = [0u8; 8];
    sum.copy_from_slice(&b[n..]);
    if digest(&b[..n]) != u64::from_le_bytes(sum) {
        return Err(SnapError::Checksum);
    }
    Ok(())
}

/// Makes `m` the match in checkpoint `b`, in place and without allocating. `m` must be a match on
/// the same map (its pools the same sizes). Its offer secret is cleared (give it one with
/// `set_offer_secret` to go on drawing keys) and its events dropped. The head, shape and checksum
/// are checked before anything is written, so a damaged or foreign checkpoint leaves `m` as it was.
pub fn read(b: &[u8], m: &mut Match) -> Result<(), SnapError> {
    check(b, m)?;
    let mut i = In { buf: &b[..b.len() - TAIL], at: HEAD };
    let Match {
        map: _,
        seed,
        tick,
        rng,
        units,
        champs,
        missiles,
        zones,
        minion_slots,
        next_wave,
        waves,
        relics,
        inhib_back,
        vision,
        aug_tiers,
        headliner,
        offer_secret,
        offer_keys,
        offer_known,
        keys_drawn: _,
        keys_drawn_n,
        events,
        events_dropped,
        outcome,
        seated,
        surrender,
        path_budget,
    } = m;
    seed.get(&mut i)?;
    tick.get(&mut i)?;
    rng.get(&mut i)?;
    get_slots(units, &mut i)?;
    champs.get(&mut i)?;
    get_slots(missiles, &mut i)?;
    get_slots(zones, &mut i)?;
    minion_slots.get(&mut i)?;
    next_wave.get(&mut i)?;
    waves.get(&mut i)?;
    relics.get(&mut i)?;
    inhib_back.get(&mut i)?;
    vision.get(&mut i)?;
    aug_tiers.get(&mut i)?;
    headliner.get(&mut i)?;
    offer_keys.get(&mut i)?;
    offer_known.get(&mut i)?;
    events_dropped.get(&mut i)?;
    outcome.get(&mut i)?;
    seated.get(&mut i)?;
    surrender.get(&mut i)?;
    *offer_secret = None;
    *keys_drawn_n = 0;
    events.clear();
    *path_budget = PATH_BUDGET;
    Ok(())
}

/// Where two matches on the same map first differ, as a checkpoint sees them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Where {
    /// `match`, `unit`, `champ`, `missile`, `zone`, `relic`, `vote`.
    pub part: &'static str,
    /// The slot or seat, where the part has them.
    pub slot: Option<u16>,
    pub field: Field,
}

impl fmt::Display for Where {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.part)?;
        if let Some(s) = self.slot {
            write!(f, " {s}")?;
        }
        if self.field.n > 0 {
            write!(f, ": {}", self.field)?;
        }
        Ok(())
    }
}

/// The first difference between `a` and `b` that a checkpoint would carry, or `None` if their
/// checkpoints would be the same bytes. What the referee names when a match goes astray.
pub fn diff(a: &Match, b: &Match) -> Option<Where> {
    fn one<T: Snap>(
        part: &'static str,
        slot: Option<u16>,
        name: &'static str,
        x: &T,
        y: &T,
    ) -> Option<Where> {
        let mut field = Field::default();
        if !x.differs(y, &mut field) {
            return None;
        }
        if !name.is_empty() {
            field.push(name);
        }
        Some(Where { part, slot, field })
    }
    fn slots<T: Snap>(part: &'static str, xs: &[T], ys: &[T]) -> Option<Where> {
        xs.iter().zip(ys).enumerate().find_map(|(k, (x, y))| one(part, Some(k as u16), "", x, y))
    }
    let Match {
        map: _,
        seed,
        tick,
        rng,
        units,
        champs,
        missiles,
        zones,
        minion_slots,
        next_wave,
        waves,
        relics,
        inhib_back,
        vision,
        aug_tiers,
        headliner,
        offer_secret: _,
        offer_keys,
        offer_known,
        keys_drawn: _,
        keys_drawn_n: _,
        events: _,
        events_dropped,
        outcome,
        seated,
        surrender,
        path_budget: _,
    } = a;
    let m = "match";
    one(m, None, "seed", seed, &b.seed)
        .or_else(|| one(m, None, "tick", tick, &b.tick))
        .or_else(|| one(m, None, "rng", rng, &b.rng))
        .or_else(|| slots("unit", units, &b.units))
        .or_else(|| slots("champ", champs, &b.champs))
        .or_else(|| slots("missile", missiles, &b.missiles))
        .or_else(|| slots("zone", zones, &b.zones))
        .or_else(|| one(m, None, "minion_slots", minion_slots, &b.minion_slots))
        .or_else(|| one(m, None, "next_wave", next_wave, &b.next_wave))
        .or_else(|| one(m, None, "waves", waves, &b.waves))
        .or_else(|| slots("relic", relics, &b.relics))
        .or_else(|| one(m, None, "inhib_back", inhib_back, &b.inhib_back))
        .or_else(|| one(m, None, "vision", vision, &b.vision))
        .or_else(|| one(m, None, "aug_tiers", aug_tiers, &b.aug_tiers))
        .or_else(|| one(m, None, "headliner", headliner, &b.headliner))
        .or_else(|| one(m, None, "offer_keys", offer_keys, &b.offer_keys))
        .or_else(|| one(m, None, "offer_known", offer_known, &b.offer_known))
        .or_else(|| one(m, None, "events_dropped", events_dropped, &b.events_dropped))
        .or_else(|| one(m, None, "outcome", outcome, &b.outcome))
        .or_else(|| one(m, None, "seated", seated, &b.seated))
        .or_else(|| slots("vote", surrender, &b.surrender))
}
