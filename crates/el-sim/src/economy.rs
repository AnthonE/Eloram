//! A champion's growth: stats from level, items, augments and buffs; experience and levels; skill
//! points; the shop; and Mayhem's augment screens and their triggers.

use crate::content::augments::{AUGMENTS, Trigger};
use crate::content::items::{self, ITEMS};
use crate::content::rules::{self, AUGMENT_LEVELS, MAX_LEVEL, PASSIVE_GOLD_FROM, PASSIVE_GOLD_PER_SEC};
use crate::content::{Op, StatMods};
use crate::events::Event;
use crate::game::Match;
use crate::math;
use crate::missile::Source;
use crate::ticks;
use crate::units::{BuffKind, SEATS, Stats, team_of_seat};

impl Match {
    /// A champion's stats now: base, growth to its level, its items' and augments' changes, its
    /// buffs.
    pub(crate) fn stats_of(&self, seat: usize) -> Stats {
        let now = self.tick;
        let c = &self.champs[seat];
        let def = c.def();
        let lvl = f32::from(c.level - 1);
        let (b, g, m) = (def.base, def.growth, c.mods);
        let u = &self.units[seat];
        Stats {
            max_hp: b.max_hp + g.max_hp * lvl + m.hp,
            hp_regen: b.hp_regen + g.hp_regen * lvl + m.hp_regen,
            max_mana: b.max_mana + g.max_mana * lvl + m.mana,
            mana_regen: b.mana_regen + g.mana_regen * lvl,
            ad: b.ad + g.ad * lvl + m.ad + u.buff(BuffKind::AttackDamage, now),
            ap: b.ap + m.ap + u.buff(BuffKind::AbilityPower, now),
            armor: b.armor + g.armor * lvl + m.armor + u.buff(BuffKind::Armor, now),
            mr: b.mr + g.mr * lvl + m.mr + u.buff(BuffKind::MagicResist, now),
            attack_speed: (b.attack_speed + g.attack_speed * lvl)
                * (1.0 + m.attack_speed + u.buff(BuffKind::AttackSpeed, now)),
            range: b.range,
            move_speed: (b.move_speed + m.move_speed)
                * (1.0 + m.move_share + u.buff(BuffKind::MoveSpeed, now)),
            haste: m.haste,
            lifesteal: m.lifesteal,
        }
    }

    /// Refreshes a champion's stats; maximum health or mana gained comes with as much health or
    /// mana.
    pub(crate) fn refresh_stats(&mut self, seat: usize) {
        let s = self.stats_of(seat);
        let u = &mut self.units[seat];
        let old = u.stats;
        u.stats = s;
        if u.alive {
            if old.max_hp > 0.0 && s.max_hp > old.max_hp {
                u.hp += s.max_hp - old.max_hp;
            }
            if old.max_mana > 0.0 && s.max_mana > old.max_mana {
                u.mana += s.max_mana - old.max_mana;
            }
            u.hp = u.hp.min(s.max_hp);
            u.mana = u.mana.min(s.max_mana);
        }
    }

    pub(crate) fn gain_xp(&mut self, seat: usize, xp: f32) {
        self.champs[seat].xp += xp;
        loop {
            let c = &mut self.champs[seat];
            if c.level >= MAX_LEVEL || c.xp < rules::xp_to_next(c.level) {
                if c.level >= MAX_LEVEL {
                    c.xp = 0.0;
                }
                return;
            }
            c.xp -= rules::xp_to_next(c.level);
            c.level += 1;
            c.points += 1;
            if AUGMENT_LEVELS[1..].contains(&c.level) {
                c.screens_earned += 1;
            }
            let level = c.level;
            self.event(Event::LevelUp { unit: seat as u16, level });
        }
    }

    /// The most ranks `slot` (Q, W, E or R) may have at `level`.
    pub fn max_rank(slot: u8, level: u8) -> u8 {
        if slot == 3 {
            u8::from(level >= 6) + u8::from(level >= 11) + u8::from(level >= 16)
        } else {
            level.div_ceil(2).min(5)
        }
    }

