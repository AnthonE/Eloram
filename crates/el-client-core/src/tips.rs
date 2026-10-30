//! What the HUD's tooltips say: an ability, an item or an augment in words, read from the same
//! tables the sim plays (`el_sim::content`), so a number changed there is changed here. The first
//! line is the name; the browser client draws it brighter (`el-client/src/hud.rs`).

use el_sim::content::augments::{AUGMENTS, Tier, Trigger};
use el_sim::content::champions::ability;
use el_sim::content::items::ITEMS;
use el_sim::content::{DmgKind, Effect, Filter, Op, StatMods, Target};
use el_sim::units::BuffKind;

/// A number without a trailing `.0`, to one decimal place otherwise.
fn num(v: f32) -> String {
    let r = (v * 10.0).round() / 10.0;
    if r.fract() == 0.0 { format!("{r:.0}") } else { format!("{r:.1}") }
}

fn pct(share: f32) -> String {
    format!("{}%", num(share * 100.0))
}

/// The ranked values at each rank, `a/b/c`, collapsed to one when they're all the same.
fn ranked(v: &[f32]) -> String {
    if v.iter().all(|&x| x == v[0]) {
        num(v[0])
    } else {
        v.iter().map(|&x| num(x)).collect::<Vec<_>>().join("/")
    }
}

fn buff(kind: BuffKind, value: f32) -> String {
    match kind {
        BuffKind::MoveSpeed => format!("{} move speed", pct(value)),
        BuffKind::AttackSpeed => format!("{} attack speed", pct(value)),
        BuffKind::AttackDamage => format!("{} attack damage", num(value)),
        BuffKind::AbilityPower => format!("{} ability power", num(value)),
        BuffKind::Armor => format!("{} armour", num(value)),
        BuffKind::MagicResist => format!("{} magic resist", num(value)),
        BuffKind::Marked => "a mark".into(),
        BuffKind::None => "nothing".into(),
    }
}

/// One op in words; `ranks` is how many of its ranked values to show (5 for Q, W and E, 3 for R,
/// 1 for what isn't ranked).
fn op(o: &Op, ranks: usize) -> String {
    let scale = |ad: f32, ap: f32| {
        let mut s = String::new();
        if ad != 0.0 {
            s.push_str(&format!(" (+{} AD)", pct(ad)));
        }
        if ap != 0.0 {
            s.push_str(&format!(" (+{} AP)", pct(ap)));
        }
        s
    };
    match *o {
        Op::Damage { kind, base, ad, ap, missing } => {
            let kind = match kind {
                DmgKind::Physical => "physical",
                DmgKind::Magic => "magic",
                DmgKind::True => "true",
            };
            let missing = if missing > 0.0 {
                format!(" and {} of missing health", pct(missing))
            } else {
                String::new()
            };
            format!("{} {kind} damage{}{missing}", ranked(&base[..ranks]), scale(ad, ap))
        }
        Op::Heal { base, ap } => format!("heals {}{}", ranked(&base[..ranks]), scale(0.0, ap)),
        Op::Shield { base, ap, secs } => {
            format!("a {}{} shield for {} s", ranked(&base[..ranks]), scale(0.0, ap), num(secs))
        }
        Op::Stun(s) => format!("stuns for {} s", num(s)),
        Op::Root(s) => format!("roots for {} s", num(s)),
        Op::Silence(s) => format!("silences for {} s", num(s)),
        Op::Airborne(s) => format!("knocks up for {} s", num(s)),
        Op::Slow { share, secs } => format!("slows by {} for {} s", pct(share), num(secs)),
        Op::Knockback(d) => format!("knocks back {}", num(d)),
        Op::Pull => "pulls to the caster".into(),
        Op::Buff { kind, secs, value } => format!("grants {} for {} s", buff(kind, value), num(secs)),
        Op::SelfBuff { kind, secs, value } => format!("you gain {} for {} s", buff(kind, value), num(secs)),
        Op::Reveal(s) => format!("reveals for {} s", num(s)),
    }
}

fn ops(list: &[Op], ranks: usize) -> String {
    let mut s = list.iter().map(|o| op(o, ranks)).collect::<Vec<_>>().join(", ");
    if let Some(first) = s.get(..1) {
        s.replace_range(..1, &first.to_uppercase());
    }
    s
}

/// How an ability reaches, in a word or two.
fn reach(target: Target, e: &Effect, range: f32) -> String {
    let what = match (target, e) {
        (_, Effect::Line { pierce: true, .. }) => "Skillshot, piercing".into(),
        (_, Effect::Line { .. }) => "Skillshot".into(),
        (_, Effect::Homing { .. }) => "Targeted missile".into(),
        (_, Effect::Strike { .. }) => "Targeted".into(),
        (Target::Caster, Effect::Area { radius, .. }) => format!("Around you ({} radius)", num(*radius)),
        (_, Effect::Area { radius, filter, .. }) => {
            let on = if *filter == Filter::Allies { ", on allies" } else { "" };
            format!("Area ({} radius{on})", num(*radius))
        }
        (_, Effect::Zone { radius, secs, follows, .. }) => {
            let on = if *follows { ", around you" } else { "" };
            format!("Zone for {} s ({} radius{on})", num(*secs), num(*radius))
        }
        (_, Effect::Dash { .. }) => "Dash".into(),
        (_, Effect::Blink { .. }) => "Blink".into(),
        (_, Effect::OnSelf { .. }) => "On yourself".into(),
    };
    if range > 0.0 && !matches!(target, Target::Caster) {
        format!("{what}, {} range", num(range))
    } else {
        what
    }
}

