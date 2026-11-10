//! The champions' own voices (`docs/FX.md`, "The voices"): what each says, when, how often, and who
//! hears it. The script is the contract, as Riot's VO scripts are: a line for each moment a
//! champion speaks, in its own words, and a note for whoever is cast to voice it. The files are
//! placeholders made from it (`art/tools/voices.py`) until a recording of the same name replaces
//! one; a line without its file is simply not heard.
//!
//! The moments are League's (its wiki's categories, `docs/FX.md`): a pick in champion select, the
//! first move of a match, moves (a long walk has longer lines), an attack ordered on a champion,
//! the first sight of the enemy, the ultimate ranked up, the ultimate cast, a kill, a death, the
//! return from it. The throttles are Riot's own numbers where they're known (a move line no more
//! than every 15 s, an attack line half the time, a respawn three times in four).
//!
//! Who hears: the lines that answer the player's own clicks (moves, attacks, the pick, the
//! respawn) are the player's alone; the ultimate's callout is everyone's who sees the caster,
//! since it warns as its sound does; a kill line is the killer's player's, and whoever sees the
//! killer; a death is heard by whoever sees it fall.

use el_sim::content::champions::CHAMPIONS;
use el_sim::units::SEATS;
use glam::Vec2;

use super::look::Side;

/// A moment a champion speaks at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Line {
    /// Champion select: dealt, rerolled or swapped to it.
    Pick,
    /// The match's first move order.
    FirstMove,
    /// A move order.
    Move,
    /// A move order at least [`LONG_MOVE`] away: the longer lines, the lore.
    LongMove,
    /// An attack ordered on an enemy champion.
    Attack,
    /// The first sight of an enemy champion.
    Encounter,
    /// The ultimate ranked up.
    UltRank,
    /// The ultimate cast: the callout everyone who sees it hears.
    Ult,
    Kill,
    Death,
    Respawn,
}

impl Line {
    pub const COUNT: usize = 11;
    pub const ALL: [Line; Line::COUNT] = [
        Line::Pick,
        Line::FirstMove,
        Line::Move,
        Line::LongMove,
        Line::Attack,
        Line::Encounter,
        Line::UltRank,
        Line::Ult,
        Line::Kill,
        Line::Death,
        Line::Respawn,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Line::Pick => "pick",
            Line::FirstMove => "first_move",
            Line::Move => "move",
            Line::LongMove => "long_move",
            Line::Attack => "attack",
            Line::Encounter => "encounter",
            Line::UltRank => "ult_rank",
            Line::Ult => "ult",
            Line::Kill => "kill",
            Line::Death => "death",
            Line::Respawn => "respawn",
        }
    }

    /// When it plays, in words (the manifest's, for whoever records it).
    pub fn when(self) -> &'static str {
        match self {
            Line::Pick => "champion select: the champion dealt, rerolled or swapped to",
            Line::FirstMove => "the match's first move order",
            Line::Move => "a move order (one in two, no more than every 15 s)",
            Line::LongMove => "a move order 2,000 units or more away (no more than every 30 s)",
            Line::Attack => "an attack ordered on an enemy champion (one in two, no more than every 15 s)",
            Line::Encounter => "the first enemy champion seen in a match",
            Line::UltRank => "the ultimate ranked up (levels 6, 11, 16)",
            Line::Ult => "the ultimate cast: heard by everyone who sees it",
            Line::Kill => "a champion killed: heard by the killer and whoever sees them",
            Line::Death => "the champion falls: heard by whoever sees it",
            Line::Respawn => "back on the platform (three times in four)",
        }
    }
}

/// A move order at least this far (units) is a long one (League's 2,000).
pub const LONG_MOVE: f32 = 2_000.0;

/// Who hears a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aim {
    /// The champion's own player, alone: the lines that answer their own clicks.
    Own,
    /// Whoever sees the champion.
    Seen,
}