    pub(crate) fn level_skill(&mut self, seat: usize, slot: u8) {
        let c = &mut self.champs[seat];
        if slot >= 4 || c.points == 0 {
            return;
        }
        if c.ranks[usize::from(slot)] < Self::max_rank(slot, c.level) {
            c.ranks[usize::from(slot)] += 1;
            c.points -= 1;
        }
    }

    pub(crate) fn recompute_mods(&mut self, seat: usize) {
        let c = &self.champs[seat];
        let mut m = StatMods::ZERO;
        for &it in c.items.iter().filter(|&&it| it != items::NONE) {
            m.add(&ITEMS[usize::from(it)].mods);
        }
        for &a in c.augments() {
            m.add(&AUGMENTS[usize::from(a)].mods);
        }
        m.add(&self.headliner().mods);
        self.champs[seat].mods = m;
    }

    pub(crate) fn buy(&mut self, seat: usize, item: u8) {
        if usize::from(item) >= ITEMS.len() || !self.in_shop(seat) {
            return;
        }
        let cost = ITEMS[usize::from(item)].cost as f32;
        let c = &mut self.champs[seat];
        let Some(slot) = c.items.iter().position(|&it| it == items::NONE) else { return };
        if c.gold < cost {
            return;
        }
        c.gold -= cost;
        c.items[slot] = item;
        self.recompute_mods(seat);
    }

    pub(crate) fn sell(&mut self, seat: usize, inv: u8) {
        if usize::from(inv) >= items::SLOTS || !self.in_shop(seat) {
            return;
        }
        let c = &mut self.champs[seat];
        let it = c.items[usize::from(inv)];
        if it == items::NONE {
            return;
        }
        c.gold += ITEMS[usize::from(it)].cost as f32 * items::SELL_SHARE;
        c.items[usize::from(inv)] = items::NONE;
        self.recompute_mods(seat);
    }

    // ---- augments ------------------------------------------------------------------------

    /// An offer for `seat` on its screen: an augment of the screen's tier it hasn't got and isn't
    /// offered, drawn from the seed, the seat, the screen and the offer, so nothing else that
    /// happened in the match moves it.
    fn draw_offer(&self, seat: usize, card: usize, salt: u32) -> u8 {
        let c = &self.champs[seat];
        let screen = usize::from(c.screens_done);
        let tier = self.aug_tiers[screen.min(3)];
        let taken = |a: u8| c.augments().contains(&a) || (c.screen.open && c.screen.offers.contains(&a));
        let key = self.offer_key(seat, screen.min(3));
        let n = AUGMENTS.len();
        let start = math::hash32(key, (screen as u32) << 8 | (card as u32) << 4 | salt) as usize % n;
        let fits =
            |a: usize, tier_only: bool| (!tier_only || AUGMENTS[a].tier as u8 == tier) && !taken(a as u8);
        for tier_only in [true, false] {
            for k in 0..n {
                let a = (start + k) % n;
                if fits(a, tier_only) {
                    return a as u8;
                }
            }
        }
        (start % n) as u8
    }

    /// The key `seat`'s offers on `screen` are drawn from: known (drawn from the server's secret, or
    /// read from the record), or else the seed's.
    fn offer_key(&self, seat: usize, screen: usize) -> u32 {
        if self.offer_known[seat] & (1 << screen) != 0 {
            return self.offer_keys[seat][screen];
        }
        (self.seed as u32) ^ (self.seed >> 32) as u32 ^ (seat as u32).wrapping_mul(0x9E37_79B9)
    }

    /// As `seat`'s `screen` opens: with the server's secret, its key is drawn now, and noted for
    /// the record, which is the first anyone else hears of it.
    fn settle_offer_key(&mut self, seat: usize, screen: usize) {
        let Some(secret) = self.offer_secret else { return };
        if self.offer_known[seat] & (1 << screen) != 0 {
            return;
        }
        let key = math::hash32(secret as u32 ^ (secret >> 32) as u32, (seat as u32) << 8 | screen as u32);
        self.offer_keys[seat][screen] = key;
        self.offer_known[seat] |= 1 << screen;
        let n = usize::from(self.keys_drawn_n);
        if n < self.keys_drawn.len() {
            self.keys_drawn[n] = (seat as u8, screen as u8, key);
            self.keys_drawn_n += 1;
        }
    }

