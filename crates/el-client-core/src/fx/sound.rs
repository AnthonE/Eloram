//! Every sound the game plays, as a table (`docs/FX.md`, "Sound"): its name, its bus, how many
//! takes it has (one is drawn each time it plays), how many may sound at once, its level, and what
//! it is, in words. The table is the contract: `art/tools/sounds.py` makes a file for every take
//! of every sound that isn't optional, from `web/assets/audio/manifest.json`, which a test writes
//! from this table and checks is current.
//!
//! A take is `web/assets/audio/<name>.<ext>` for a sound of one take, else `<name>.<k>.<ext>`, `k`
//! from 1; each in Opus (`.ogg`) and MP3 (`.mp3`, for a browser without Opus). The announcer's
//! lines and the music are optional: a file that isn't there is a sound that doesn't play.

/// Where a sound goes: each bus has its own volume on the page, under the master.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bus {
    /// The match: abilities, attacks, statuses, structures.
    Effects,
    /// The announcer.
    Voice,
    /// The champions' own voices: their lines as they move, fight, cast their ultimates, kill and
    /// fall ([`super::vo`]).
    Champions,
    /// The interface: the shop, the cards, pings.
    Ui,
    /// The Causeway itself: the wind, the braziers.
    Ambience,
    Music,
}

impl Bus {
    /// The buses, in the menu's order (the settings keep each by its name).
    pub const ALL: [Bus; 6] = [Bus::Effects, Bus::Champions, Bus::Voice, Bus::Ui, Bus::Ambience, Bus::Music];

    /// Its place in [`Bus::ALL`].
    pub fn index(self) -> usize {
        Bus::ALL.iter().position(|&b| b == self).unwrap_or(0)
    }

    pub fn name(self) -> &'static str {
        match self {
            Bus::Effects => "effects",
            Bus::Voice => "voice",
            Bus::Champions => "champions",
            Bus::Ui => "ui",
            Bus::Ambience => "ambience",
            Bus::Music => "music",
        }
    }
}

/// A sound's line in the table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundDef {
    pub name: &'static str,
    pub bus: Bus,
    pub takes: u8,
    /// How many of it may sound at once (past it, the oldest is cut).
    pub cap: u8,
    /// Its level against its bus's, in decibels.
    pub gain_db: f32,
    /// Stereo (beds and music); else mono, panned where it happens.
    pub stereo: bool,
    /// It loops.
    pub looped: bool,
    /// Not `sounds.py`'s to make: the music, which comes as files when it comes, and the
    /// announcer's lines, which `voices.py` makes as placeholders until recordings replace them.
    pub optional: bool,
    /// What it is: what the sound tool makes from.
    pub what: &'static str,
}

/// An ability's part, a family's sound each (a family is the champions who share a palette:
/// [`super::look::Family`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The cast begins (the windup).
    Cast,
    /// A missile leaves, or a ranged auto.
    Launch,
    /// It strikes a unit.
    Impact,
    /// An area lands, or a field pulses.
    Blast,
    /// The warning before an area lands: what an enemy hears to dodge by.
    Windup,
    /// The caster dashes or blinks.
    Dash,
    /// A buff, a heal or a shield given.
    Buff,
    /// An ultimate's cast, over its cast.
    Ult,
}

impl Part {
    pub const COUNT: usize = 8;
}

macro_rules! sounds {
    ($($id:ident = $name:literal, $bus:ident, $takes:literal, $cap:literal, $db:literal, $flags:literal, $what:literal;)*) => {
        /// A sound, by its line in [`SOUNDS`].
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(u16)]
        pub enum Sfx {
            $($id),*
        }

        impl Sfx {
            /// Every sound, in the table's order.
            pub const ALL: &'static [Sfx] = &[$(Sfx::$id),*];
        }

        /// The table.
        pub static SOUNDS: &[SoundDef] = &[$(SoundDef {
            name: $name,
            bus: Bus::$bus,
            takes: $takes,
            cap: $cap,
            gain_db: $db,
            stereo: flag($flags, 's'),
            looped: flag($flags, 'l'),
            optional: flag($flags, 'o'),
            what: $what,
        }),*];
    };
}

/// Whether a line's flags (`"s"` stereo, `"l"` looped, `"o"` optional) hold `c`.
const fn flag(flags: &str, c: char) -> bool {
    let b = flags.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == c as u8 {
            return true;
        }
        i += 1;
    }
    false
}