/// How a line is throttled, and aimed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rule {
    pub aim: Aim,
    /// Seconds at least between two of it from one champion.
    pub cooldown: f64,
    /// The chance it plays when its moment comes, and nothing holds it back.
    pub chance: f32,
    /// At most this many a match (0: no limit).
    pub per_match: u8,
    /// It cuts the champion's own line still sounding (the moments that matter more than talk).
    pub interrupts: bool,
}

pub fn rule(line: Line) -> Rule {
    let r =
        |aim, cooldown, chance, per_match, interrupts| Rule { aim, cooldown, chance, per_match, interrupts };
    match line {
        Line::Pick => r(Aim::Own, 0.0, 1.0, 0, true),
        Line::FirstMove => r(Aim::Own, 0.0, 1.0, 1, false),
        // Riot's datamined numbers: a move line on a 15 s cooldown, a basic attack's at 30 to 50%.
        Line::Move => r(Aim::Own, 15.0, 0.5, 0, false),
        Line::LongMove => r(Aim::Own, 30.0, 0.75, 0, false),
        Line::Attack => r(Aim::Own, 15.0, 0.5, 0, false),
        Line::Encounter => r(Aim::Own, 0.0, 1.0, 1, false),
        Line::UltRank => r(Aim::Own, 0.0, 1.0, 0, false),
        Line::Ult => r(Aim::Seen, 0.0, 1.0, 0, true),
        Line::Kill => r(Aim::Seen, 0.0, 1.0, 0, true),
        Line::Death => r(Aim::Seen, 0.0, 1.0, 0, true),
        // Riot's respawn line: three times in four.
        Line::Respawn => r(Aim::Own, 0.0, 0.75, 0, false),
    }
}

/// One champion's script: what it says at each moment (more than one: a take drawn each time,
/// never the last one twice running), and how it should sound.
#[derive(Clone, Copy, Debug)]
pub struct Script {
    /// Who to cast: the voice, the age, the manner (a casting sheet's line, and the placeholder
    /// voice's brief).
    pub cast: &'static str,
    pub pick: &'static str,
    pub first_move: &'static str,
    pub moves: [&'static str; 3],
    pub long_moves: [&'static str; 2],
    pub attacks: [&'static str; 2],
    pub encounter: &'static str,
    pub ult_rank: &'static str,
    pub ult: &'static str,
    pub kills: [&'static str; 2],
    pub death: &'static str,
    pub respawn: &'static str,
}

impl Script {
    /// What it says at `line`: a take each.
    pub fn takes(&self, line: Line) -> &[&'static str] {
        match line {
            Line::Pick => core::slice::from_ref(&self.pick),
            Line::FirstMove => core::slice::from_ref(&self.first_move),
            Line::Move => &self.moves,
            Line::LongMove => &self.long_moves,
            Line::Attack => &self.attacks,
            Line::Encounter => core::slice::from_ref(&self.encounter),
            Line::UltRank => core::slice::from_ref(&self.ult_rank),
            Line::Ult => core::slice::from_ref(&self.ult),
            Line::Kill => &self.kills,
            Line::Death => core::slice::from_ref(&self.death),
            Line::Respawn => core::slice::from_ref(&self.respawn),
        }
    }
}

/// Champion `champ`'s script.
pub fn script(champ: u8) -> &'static Script {
    &SCRIPTS[usize::from(champ) % SCRIPTS.len()]
}

/// A take's file, without its extension: `vo.<champion>.<line>`, then `.<k>` (from 1) when the line
/// has more than one.
pub fn file(champ: u8, line: Line, take: u8) -> String {
    let name = CHAMPIONS[usize::from(champ) % CHAMPIONS.len()].name.to_lowercase();
    if script(champ).takes(line).len() <= 1 {
        format!("vo.{name}.{}", line.name())
    } else {
        format!("vo.{name}.{}.{}", line.name(), take + 1)
    }
}

/// About how long a take is said for (s), from its words: what keeps a champion from talking over
/// itself before its file's own length is known.
pub fn est_secs(text: &str) -> f64 {
    0.35 + 0.062 * text.chars().count() as f64
}

/// A line to say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spoken {
    /// The champion's seat, and its number.
    pub seat: u8,
    pub champ: u8,
    pub line: Line,
    pub take: u8,
    /// Where it's said (heard from there, as the effects are); none for the own champion's lines,
    /// heard in the middle as Riot plays a player's own.
    pub at: Option<Vec2>,
    pub side: Side,
    /// Its rung on the mixer's ladder ([`super::mix::rung`]'s).
    pub rung: u8,
    /// It cuts this champion's line still sounding.
    pub interrupts: bool,
}

impl Spoken {
    pub fn file(&self) -> String {
        file(self.champ, self.line, self.take)
    }

    pub fn text(&self) -> &'static str {
        let t = script(self.champ).takes(self.line);
        t[usize::from(self.take) % t.len()]
    }
}

/// Each champion's chatter: when it last said each line, how many of each this match, until when
/// it's talking, and its last take of each (so a take isn't said twice running).
#[derive(Clone, Debug)]
pub struct Chatter {
    rng: u32,
    last: [[f64; Line::COUNT]; SEATS],
    count: [[u8; Line::COUNT]; SEATS],
    busy: [f64; SEATS],
    last_take: [[u8; Line::COUNT]; SEATS],
}

impl Chatter {
    pub fn new(seed: u32) -> Self {
        Self {
            rng: seed | 1,
            last: [[f64::NEG_INFINITY; Line::COUNT]; SEATS],
            count: [[0; Line::COUNT]; SEATS],
            busy: [f64::NEG_INFINITY; SEATS],
            last_take: [[u8::MAX; Line::COUNT]; SEATS],
        }
    }