    /// The server's secret for this match's augment offers (set after `reset`).
    pub fn set_offer_secret(&mut self, secret: u64) {
        self.offer_secret = Some(secret);
    }

    /// A key the record gives: `seat`'s offers on `screen` are drawn from `key`.
    pub fn give_offer_key(&mut self, seat: u8, screen: u8, key: u32) {
        let (seat, screen) = (usize::from(seat), usize::from(screen));
        if seat < SEATS && screen < 4 {
            self.offer_keys[seat][screen] = key;
            self.offer_known[seat] |= 1 << screen;
        }
    }

    /// The keys the last tick drew from the secret: seat, screen, key.
    pub fn keys_drawn(&self) -> &[(u8, u8, u32)] {
        &self.keys_drawn[..usize::from(self.keys_drawn_n)]
    }

    fn open_screen(&mut self, seat: usize) {
        let c = self.champs[seat];
        if c.screen.open
            || c.screens_done >= c.screens_earned
            || usize::from(c.screens_done) >= AUGMENT_LEVELS.len()
        {
            return;
        }
        self.settle_offer_key(seat, usize::from(c.screens_done));
        self.champs[seat].screen = crate::game::Screen {
            open: true,
            tier: self.aug_tiers[usize::from(c.screens_done)],
            offers: [crate::content::augments::NONE; 3],
            rerolled: 0,
        };
        for card in 0..3 {
            let a = self.draw_offer(seat, card, 0);
            self.champs[seat].screen.offers[card] = a;
        }
    }

    pub(crate) fn pick_augment(&mut self, seat: usize, card: u8) {
        let c = self.champs[seat];
        if !c.screen.open || card > 2 || !self.in_shop(seat) || usize::from(c.n_aug) >= c.augments.len() {
            return;
        }
        let a = c.screen.offers[usize::from(card)];
        let c = &mut self.champs[seat];
        c.augments[usize::from(c.n_aug)] = a;
        c.n_aug += 1;
        c.screen.open = false;
        c.screens_done += 1;
        self.recompute_mods(seat);
        self.event(Event::Augment { unit: seat as u16, augment: a });
        self.open_screen(seat);
    }

    /// Gives `seat` augment `a` outright, past its screens (tests and tools; while there is room).
    pub fn grant_augment(&mut self, seat: usize, a: u8) {
        let c = &mut self.champs[seat];
        if usize::from(a) >= AUGMENTS.len() || usize::from(c.n_aug) >= c.augments.len() {
            return;
        }
        c.augments[usize::from(c.n_aug)] = a;
        c.n_aug += 1;
        self.recompute_mods(seat);
    }

    pub(crate) fn reroll_augment(&mut self, seat: usize, card: u8) {
        let c = self.champs[seat];
        if !c.screen.open || card > 2 || c.screen.rerolled & (1 << card) != 0 {
            return;
        }
        let a = self.draw_offer(seat, usize::from(card), 1 + u32::from(card));
        let s = &mut self.champs[seat].screen;
        s.offers[usize::from(card)] = a;
        s.rerolled |= 1 << card;
    }

    /// Passive gold, and screens that come due.
    pub(crate) fn economy_tick(&mut self) {
        let paid = self.tick >= ticks(PASSIVE_GOLD_FROM);
        let rate = PASSIVE_GOLD_PER_SEC * self.headliner().gold;
        for seat in 0..SEATS {
            if paid {
                self.champs[seat].gold += rate * crate::DT;
            }
            self.open_screen(seat);
        }
    }