// A family's eight parts, in `Part`'s order, then the families in `Family`'s order (what
// `Sfx::family` counts on).
sounds! {
    EmberCast = "ember.cast", Effects, 2, 4, -2.0, "", "fire drawn up: a rising whoosh of flame with crackle, 0.35 s";
    EmberLaunch = "ember.launch", Effects, 3, 6, -3.0, "", "a fireball thrown: a short roar and hiss leaving, 0.4 s";
    EmberImpact = "ember.impact", Effects, 3, 6, -2.0, "", "fire bursting on a body: a thump and a crackling flare, 0.5 s";
    EmberBlast = "ember.blast", Effects, 2, 4, 0.0, "", "an explosion of embers: a deep whump, roar and falling sparks, 0.9 s";
    EmberWindup = "ember.windup", Effects, 1, 3, 0.0, "", "fire gathering to fall: a rising roar with a bright edge, 0.8 s";
    EmberDash = "ember.dash", Effects, 2, 3, -3.0, "", "a step through flame: a quick whoosh and a puff of fire, 0.35 s";
    EmberBuff = "ember.buff", Effects, 2, 3, -5.0, "", "a warm kindling: a soft flare and crackle that settles, 0.6 s";
    EmberUlt = "ember.ult", Effects, 1, 2, 1.0, "", "a great fire called: a swelling roar under a ringing tone, 1.4 s";
    FrostCast = "frost.cast", Effects, 2, 4, -2.0, "", "cold drawn in: a breathy shimmer and glassy tinkle rising, 0.35 s";
    FrostLaunch = "frost.launch", Effects, 3, 6, -3.0, "", "an ice shard thrown: a crystalline whistle and whoosh, 0.35 s";
    FrostImpact = "frost.impact", Effects, 3, 6, -2.0, "", "ice shattering on a body: a crack and a spray of glass chimes, 0.5 s";
    FrostBlast = "frost.blast", Effects, 2, 4, 0.0, "", "a field of rime bursting: a deep crunch and ringing shards, 0.9 s";
    FrostWindup = "frost.windup", Effects, 1, 3, 0.0, "", "frost creeping: a rising glassy drone with crackling, 0.8 s";
    FrostDash = "frost.dash", Effects, 2, 3, -3.0, "", "a glide on ice: a quick airy swish with a chime, 0.35 s";
    FrostBuff = "frost.buff", Effects, 2, 3, -5.0, "", "a cold glow: a soft bell-like shimmer, 0.6 s";
    FrostUlt = "frost.ult", Effects, 1, 2, 1.0, "", "a whiteout: a howling gust, cracking ice and a low glass drone, 1.4 s";
    TideCast = "tide.cast", Effects, 2, 4, -2.0, "", "water drawn up: a rising slosh and swell, 0.35 s";
    TideLaunch = "tide.launch", Effects, 3, 6, -3.0, "", "a wave thrown: a rushing splash leaving, 0.4 s";
    TideImpact = "tide.impact", Effects, 3, 6, -2.0, "", "water crashing on a body: a heavy splash and spray, 0.5 s";
    TideBlast = "tide.blast", Effects, 2, 4, 0.0, "", "a surge breaking: a deep crash of water and foam, 0.9 s";
    TideWindup = "tide.windup", Effects, 1, 3, 0.0, "", "an undertow gathering: a swelling low rush, 0.8 s";
    TideDash = "tide.dash", Effects, 2, 3, -3.0, "", "riding a breaker: a quick rushing splash, 0.35 s";
    TideBuff = "tide.buff", Effects, 2, 3, -5.0, "", "a calm swell: a soft wash and bubble, 0.6 s";
    TideUlt = "tide.ult", Effects, 1, 2, 1.0, "", "a maelstrom: a roaring whirl of water rising, 1.4 s";
    WindCast = "wind.cast", Effects, 2, 4, -2.0, "", "air drawn in: a rising airy whoosh, 0.35 s";
    WindLaunch = "wind.launch", Effects, 3, 6, -3.0, "", "a gale cut: a sharp whistling swish leaving, 0.35 s";
    WindImpact = "wind.impact", Effects, 3, 6, -2.0, "", "a blade of wind landing: a cutting swish and thud, 0.45 s";
    WindBlast = "wind.blast", Effects, 2, 4, 0.0, "", "a burst of wind: a deep gust and swirl, 0.9 s";
    WindWindup = "wind.windup", Effects, 1, 3, 0.0, "", "a gust gathering: a rising howl, 0.8 s";
    WindDash = "wind.dash", Effects, 2, 3, -3.0, "", "a glide: a quick flutter and whoosh, 0.35 s";
    WindBuff = "wind.buff", Effects, 2, 3, -5.0, "", "an updraft: a soft rising breeze and flutter, 0.6 s";
    WindUlt = "wind.ult", Effects, 1, 2, 1.0, "", "a tempest: a roaring spiral of wind with a whistle, 1.4 s";
    DuskCast = "dusk.cast", Effects, 2, 4, -2.0, "", "starlight drawn in: a rising shimmer over a low hum, 0.35 s";
    DuskLaunch = "dusk.launch", Effects, 3, 6, -3.0, "", "a dark bolt thrown: a phasey whoosh with a sparkle, 0.4 s";
    DuskImpact = "dusk.impact", Effects, 3, 6, -2.0, "", "a hex striking: a hollow thump and a dark shimmer, 0.5 s";
    DuskBlast = "dusk.blast", Effects, 2, 4, 0.0, "", "a well of night collapsing: a deep boom and swirling shimmer, 0.9 s";
    DuskWindup = "dusk.windup", Effects, 1, 3, 0.0, "", "the sky about to fall: a rising whine of starlight, 0.8 s";
    DuskDash = "dusk.dash", Effects, 2, 3, -3.0, "", "a shade step: a breathy swoosh and a soft pop, 0.35 s";
    DuskBuff = "dusk.buff", Effects, 2, 3, -5.0, "", "a hush: a soft dark shimmer, 0.6 s";
    DuskUlt = "dusk.ult", Effects, 1, 2, 1.0, "", "a dark hour: a falling star's roar into a deep boom and shimmer, 1.4 s";
    GrowthCast = "growth.cast", Effects, 2, 4, -2.0, "", "sap rising: a woody creak and rustle, 0.35 s";
    GrowthLaunch = "growth.launch", Effects, 3, 6, -3.0, "", "a seed thrown: a soft whoosh with a leafy flutter, 0.35 s";
    GrowthImpact = "growth.impact", Effects, 3, 6, -2.0, "", "thorns striking: a woody crack and rustle, 0.45 s";
    GrowthBlast = "growth.blast", Effects, 2, 4, 0.0, "", "a bloom bursting: a soft thump, rustle and rising chimes, 0.9 s";
    GrowthWindup = "growth.windup", Effects, 1, 3, 0.0, "", "roots stirring: a creaking rising groan, 0.8 s";
    GrowthDash = "growth.dash", Effects, 2, 3, -3.0, "", "a step through leaves: a quick rustling swish, 0.35 s";
    GrowthBuff = "growth.buff", Effects, 2, 3, -5.0, "", "a healing bloom: soft bright chimes and a breath, 0.6 s";
    GrowthUlt = "growth.ult", Effects, 1, 2, 1.0, "", "a verdance: a swelling chord of chimes over rustling, 1.4 s";
    StoneCast = "stone.cast", Effects, 2, 4, -2.0, "", "weight gathered: a low grinding rumble, 0.35 s";
    StoneLaunch = "stone.launch", Effects, 3, 6, -3.0, "", "a shell or stone hurled: a heavy thud and whoosh, 0.4 s";
    StoneImpact = "stone.impact", Effects, 3, 6, -2.0, "", "rock striking a body: a heavy crunch and thud, 0.5 s";
    StoneBlast = "stone.blast", Effects, 2, 4, 0.0, "", "the ground breaking: a deep boom with falling rubble, 0.9 s";
    StoneWindup = "stone.windup", Effects, 1, 3, 0.0, "", "a shell falling: a descending whistle into a rumble, 0.8 s";
    StoneDash = "stone.dash", Effects, 2, 3, -3.0, "", "a charge: heavy pounding steps and a grind, 0.35 s";
    StoneBuff = "stone.buff", Effects, 2, 3, -5.0, "", "a stone skin: a low crackling grind settling, 0.6 s";
    StoneUlt = "stone.ult", Effects, 1, 2, 1.0, "", "an upheaval: a huge rumble and crack of breaking ground, 1.4 s";
    BronzeCast = "bronze.cast", Effects, 2, 4, -2.0, "", "light gathered: a rising warm chime, 0.35 s";
    BronzeLaunch = "bronze.launch", Effects, 3, 6, -3.0, "", "a bolt of light thrown: a bright ringing whoosh, 0.4 s";
    BronzeImpact = "bronze.impact", Effects, 3, 6, -2.0, "", "a bell's toll striking: a metallic clang and ring, 0.6 s";
    BronzeBlast = "bronze.blast", Effects, 2, 4, 0.0, "", "a peal: a great bell struck once, ringing out, 1.0 s";
    BronzeWindup = "bronze.windup", Effects, 1, 3, 0.0, "", "light about to fall: a rising shimmering chord, 0.8 s";
    BronzeDash = "bronze.dash", Effects, 2, 3, -3.0, "", "a spur: a quick ringing swish and hoofbeat, 0.35 s";
    BronzeBuff = "bronze.buff", Effects, 2, 3, -5.0, "", "an aegis: a soft bright chime that hangs, 0.6 s";
    BronzeUlt = "bronze.ult", Effects, 1, 2, 1.0, "", "a sanctuary or a banner: a deep bell under a rising choir-like chord, 1.4 s";
    SteelCast = "steel.cast", Effects, 2, 4, -2.0, "", "a weapon drawn: a metallic scrape and click, 0.3 s";
    SteelLaunch = "steel.launch", Effects, 3, 6, -3.0, "", "a bolt loosed or a shot fired: a twang or crack and a whiz, 0.35 s";
    SteelImpact = "steel.impact", Effects, 3, 6, -2.0, "", "steel striking a body: a slicing hit with a ring, 0.45 s";
    SteelBlast = "steel.blast", Effects, 2, 4, 0.0, "", "a salvo landing: rapid cracks and thuds, 0.9 s";
    SteelWindup = "steel.windup", Effects, 1, 3, 0.0, "", "a long shot drawn: a rising metallic whine, 0.8 s";
    SteelDash = "steel.dash", Effects, 2, 3, -3.0, "", "a roll or a close-in: a quick cloth swish and footfall, 0.35 s";
    SteelBuff = "steel.buff", Effects, 2, 3, -5.0, "", "a war cry: a short metallic ring and grunt-like swell, 0.6 s";
    SteelUlt = "steel.ult", Effects, 1, 2, 1.0, "", "a verdict or an execution: a long ringing blade and heavy hit, 1.4 s";

    AutoSwing = "auto.swing", Effects, 3, 6, -8.0, "", "a melee swing: a short weapon whoosh, 0.2 s";
    AutoHit = "auto.hit", Effects, 4, 6, -6.0, "", "a melee hit landing: a punchy thud with a little metal, 0.25 s";
    MinionSwing = "minion.swing", Effects, 3, 4, -16.0, "", "a small creature's swing: a light whoosh, 0.15 s";
    MinionHit = "minion.hit", Effects, 3, 4, -15.0, "", "a small hit: a light thud, 0.15 s";
    MinionShot = "minion.shot", Effects, 3, 4, -16.0, "", "a small magic bolt: a soft zap, 0.2 s";
    MinionSiege = "minion.siege", Effects, 2, 3, -11.0, "", "a small cannon: a muffled boom, 0.4 s";
    MinionDie = "minion.die", Effects, 3, 4, -14.0, "", "a small creature falling: a short crumble and puff, 0.3 s";
    TurretShot = "turret.shot", Effects, 2, 3, -4.0, "", "a turret's bolt of crystal fire: a charged zap and whoosh, 0.5 s";
    TurretHit = "turret.hit", Effects, 2, 3, -3.0, "", "a turret's bolt striking: a heavy crackling thump, 0.5 s";
    TurretWarn = "turret.warn", Effects, 1, 1, 0.0, "", "a turret turning on you: a low rising warning tone, 0.6 s";
    TurretFall = "turret.fall", Effects, 1, 2, 2.0, "", "a turret destroyed: a crack, a roar of falling stone, 1.8 s";
    InhibitorFall = "inhibitor.fall", Effects, 1, 2, 2.0, "", "an inhibitor's crystal breaking: a ringing shatter and boom, 1.8 s";
    NexusFall = "nexus.fall", Effects, 1, 1, 4.0, "", "the nexus destroyed: a huge shatter and a long falling roar, 3.0 s";
    StatusStun = "status.stun", Effects, 2, 3, -3.0, "", "stunned: a ringing daze, a tinny wobble, 0.6 s";
    StatusRoot = "status.root", Effects, 2, 3, -3.0, "", "rooted: a quick snare of creaking binding, 0.5 s";
    StatusSilence = "status.silence", Effects, 1, 3, -4.0, "", "silenced: a muffled hush and a soft pop, 0.5 s";
    StatusSlow = "status.slow", Effects, 2, 3, -8.0, "", "slowed: a dragging downward smear, 0.4 s";
    StatusAirborne = "status.airborne", Effects, 2, 3, -2.0, "", "thrown into the air: a rising whoosh and a thud, 0.6 s";
    StatusShove = "status.shove", Effects, 2, 3, -4.0, "", "knocked back or pulled: a heavy shove and scrape, 0.4 s";
    StatusReveal = "status.reveal", Effects, 1, 2, -8.0, "", "revealed: a short bright ping, 0.4 s";
    StatusImmune = "status.immune", Effects, 1, 2, -3.0, "", "cleansed and unstoppable: a bright rising ring, 0.6 s";
    ShieldUp = "shield.up", Effects, 2, 3, -6.0, "", "a shield raised: a soft humming swell, 0.5 s";
    Heal = "heal", Effects, 2, 3, -8.0, "", "healed: a soft bright rising chime, 0.5 s";
    Empowered = "empowered", Effects, 1, 3, -8.0, "", "a buff to strength: a short warm surge, 0.4 s";
    Hasted = "hasted", Effects, 1, 3, -9.0, "", "a buff to speed: a short airy lift, 0.35 s";
    Blink = "blink", Effects, 2, 3, -3.0, "", "Blink, there at once: a sharp zip and a pop, 0.3 s";
    TagThrow = "tag.throw", Effects, 1, 3, -6.0, "", "a snowball thrown: a soft whoosh, 0.3 s";
    TagHit = "tag.hit", Effects, 1, 3, -4.0, "", "a snowball's hit: a soft packed-snow thump, 0.3 s";
    TagDash = "tag.dash", Effects, 1, 3, -4.0, "", "the dash to a tag: a rushing swish, 0.4 s";
    DeathChampion = "death.champion", Effects, 2, 3, 0.0, "", "a champion falls: a heavy body-fall and a low swell, 1.0 s";
    DeathOwn = "death.own", Effects, 1, 1, 2.0, "", "you fall: a muffled heavy hit into a low drone, 1.6 s";
    Respawn = "respawn", Effects, 1, 2, -4.0, "", "back on the platform: a soft rising whoosh and chime, 0.8 s";
    LevelUp = "level.up", Effects, 1, 2, -3.0, "", "a level gained: a bright rising arpeggio, 0.8 s";
    RelicTake = "relic.take", Effects, 1, 2, -3.0, "", "a relic taken: a bright bubbling heal, 0.7 s";
    RelicBeam = "relic.beam", Effects, 1, 1, -4.0, "", "a relic's beam about to fall: a rising hum, 1.0 s";
    AugmentProc = "augment.proc", Effects, 2, 4, -9.0, "", "an augment answering: a small magical tick, 0.25 s";
    GoldCoin = "gold.coin", Ui, 3, 3, -7.0, "", "your last hit's gold: one bright coin's plink, 0.3 s";
    TakedownX1 = "takedown.x1", Ui, 1, 1, -3.0, "", "your kill: a punchy low hit, one bright note up and a few coins, 0.8 s";
    TakedownX2 = "takedown.x2", Ui, 1, 1, -2.5, "", "your second kill running: the hit, two notes up and more coins, 0.9 s";
    TakedownX3 = "takedown.x3", Ui, 1, 1, -2.0, "", "your third kill running: the hit, three notes up and a spill of coins, 1.0 s";
    TakedownX4 = "takedown.x4", Ui, 1, 1, -1.5, "", "your fourth kill running: a heavier hit, four notes up and a shower of coins, 1.1 s";
    TakedownX5 = "takedown.x5", Ui, 1, 1, -1.0, "", "all five by your hand: the heaviest hit, a run of five notes to a ringing chord and coins pouring, 1.4 s";
    TakedownAssist = "takedown.assist", Ui, 1, 1, -6.0, "", "your assist: a soft two-note chime and a coin or two, 0.6 s";

    UiClick = "ui.click", Ui, 1, 2, -6.0, "", "a button: a soft dry click";
    UiBuy = "ui.buy", Ui, 1, 2, -4.0, "", "an item bought: a coin clink";
    UiSell = "ui.sell", Ui, 1, 2, -6.0, "", "an item sold: two coins down";
    UiLevel = "ui.level", Ui, 1, 2, -5.0, "", "a skill point spent: a short bright tick up";
    UiCard = "ui.card", Ui, 1, 1, -4.0, "", "the augment cards dealt: three soft card flips";
    UiAugment = "ui.augment", Ui, 1, 1, -3.0, "", "an augment taken: a shimmering confirm";
    UiReroll = "ui.reroll", Ui, 1, 1, -5.0, "", "a reroll: a quick shuffle";
    UiError = "ui.error", Ui, 1, 1, -6.0, "", "not now: a short low double buzz";
    UiFound = "ui.found", Ui, 1, 1, -2.0, "", "a match found: a bright rising chime";
    UiReady = "ui.ready", Ui, 1, 1, -4.0, "", "ready: a firm confirming chime";
    UiTick = "ui.tick", Ui, 1, 1, -8.0, "", "a countdown's last seconds: a small clock tick";
    UiVote = "ui.vote", Ui, 1, 1, -4.0, "", "a vote called: a two-note horn";
    PingLook = "ping.look", Ui, 1, 2, -4.0, "", "a ping, look here: a short clear bell";
    PingDanger = "ping.danger", Ui, 1, 2, -3.0, "", "a ping, careful: a sharp two-tone warning";
    PingComing = "ping.coming", Ui, 1, 2, -4.0, "", "a ping, on my way: a quick rising run";
    PingGone = "ping.gone", Ui, 1, 2, -4.0, "", "a ping, one's missing: a questioning two-note fall";
    PingAttack = "ping.attack", Ui, 1, 2, -3.0, "", "a ping, go in: a bold low horn";
    PingBack = "ping.back", Ui, 1, 2, -3.0, "", "a ping, fall back: a falling three-note warning";

    AmbWind = "amb.wind", Ambience, 1, 1, 0.0, "sl", "the wind over the bridge at night, a seamless 24 s stereo loop";
    AmbFire = "amb.fire", Ambience, 1, 1, 0.0, "l", "a brazier's crackle, a seamless 8 s loop";

    VoiceWelcome = "voice.welcome", Voice, 1, 1, 0.0, "o", "the match begins: welcome to the Causeway";
    VoiceMinionsSoon = "voice.minions_soon", Voice, 1, 1, 0.0, "o", "thirty seconds until the minions march";
    VoiceMinions = "voice.minions", Voice, 1, 1, 0.0, "o", "the minions march";
    VoiceFirstBlood = "voice.first_blood", Voice, 1, 1, 0.0, "o", "the first kill of the match";
    VoiceYouSlew = "voice.you_slew", Voice, 1, 1, 0.0, "o", "you killed an enemy";
    VoiceYouFell = "voice.you_fell", Voice, 1, 1, 0.0, "o", "you were killed";
    VoiceAllyFell = "voice.ally_fell", Voice, 1, 1, 0.0, "o", "an ally was killed";
    VoiceEnemyFell = "voice.enemy_fell", Voice, 1, 1, 0.0, "o", "an enemy was killed";
    VoiceMulti2 = "voice.multi_2", Voice, 1, 1, 0.0, "o", "two kills in a row, quickly";
    VoiceMulti3 = "voice.multi_3", Voice, 1, 1, 0.0, "o", "three kills in a row";
    VoiceMulti4 = "voice.multi_4", Voice, 1, 1, 0.0, "o", "four kills in a row";
    VoiceMulti5 = "voice.multi_5", Voice, 1, 1, 0.0, "o", "all five, by one hand";
    VoiceAce = "voice.ace", Voice, 1, 1, 0.0, "o", "a whole team down";
    VoiceTurretDown = "voice.turret_down", Voice, 1, 1, 0.0, "o", "your team destroyed a turret";
    VoiceTurretLost = "voice.turret_lost", Voice, 1, 1, 0.0, "o", "your team lost a turret";
    VoiceInhibitorDown = "voice.inhibitor_down", Voice, 1, 1, 0.0, "o", "your team destroyed an inhibitor";
    VoiceInhibitorLost = "voice.inhibitor_lost", Voice, 1, 1, 0.0, "o", "your team lost an inhibitor";
    VoiceInhibitorBack = "voice.inhibitor_back", Voice, 1, 1, 0.0, "o", "your inhibitor stands again";
    VoiceVictory = "voice.victory", Voice, 1, 1, 0.0, "o", "victory";
    VoiceDefeat = "voice.defeat", Voice, 1, 1, 0.0, "o", "defeat";
    VoiceSurrender = "voice.surrender", Voice, 1, 1, 0.0, "o", "your team surrendered";
    VoiceEnemySurrender = "voice.enemy_surrender", Voice, 1, 1, 0.0, "o", "the enemy surrendered";
    VoiceEnemyMulti2 = "voice.enemy_multi_2", Voice, 1, 1, 0.0, "o", "an enemy's two kills in a row";
    VoiceEnemyMulti3 = "voice.enemy_multi_3", Voice, 1, 1, 0.0, "o", "an enemy's three kills in a row";
    VoiceEnemyMulti4 = "voice.enemy_multi_4", Voice, 1, 1, 0.0, "o", "an enemy's four kills in a row";
    VoiceEnemyMulti5 = "voice.enemy_multi_5", Voice, 1, 1, 0.0, "o", "all five of your team, by one enemy's hand";
    VoiceAced = "voice.aced", Voice, 1, 1, 0.0, "o", "your whole team down";
    VoiceSpree3 = "voice.spree_3", Voice, 1, 1, 0.0, "o", "one of yours: three kills without falling";
    VoiceSpree4 = "voice.spree_4", Voice, 1, 1, 0.0, "o", "one of yours: four kills without falling";
    VoiceSpree5 = "voice.spree_5", Voice, 1, 1, 0.0, "o", "one of yours: five kills without falling";
    VoiceSpree6 = "voice.spree_6", Voice, 1, 1, 0.0, "o", "one of yours: six kills without falling";
    VoiceSpree7 = "voice.spree_7", Voice, 1, 1, 0.0, "o", "one of yours: seven kills without falling";
    VoiceSpree8 = "voice.spree_8", Voice, 1, 1, 0.0, "o", "one of yours: eight kills and more without falling";
    VoiceEnemySpree3 = "voice.enemy_spree_3", Voice, 1, 1, 0.0, "o", "an enemy: three kills without falling";
    VoiceEnemySpree4 = "voice.enemy_spree_4", Voice, 1, 1, 0.0, "o", "an enemy: four kills without falling";
    VoiceEnemySpree5 = "voice.enemy_spree_5", Voice, 1, 1, 0.0, "o", "an enemy: five kills without falling";
    VoiceEnemySpree6 = "voice.enemy_spree_6", Voice, 1, 1, 0.0, "o", "an enemy: six kills without falling";
    VoiceEnemySpree7 = "voice.enemy_spree_7", Voice, 1, 1, 0.0, "o", "an enemy: seven kills without falling";
    VoiceEnemySpree8 = "voice.enemy_spree_8", Voice, 1, 1, 0.0, "o", "an enemy: eight kills and more without falling";
    VoiceShutdown = "voice.shutdown", Voice, 1, 1, 0.0, "o", "a champion's run of three or more ended";
    VoiceExecuted = "voice.executed", Voice, 1, 1, 0.0, "o", "a champion killed by no champion (a turret, minions, the platform)";
    VoiceInhibitorSoon = "voice.inhibitor_soon", Voice, 1, 1, 0.0, "o", "your inhibitor stands again in fifteen seconds";
    VoiceEnemyInhibitorSoon = "voice.enemy_inhibitor_soon", Voice, 1, 1, 0.0, "o", "the enemy's inhibitor stands again in fifteen seconds";
    VoiceEnemyInhibitorBack = "voice.enemy_inhibitor_back", Voice, 1, 1, 0.0, "o", "the enemy's inhibitor stands again";

    MusicSelect = "music.select", Music, 1, 1, 0.0, "slo", "champion select, looped";
    MusicMatch = "music.match", Music, 1, 1, -4.0, "slo", "the match, looped under it all";
    MusicVictory = "music.victory", Music, 1, 1, 0.0, "so", "a win";
    MusicDefeat = "music.defeat", Music, 1, 1, 0.0, "so", "a loss";
}