    fn rand(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    fn roll(&mut self, chance: f32) -> bool {
        chance >= 1.0 || (self.rand() % 10_000) as f32 / 10_000.0 < chance
    }

    /// Champion `champ` in `seat` has a moment for `line` at `now`, for a viewer on `side` of it
    /// (`Side::Own`: it's the viewer's own). What it says, if it says anything: the line's aim,
    /// cooldown, chance and count decide, then whether it's still talking.
    pub fn moment(
        &mut self,
        seat: usize,
        champ: u8,
        line: Line,
        side: Side,
        at: Option<Vec2>,
        now: f64,
    ) -> Option<Spoken> {
        if seat >= SEATS {
            return None;
        }
        let r = rule(line);
        let own = side == Side::Own;
        if r.aim == Aim::Own && !own {
            return None;
        }
        let k = line as usize;
        if now - self.last[seat][k] < r.cooldown {
            return None;
        }
        if r.per_match > 0 && self.count[seat][k] >= r.per_match {
            return None;
        }
        if !r.interrupts && now < self.busy[seat] {
            return None;
        }
        // A kill heard by the killer's allies is half as likely: it's for the killer and the
        // team it was said at.
        let chance = if line == Line::Kill && side == Side::Ally { r.chance * 0.5 } else { r.chance };
        if !self.roll(chance) {
            return None;
        }
        let takes = script(champ).takes(line).len().max(1) as u32;
        let last = self.last_take[seat][k];
        let mut take = (self.rand() % takes) as u8;
        if takes > 1 && take == last {
            take = ((u32::from(take) + 1 + self.rand() % (takes - 1)) % takes) as u8;
        }
        self.last_take[seat][k] = take;
        self.last[seat][k] = now;
        self.count[seat][k] = self.count[seat][k].saturating_add(1);
        let text = script(champ).takes(line)[usize::from(take)];
        self.busy[seat] = now + est_secs(text);
        let rung = match (side, line) {
            (Side::Own, _) => 0,
            (Side::Enemy, Line::Ult) => 1,
            (Side::Enemy, _) => 2,
            (_, Line::Ult | Line::Kill | Line::Death) => 3,
            _ => 4,
        };
        Some(Spoken {
            seat: seat as u8,
            champ,
            line,
            take,
            at: if own { None } else { at },
            side,
            rung,
            interrupts: r.interrupts,
        })
    }

    /// The champion in `seat` is talking until `until` (its file's own length, once it's playing).
    pub fn talking(&mut self, seat: usize, until: f64) {
        if let Some(b) = self.busy.get_mut(seat) {
            *b = until;
        }
    }
}

/// The scripts, in the pool's order (`el_sim::content::champions`).
pub static SCRIPTS: [Script; 20] = [
    Script {
        cast: "Vessa, the Long Watch: a woman in her thirties, a sentry with a long crossbow. Calm, dry, \
               exact; she never raises her voice, even at the ultimate. Close-miked, little room.",
        pick: "The watch is mine tonight.",
        first_move: "Eyes on the far end of the bridge.",
        moves: ["Moving to cover.", "Keep the line.", "I see further from here."],
        long_moves: [
            "Every watch ends at dawn. Not this one.",
            "I've counted every stone of this bridge. Twice.",
        ],
        attacks: ["In my sights.", "Hold still."],
        encounter: "There. Movement on the bridge.",
        ult_rank: "A longer reach.",
        ult: "There's nowhere to hide!",
        kills: ["One less to watch.", "Just as I called it."],
        death: "Lost the light.",
        respawn: "Back on the watch.",
    },
    Script {
        cast: "Orrin of the Ash Choir: a man in his fifties, a fire-priest who sings his spells. Sonorous, \
               unhurried, a little ecstatic; warm, with the room of a hall round him.",
        pick: "Our choir sings in fire and ash.",
        first_move: "Let the braziers hear us.",
        moves: ["The hymn moves on.", "Walk in the warm.", "Ashes to ashes."],
        long_moves: [
            "We sang before the bridge was stone. We will sing after.",
            "Every ember remembers the fire it came from.",
        ],
        attacks: ["Burn for me.", "Catch fire!"],
        encounter: "New voices for the choir.",
        ult_rank: "The hymn grows louder.",
        ult: "Let the heavens burn!",
        kills: ["Sung to silence.", "Ash, at last."],
        death: "And so, the hymn ends.",
        respawn: "The choir calls me back.",
    },
    Script {
        cast: "Brakka, the Quarry: a huge man of the mountains, slow and deep; few words, each one set \
               down like a block. Pitched down, gravel in the low end.",
        pick: "Brakka stands. You break.",
        first_move: "Good ground here. Hard ground.",
        moves: ["Rolling.", "Heavy footsteps.", "Out of my way."],
        long_moves: [
            "They cut this bridge from my mountain. I walk it anyway.",
            "Stone does not hurry. Stone arrives.",
        ],
        attacks: ["Come here.", "I crush you!"],
        encounter: "Something small. Something to break.",
        ult_rank: "The mountain is waking.",
        ult: "The ground is mine!",
        kills: ["Broken.", "You are gravel."],
        death: "I crumble.",
        respawn: "Stone, again.",
    },
    Script {
        cast: "Kesh, the Quiet Knife: a man of any age, a hired blade. Half a whisper, close and dry, \
               amused; never loud, even when he kills. A breathy double under the voice.",
        pick: "No one hears the second blade.",
        first_move: "Now, we go quietly.",
        moves: ["Out of sight.", "Softly.", "Between the lights."],
        long_moves: ["I was paid in silence. I spend it well.", "A quiet knife cuts deepest."],
        attacks: ["There you are.", "Hush now."],
        encounter: "I see you. You don't see me.",
        ult_rank: "A sharper edge.",
        ult: "Your sentence is now.",
        kills: ["Done, and quietly.", "Nothing to hear."],
        death: "Too loud.",
        respawn: "Back again. Quietly.",
    },
    Script {
        cast: "Liora, the Lantern: a woman in her forties, a keeper of lights. Warm, kind and firm, the \
               voice that tells you you're safe. A faint bright ring on her vowels.",
        pick: "I'll keep the light for you.",
        first_move: "Stay close to the lantern.",
        moves: ["Follow the light.", "I'm with you.", "Steady now."],
        long_moves: [
            "Every lantern on this bridge was lit by someone who stayed.",
            "The night is long. So is my oil.",
        ],
        attacks: ["Be still.", "The light finds you."],
        encounter: "Shadows ahead. Stay near me.",
        ult_rank: "The flame burns brighter.",
        ult: "Stand in the light!",
        kills: ["Rest now.", "Go gently."],
        death: "The light dims.",
        respawn: "The lantern is relit.",
    },
    Script {
        cast: "Talmar, the Tidewarden: a big man in his forties, a sea-warden, booming and cheerful, \
               used to shouting over surf. A slow chorus on him, like water moving.",
        pick: "The tide answers to me.",
        first_move: "Smell that? The sea, far below.",
        moves: ["With the current.", "Rolling in.", "The tide's turning."],
        long_moves: [
            "The sea took the old bridge. It'll take this one too, in time.",
            "I've stood watch over worse water than this.",
        ],
        attacks: ["Overboard with you!", "Wash out!"],
        encounter: "Sails on the horizon!",
        ult_rank: "The deep rises.",
        ult: "Into the deep!",
        kills: ["Swept away.", "The tide's out for you."],
        death: "Pulled under.",
        respawn: "High tide again.",
    },
    Script {
        cast: "Seren, the Quiet Star: a fallen star in a woman's shape, ageless. Soft, distant, a little \
               sad; she speaks as if from far above. A shimmer an octave up, a long bright room.",
        pick: "I fell so that you could look up.",
        first_move: "The sky is listening.",
        moves: ["Drifting.", "As the stars turn.", "Quietly."],
        long_moves: ["Every star is a door left open.", "I have watched this bridge from very far away."],
        attacks: ["Fade away.", "Your light dims."],
        encounter: "Small lights, burning so brightly.",
        ult_rank: "The sky draws nearer.",
        ult: "Look to the sky!",
        kills: ["Another light goes out.", "Gone to the dark."],
        death: "Falling again.",
        respawn: "I rise again.",
    },
    Script {
        cast: "Hollis, the Bell Warden: an old man, deep and steady, a voice that has called the hours \
               for fifty years. Unhurried even at the ultimate. A bronze resonance under him.",
        pick: "The bell rings for us.",
        first_move: "Ring out, and hold.",
        moves: ["Right on the hour.", "Steady as bronze.", "Keep time."],
        long_moves: [
            "A bell does not fight. It reminds.",
            "I have rung the hours of every watch on this bridge.",
        ],
        attacks: ["Hear the bell toll.", "Ring out!"],
        encounter: "Strangers at the gate.",
        ult_rank: "Louder, old bell.",
        ult: "Hold the bell!",
        kills: ["Your hour is rung.", "The bell tolls for you."],
        death: "The bell falls silent.",
        respawn: "The bell rings again.",
    },
    Script {
        cast: "Vire, the Ashen Blade: a woman in her thirties, the last of a burned order of judges. \
               Hard, clipped, contemptuous; fierce at the verdict. Dry, a little grit in the top.",
        pick: "My blade has a verdict for you.",
        first_move: "Ash on the wind. Good.",
        moves: ["Pressing on.", "Closing.", "Blade ready."],
        long_moves: ["They burned my order. I kept the blade.", "Judgment doesn't need a court."],
        attacks: ["Face me!", "Cut down!"],
        encounter: "The accused approach.",
        ult_rank: "Judgment sharpens.",
        ult: "You are guilty!",
        kills: ["You have been judged.", "Sentence carried out."],
        death: "This isn't finished.",
        respawn: "The blade returns.",
    },
    Script {
        cast: "Oda of the Long Road: an old woman, a wanderer and healer; warm, wry, patient, a little \
               out of breath from the walking. Close and natural, a soft bronze ring.",
        pick: "Every road leads here, sooner or later.",
        first_move: "One more step on the long road.",
        moves: ["Onward.", "Mind the stones.", "The road goes on."],
        long_moves: [
            "I've walked further than this bridge is long, and back again.",
            "A good road remembers who walked it.",
        ],
        attacks: ["Stay right there.", "Off the road!"],
        encounter: "Travellers. Not friendly ones.",
        ult_rank: "The road grows longer.",
        ult: "Walk with me!",
        kills: ["Your road ends here.", "Rest by the wayside."],
        death: "The road goes on without me.",
        respawn: "Back on the road.",
    },
    Script {
        cast: "Marret, the Gunwright: a man in his forties, a gunsmith who tests his own work. Gruff, \
               wry, matter-of-fact; pleased by a good shot. Dry and close.",
        pick: "Loaded, oiled and ready to fire.",
        first_move: "Let's see what this barrel can do.",
        moves: ["Reloading on the move.", "Keep the powder dry.", "Walking."],
        long_moves: ["I built every gun I carry. Named none of them.", "Brass is cheap. Good aim isn't."],
        attacks: ["Lining it up.", "Bang, bang."],
        encounter: "Targets, down range.",
        ult_rank: "Bigger guns.",
        ult: "All barrels, fire!",
        kills: ["Spent, like the brass.", "That's one for the ledger."],
        death: "The gun's jammed.",
        respawn: "Fresh powder.",
    },
    Script {
        cast: "Nimra, the Frostglass: a young woman, a glass-maker who works in ice. Cool, crisp and \
               precise, a little haughty. A glassy shimmer an octave up, very light.",
        pick: "Clear as glass, and just as cold.",
        first_move: "The air is thin and cold. Lovely.",
        moves: ["Gliding.", "Mind the frost.", "Clear as crystal."],
        long_moves: ["Glass remembers every crack. So do I.", "Winter isn't cruel. It's honest."],
        attacks: ["Freeze.", "Shatter like glass."],
        encounter: "Warm blood. How quaint.",
        ult_rank: "The cold deepens.",
        ult: "Into the whiteout!",
        kills: ["Frozen still.", "Brittle as glass."],
        death: "Broken glass.",
        respawn: "Whole again.",
    },
    Script {
        cast: "Gorum, the Old Ox: a very old, very big man, a beast of burden who stood up. The deepest \
               and slowest voice of the twenty; kind, stubborn. Pitched down, felt in the chest.",
        pick: "Old. Not done.",
        first_move: "A long field.",
        moves: ["Plodding on.", "Hooves on stone.", "Slow, and sure."],
        long_moves: [
            "I've pulled heavier loads than this bridge.",
            "The young ones run. The old ones arrive.",
        ],
        attacks: ["Take the horns!", "Out of my field."],
        encounter: "Ah. Company.",
        ult_rank: "Still standing.",
        ult: "Nothing breaks me!",
        kills: ["Trampled flat.", "Stay down."],
        death: "Rest, at last.",
        respawn: "Back to the yoke.",
    },
    Script {
        cast: "Sable, the Night Courier: a woman in her twenties, a courier of curses. Smooth, sly, \
               amused, quick. A dark octave-down double under her, a little night air.",
        pick: "Special delivery.",
        first_move: "The night's young. So are they.",
        moves: ["On my route.", "Through the shade.", "Swift and silent."],
        long_moves: [
            "I've never lost a letter. Or a mark.",
            "The night carries everything, if you know how to ask.",
        ],
        attacks: ["Signed for.", "You're hexed."],
        encounter: "A name on my list.",
        ult_rank: "The night grows longer.",
        ult: "This is the dark hour.",
        kills: ["Delivered.", "Return to sender."],
        death: "Lost in the post.",
        respawn: "Back on the route.",
    },
    Script {
        cast: "Pell, the Hedge Knight: a young man, a knight with no lord, earnest and brave and a \
               little too loud. Bright, open; a bronze ring on the battle cries.",
        pick: "No lord, no lands, but a good shield!",
        first_move: "Banner up! Let's ride.",
        moves: ["Onward!", "For the hedge!", "Shield first."],
        long_moves: [
            "A knight of the hedge sleeps under the stars. I don't mind.",
            "No one knighted me. I knighted myself.",
        ],
        attacks: ["Have at you!", "Guard yourself!"],
        encounter: "A worthy foe!",
        ult_rank: "My banner flies higher!",
        ult: "To the banner!",
        kills: ["Do you yield?", "Honour is served."],
        death: "My banner falls.",
        respawn: "Back up again!",
    },
    Script {
        cast: "Ulla, the Mender: an old woman, a gardener and healer. Gentle, earthy, unhurried, with \
               a smile in it; firm when the thorns come out. Warm and close.",
        pick: "Everything grows back.",
        first_move: "Even here, something grows.",
        moves: ["Growing, growing.", "Root and branch.", "Gently now."],
        long_moves: [
            "Stone is only soil that forgot how to grow.",
            "I've mended worse than a broken bridge.",
        ],
        attacks: ["Thorns for you.", "I'll sap you dry."],
        encounter: "Weeds in the garden.",
        ult_rank: "Deeper roots.",
        ult: "Let it all bloom!",
        kills: ["Back to the soil.", "Into the compost."],
        death: "I'm wilting.",
        respawn: "Spring again.",
    },
    Script {
        cast: "Torek, the Siege Engine: a war machine that learned to talk. Flat, measured, mechanical; \
               reports, never feels. The one voice processed hard: ring-modulated, a little crushed.",
        pick: "Siege engine, ready.",
        first_move: "Calculating range.",
        moves: ["Repositioning.", "Treads turning.", "Advancing."],
        long_moves: [
            "They built me to break walls. A bridge is a wall lying down.",
            "Every shell is a question. The answer is always yes.",
        ],
        attacks: ["Target acquired.", "Firing."],
        encounter: "Hostiles detected.",
        ult_rank: "Payload upgraded.",
        ult: "Commencing bombardment!",
        kills: ["Target destroyed.", "Range confirmed."],
        death: "All systems failing.",
        respawn: "Rebuilt.",
    },
    Script {
        cast: "Sefa, the Wind Dancer: a young woman, a dancer who fights. Light, playful, laughing, \
               always moving; breathless at the tempest. Airy, a little breath round her.",
        pick: "Let's dance!",
        first_move: "Feel that breeze!",
        moves: ["Light on my feet.", "With the wind.", "Spinning!"],
        long_moves: [
            "The wind never walks the same road twice.",
            "Dance like the bridge is falling. It might.",
        ],
        attacks: ["Keep up!", "One, two!"],
        encounter: "Partners! Shall we?",
        ult_rank: "The wind picks up!",
        ult: "Dance in the storm!",
        kills: ["Swept off your feet.", "Much too slow!"],
        death: "The music stops.",
        respawn: "Time for an encore!",
    },
    Script {
        cast: "Brun, the Smith: a man in his fifties, a blacksmith; gruff, hearty, proud of his work, \
               loud from the forge. Warm, a little saturated, the forge's room.",
        pick: "Strike while it's hot!",
        first_move: "Good iron in these gates.",
        moves: ["The hammer's hungry.", "Stoke the fire.", "Heading out."],
        long_moves: [
            "I forged the hinges on those gates. Built to last.",
            "Every blade I make is a promise.",
        ],
        attacks: ["Hammer coming down!", "On the anvil!"],
        encounter: "Fresh metal to work.",
        ult_rank: "Hotter forge.",
        ult: "Meet the anvil!",
        kills: ["Flattened.", "Hammered flat."],
        death: "The fire's going out.",
        respawn: "Back from the forge.",
    },
    Script {
        cast: "Mira, the Lamplighter: a girl of about seventeen, the bridge's lamplighter. Bright, \
               quick, cheerful and brave; fierce when she snuffs. Warm, a little crackle round her.",
        pick: "Someone has to light the way.",
        first_move: "The lamps are lit. Let's go!",
        moves: ["Lighting the way.", "Mind the dark corners.", "Up bright and early."],
        long_moves: [
            "I light the lamps so nobody walks the bridge alone.",
            "My grandmother lit these lamps. And hers before her.",
        ],
        attacks: ["Burn nice and bright!", "Snuffed!"],
        encounter: "Someone's out past dark.",
        ult_rank: "A bigger flame!",
        ult: "Light the bonfire!",
        kills: ["Lights out.", "Snuffed out."],
        death: "Burning out.",
        respawn: "Lit again!",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_champion_has_every_line_and_its_files_are_its_own() {
        assert_eq!(SCRIPTS.len(), CHAMPIONS.len());
        let mut files = std::collections::HashSet::new();
        for c in 0..CHAMPIONS.len() as u8 {
            let s = script(c);
            assert!(s.cast.starts_with(CHAMPIONS[usize::from(c)].name), "{}'s casting note", s.cast);
            for line in Line::ALL {
                let takes = s.takes(line);
                assert!(!takes.is_empty());
                for (k, t) in takes.iter().enumerate() {
                    assert!(!t.trim().is_empty() && t.len() < 80, "{c} {line:?}: {t:?}");
                    assert!(files.insert(file(c, line, k as u8)), "{} twice", file(c, line, k as u8));
                }
            }
        }
    }

    #[test]
    fn the_own_lines_are_the_players_and_a_callout_everyones() {
        let mut c = Chatter::new(7);
        assert!(c.moment(0, 0, Line::Move, Side::Ally, None, 0.0).is_none(), "a move line is the player's");
        assert!(c.moment(0, 0, Line::Respawn, Side::Enemy, None, 0.0).is_none());
        let at = Some(Vec2::new(1.0, 2.0));
        let ult = c.moment(5, 6, Line::Ult, Side::Enemy, at, 0.0).expect("an enemy's callout is heard");
        assert_eq!((ult.rung, ult.at, ult.interrupts), (1, at, true));
        let own = c.moment(0, 0, Line::Ult, Side::Own, at, 0.0).unwrap();
        assert_eq!((own.rung, own.at), (0, None), "the own champion's lines are heard in the middle");
    }

    #[test]
    fn moves_are_throttled_and_takes_never_repeat_back_to_back() {
        let mut c = Chatter::new(3);
        // A click a second for five minutes: a move line no more than every 15 s, about half the
        // times it could.
        let mut said = Vec::new();
        for s in 0..300 {
            if let Some(v) = c.moment(2, 4, Line::Move, Side::Own, None, f64::from(s)) {
                said.push((s, v.take));
            }
        }
        assert!(said.windows(2).all(|w| w[1].0 - w[0].0 >= 15), "{said:?}");
        assert!((8..=20).contains(&said.len()), "about one in two of 20 chances: {}", said.len());
        assert!(said.windows(2).all(|w| w[1].1 != w[0].1), "a take twice running: {said:?}");
        // Once a match.
        assert!(c.moment(2, 4, Line::FirstMove, Side::Own, None, 400.0).is_some());
        assert!(c.moment(2, 4, Line::FirstMove, Side::Own, None, 900.0).is_none());
    }

    #[test]
    fn a_champion_doesnt_talk_over_itself_but_its_callout_cuts_in() {
        let mut c = Chatter::new(11);
        // Its long line, playing for four seconds from 100 s.
        c.talking(1, 104.0);
        // While it's still talking, an attack line waits; the ultimate doesn't.
        let attacked = (0..40)
            .any(|k| c.moment(1, 1, Line::Attack, Side::Own, None, 100.5 + f64::from(k) * 0.05).is_some());
        assert!(!attacked, "talked over its own long line");
        let ult = c.moment(1, 1, Line::Ult, Side::Own, None, 102.5).expect("the callout cuts in");
        assert!(ult.interrupts);
        // And once it's done, it talks again.
        assert!(
            (0..40).any(|k| c
                .moment(1, 1, Line::Attack, Side::Own, None, 120.0 + f64::from(k) * 16.0)
                .is_some())
        );
    }
}