    /// Augments that act on their own: pulses, and the ones that answer low health.
    pub(crate) fn augment_ticks(&mut self) {
        let now = self.tick;
        for seat in 0..SEATS {
            if !self.units[seat].alive {
                continue;
            }
            for k in 0..usize::from(self.champs[seat].n_aug) {
                let a = &AUGMENTS[usize::from(self.champs[seat].augments[k])];
                match a.trigger {
                    Trigger::Pulse { period, radius, ops } if self.champs[seat].aug_ready[k] <= now => {
                        self.champs[seat].aug_ready[k] = now + ticks(period);
                        let team = team_of_seat(seat);
                        let at = self.units[seat].pos;
                        for j in 0..self.units.len() {
                            let u = &self.units[j];
                            let reach = radius + u.radius;
                            if !u.kind.is_structure()
                                && (u.pos - at).length_squared() <= reach * reach
                                && self.attackable(team, j)
                            {
                                self.apply_ops(
                                    seat,
                                    j,
                                    ops,
                                    0,
                                    Source::Augment(self.champs[seat].augments[k]),
                                );
                            }
                        }
                    }
                    Trigger::LowHealth { below, cd, ops } if self.champs[seat].aug_ready[k] <= now => {
                        let u = &self.units[seat];
                        if u.hp < u.stats.max_hp * below {
                            self.champs[seat].aug_ready[k] = now + ticks(cd);
                            self.apply_ops(
                                seat,
                                seat,
                                ops,
                                0,
                                Source::Augment(self.champs[seat].augments[k]),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// An ability of `caster`'s hit `target`: its augments that answer that.
    pub(crate) fn ability_hit(&mut self, caster: usize, target: usize) {
        if caster >= SEATS {
            return;
        }
        let now = self.tick;
        for k in 0..usize::from(self.champs[caster].n_aug) {
            if let Trigger::AbilityHit { cd, ops } =
                AUGMENTS[usize::from(self.champs[caster].augments[k])].trigger
                && self.champs[caster].aug_ready[k] <= now
                && self.units[target].alive
            {
                self.champs[caster].aug_ready[k] = now + ticks(cd);
                self.apply_ops(caster, target, ops, 0, Source::Augment(self.champs[caster].augments[k]));
            }
        }
    }

    /// `seat` cast one of its four abilities: its augments that answer that.
    pub(crate) fn cast_triggers(&mut self, seat: usize) {
        let now = self.tick;
        for k in 0..usize::from(self.champs[seat].n_aug) {
            if let Trigger::Cast { cd, ops } = AUGMENTS[usize::from(self.champs[seat].augments[k])].trigger
                && self.champs[seat].aug_ready[k] <= now
            {
                self.champs[seat].aug_ready[k] = now + ticks(cd);
                self.apply_ops(seat, seat, ops, 0, Source::Augment(self.champs[seat].augments[k]));
            }
        }
    }

    /// An auto attack of `seat`'s landed on `target`.
    pub(crate) fn auto_hit_triggers(&mut self, seat: usize, target: usize) {
        self.champs[seat].auto_hits = self.champs[seat].auto_hits.wrapping_add(1);
        let n = self.champs[seat].auto_hits;
        for k in 0..usize::from(self.champs[seat].n_aug) {
            if let Trigger::AutoHit { every, ops } =
                AUGMENTS[usize::from(self.champs[seat].augments[k])].trigger
                && n.is_multiple_of(u16::from(every.max(1)))
                && self.units[target].alive
            {
                self.apply_ops(seat, target, ops, 0, Source::Augment(self.champs[seat].augments[k]));
            }
        }
    }

    /// `seat` took part in a kill.
    pub(crate) fn takedown(&mut self, seat: usize) {
        for k in 0..usize::from(self.champs[seat].n_aug) {
            if let Trigger::Takedown { ops } = AUGMENTS[usize::from(self.champs[seat].augments[k])].trigger {
                let ops: &[Op] = ops;
                if self.units[seat].alive {
                    self.apply_ops(seat, seat, ops, 0, Source::Augment(self.champs[seat].augments[k]));
                }
            }
        }
    }
}