use el_sim::content::champions::CHAMPIONS;

use super::vo;

/// The first family sound in the table.
const FAMILIES_AT: usize = 0;

impl Sfx {
    pub fn def(self) -> &'static SoundDef {
        &SOUNDS[self as usize]
    }

    /// Family `family`'s sound for `part` (families are numbered as [`super::look::Family`]).
    pub fn family(family: usize, part: Part) -> Sfx {
        Sfx::ALL[FAMILIES_AT + family * Part::COUNT + part as usize]
    }

    /// Take `k`'s file (`k` from 0), without its extension, from the audio folder.
    pub fn file(self, k: u8) -> String {
        let d = self.def();
        if d.takes <= 1 { d.name.to_string() } else { format!("{}.{}", d.name, k + 1) }
    }

    /// A sound's line by its name.
    pub fn by_name(name: &str) -> Option<Sfx> {
        SOUNDS.iter().position(|d| d.name == name).map(|i| Sfx::ALL[i])
    }
}

/// The table as `web/assets/audio/manifest.json` has it, for the sound tool and the voices'.
pub fn manifest() -> String {
    let lines: Vec<serde_json::Value> = Sfx::ALL
        .iter()
        .map(|&s| {
            let d = s.def();
            let mut v = serde_json::json!({
                "name": d.name,
                "bus": d.bus.name(),
                "takes": d.takes,
                "files": (0..d.takes).map(|k| s.file(k)).collect::<Vec<_>>(),
                "gain_db": d.gain_db,
                "stereo": d.stereo,
                "loop": d.looped,
                "optional": d.optional,
                "what": d.what,
            });
            if let Some(t) = super::voice::says(s) {
                v["says"] = serde_json::json!(t);
            }
            v
        })
        .collect();
    let voices: Vec<serde_json::Value> = (0..CHAMPIONS.len() as u8)
        .flat_map(|c| {
            let sc = vo::script(c);
            vo::Line::ALL.iter().map(move |&line| {
                let takes = sc.takes(line);
                let r = vo::rule(line);
                serde_json::json!({
                    "name": format!("vo.{}.{}", CHAMPIONS[usize::from(c)].name.to_lowercase(), line.name()),
                    "champion": CHAMPIONS[usize::from(c)].name,
                    "family": super::look::Family::of(c).name(),
                    "line": line.name(),
                    "when": line.when(),
                    "heard_by": match r.aim { vo::Aim::Own => "the player alone", vo::Aim::Seen => "whoever sees the champion" },
                    "bus": Bus::Champions.name(),
                    "takes": takes.len(),
                    "files": (0..takes.len() as u8).map(|k| vo::file(c, line, k)).collect::<Vec<_>>(),
                    "says": takes,
                    "cast": sc.cast,
                })
            })
        })
        .collect();
    let mut out = serde_json::to_string_pretty(&serde_json::json!({
        "about": "Every sound the game plays (el_client_core::fx::sound), and every champion's lines \
                  (el_client_core::fx::vo). Written by `cargo test -p el-client-core fx::sound -- \
                  --ignored write_manifest`; art/tools/sounds.py makes the files of every sound that \
                  isn't optional, art/tools/voices.py the announcer's and the champions' placeholders \
                  (a recording of the same name replaces one).",
        "formats": ["ogg", "mp3"],
        "sounds": lines,
        "voices": voices,
    }))
    .unwrap_or_default();
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn audio_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/assets/audio")
    }

    #[test]
    fn the_families_come_first_in_order() {
        use super::super::look::Family;
        for f in 0..Family::COUNT {
            for (k, part) in [
                Part::Cast,
                Part::Launch,
                Part::Impact,
                Part::Blast,
                Part::Windup,
                Part::Dash,
                Part::Buff,
                Part::Ult,
            ]
            .into_iter()
            .enumerate()
            {
                let s = Sfx::family(f, part);
                let suffix = ["cast", "launch", "impact", "blast", "windup", "dash", "buff", "ult"][k];
                assert_eq!(s.def().name, format!("{}.{suffix}", Family::ALL[f].name()));
            }
        }
    }

    #[test]
    fn every_name_is_its_own() {
        for (i, d) in SOUNDS.iter().enumerate() {
            assert_eq!(Sfx::by_name(d.name), Some(Sfx::ALL[i]), "{} is named once", d.name);
            assert!(d.takes >= 1 && d.cap >= 1, "{}", d.name);
        }
    }

    #[test]
    fn the_manifest_is_current() {
        let path = audio_dir().join("manifest.json");
        let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            on_disk == manifest(),
            "{} is out of date: cargo test -p el-client-core fx::sound -- --ignored write_manifest",
            path.display()
        );
    }

    #[test]
    fn every_sound_we_make_is_there() {
        let dir = audio_dir();
        for &s in Sfx::ALL {
            let d = s.def();
            if d.optional {
                continue;
            }
            for k in 0..d.takes {
                for ext in ["ogg", "mp3"] {
                    let f = dir.join(format!("{}.{ext}", s.file(k)));
                    assert!(f.is_file(), "{} is missing: python3 -I art/tools/sounds.py", f.display());
                }
            }
        }
    }

    #[test]
    fn every_voice_has_its_files() {
        // The announcer's lines and every champion's takes: placeholders from voices.py, or the
        // recordings that replaced them.
        let dir = audio_dir();
        let mut stems: Vec<String> =
            Sfx::ALL.iter().filter(|s| s.def().bus == Bus::Voice).map(|s| s.file(0)).collect();
        for c in 0..CHAMPIONS.len() as u8 {
            for line in vo::Line::ALL {
                for k in 0..vo::script(c).takes(line).len() as u8 {
                    stems.push(vo::file(c, line, k));
                }
            }
        }
        for stem in stems {
            for ext in ["ogg", "mp3"] {
                let f = dir.join(format!("{stem}.{ext}"));
                assert!(f.is_file(), "{} is missing: python3 -I art/tools/voices.py", f.display());
            }
        }
    }

    #[test]
    #[ignore]
    fn write_manifest() {
        let dir = audio_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"), manifest()).unwrap();
    }
}