/// Champion `champ`'s ability in `slot` (Q, W, E, R, then the two summoner spells), at `rank`
/// (0 not learned yet).
pub fn ability_tip(champ: u8, slot: u8, rank: u8) -> String {
    let a = ability(champ, slot);
    let ranks = match slot {
        0..=2 => 5,
        3 => 3,
        _ => 1,
    };
    let rank_line = match slot {
        0..=3 if rank == 0 => "  (not learned)".to_string(),
        0..=3 => format!("  rank {rank}/{ranks}"),
        _ => String::new(),
    };
    let mut out = format!("{}{rank_line}\n{}\n", a.name, reach(a.target, &a.effect, a.range));
    let cd = ranked(&a.cooldown[..ranks]);
    let cost = ranked(&a.cost[..ranks]);
    if a.cost[..ranks].iter().any(|&c| c > 0.0) {
        out.push_str(&format!("Cooldown {cd} s   Cost {cost} mana\n"));
    } else {
        out.push_str(&format!("Cooldown {cd} s\n"));
    }
    let what = ops(a.effect.ops(), ranks);
    if !what.is_empty() {
        out.push_str(&what);
        out.push('.');
    }
    out.trim_end().to_string()
}

/// Stat changes in words, a line each.
pub fn mods_lines(m: &StatMods) -> Vec<String> {
    let flat = |v: f32, what: &str| (v != 0.0).then(|| format!("+{} {what}", num(v)));
    let share = |v: f32, what: &str| (v != 0.0).then(|| format!("+{} {what}", pct(v)));
    [
        flat(m.hp, "health"),
        flat(m.mana, "mana"),
        flat(m.ad, "attack damage"),
        flat(m.ap, "ability power"),
        flat(m.armor, "armour"),
        flat(m.mr, "magic resist"),
        share(m.attack_speed, "attack speed"),
        flat(m.move_speed, "move speed"),
        share(m.move_share, "move speed"),
        flat(m.haste, "ability haste"),
        share(m.lifesteal, "life steal"),
        flat(m.hp_regen, "health regen a second"),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Item `i` in the shop's words.
pub fn item_tip(i: u8) -> String {
    let Some(it) = ITEMS.get(usize::from(i)) else { return String::new() };
    let mut out = format!("{}\n{} gold", it.name, it.cost);
    for l in mods_lines(&it.mods) {
        out.push('\n');
        out.push_str(&l);
    }
    out
}

pub fn tier_name(t: Tier) -> &'static str {
    match t {
        Tier::Silver => "Silver",
        Tier::Gold => "Gold",
        Tier::Prismatic => "Prismatic",
    }
}

/// What augment `a` does, without its name or tier.
pub fn augment_text(a: u8) -> String {
    let Some(a) = AUGMENTS.get(usize::from(a)) else { return String::new() };
    let mut lines = mods_lines(&a.mods);
    let every = |cd: f32| if cd > 0.0 { format!(" (once every {} s)", num(cd)) } else { String::new() };
    let on = match a.trigger {
        Trigger::None => None,
        Trigger::AutoHit { every: n, ops: o } => Some(format!("Every {n} attacks that land: {}", ops(o, 1))),
        Trigger::AbilityHit { cd, ops: o } => {
            Some(format!("An ability that hits{}: {}", every(cd), ops(o, 1)))
        }
        Trigger::Cast { cd, ops: o } => Some(format!("Casting an ability{}: {}", every(cd), ops(o, 1))),
        Trigger::Takedown { ops: o } => Some(format!("A kill or an assist: {}", ops(o, 1))),
        Trigger::LowHealth { below, cd, ops: o } => {
            Some(format!("Falling under {} health{}: {}", pct(below), every(cd), ops(o, 1)))
        }
        Trigger::Pulse { period, radius, ops: o } => {
            Some(format!("Every {} s, enemies within {}: {}", num(period), num(radius), ops(o, 1)))
        }
    };
    lines.extend(on.map(|mut s| {
        s.push('.');
        s
    }));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use el_sim::content::augments::AUGMENTS;
    use el_sim::content::champions::CHAMPIONS;

    #[test]
    fn numbers_read_cleanly() {
        assert_eq!(num(80.0), "80");
        assert_eq!(num(1.5), "1.5");
        assert_eq!(pct(0.3), "30%");
        assert_eq!(ranked(&[8.0, 7.0, 6.0]), "8/7/6");
        assert_eq!(ranked(&[60.0, 60.0, 60.0]), "60");
    }

    #[test]
    fn every_ability_item_and_augment_has_words() {
        for c in 0..CHAMPIONS.len() as u8 {
            for s in 0..6 {
                for rank in [0, 1] {
                    let t = ability_tip(c, s, rank);
                    let first = t.lines().next().unwrap_or("");
                    assert!(first.starts_with(ability(c, s).name), "{t}");
                    assert!(t.lines().count() >= 3, "{t}");
                }
            }
        }
        for i in 0..ITEMS.len() as u8 {
            assert!(item_tip(i).lines().count() >= 3, "{}", item_tip(i));
        }
        for a in 0..AUGMENTS.len() as u8 {
            assert!(!augment_text(a).is_empty(), "{}", AUGMENTS[usize::from(a)].name);
        }
    }
}
