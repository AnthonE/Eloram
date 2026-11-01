//! The death recap, as League's: what hurt the player's champion in the seconds before it fell,
//! by who and with what, each line split physical, magic and true. The client hands it every
//! damage event its champion takes ([`Recap::hit`]); when it falls, [`Recap::sum`] says what killed
//! it.

use el_sim::TICK_HZ;
use el_sim::missile::Source;

/// How far back the recap looks from the fall (s), as League's does.
pub const WINDOW_SECS: u32 = 15;
/// Hits remembered: plenty for the window.
const HITS: usize = 128;
/// Lines shown at most.
pub const LINES: usize = 5;

/// One hit taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub tick: u32,
    /// The unit that dealt it.
    pub source: u8,
    pub amount: u16,
    /// 0 physical, 1 magic, 2 true (`DmgKind`'s order).
    pub kind: u8,
    /// What dealt it (`Source::code`).
    pub from: u16,
}

/// A line of the recap: a unit's damage with one thing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub source: u8,
    pub from: Source,
    /// Physical, magic, true.
    pub split: [u32; 3],
    pub hits: u16,
}

impl Line {
    pub fn total(&self) -> u32 {
        self.split.iter().sum()
    }
}

/// The hits taken lately, oldest first, in a ring.
#[derive(Clone, Debug, Default)]
pub struct Recap {
    hits: Vec<Hit>,
    next: usize,
}

impl Recap {
    pub fn hit(&mut self, h: Hit) {
        if self.hits.len() < HITS {
            self.hits.push(h);
        } else {
            self.hits[self.next] = h;
            self.next = (self.next + 1) % HITS;
        }
    }

    /// What hurt the champion in the window before `fell`, by unit and source, most first, and
    /// the damage taken in all.
    pub fn sum(&self, fell: u32) -> (Vec<Line>, u32) {
        let from = fell.saturating_sub(WINDOW_SECS * TICK_HZ);
        let mut lines: Vec<Line> = Vec::new();
        let mut all = 0u32;
        for h in self.hits.iter().filter(|h| h.tick >= from && h.tick <= fell) {
            let src = Source::from_code(u32::from(h.from));
            let k = usize::from(h.kind.min(2));
            all += u32::from(h.amount);
            match lines.iter_mut().find(|l| l.source == h.source && l.from == src) {
                Some(l) => {
                    l.split[k] += u32::from(h.amount);
                    l.hits += 1;
                }
                None => {
                    let mut split = [0; 3];
                    split[k] = u32::from(h.amount);
                    lines.push(Line { source: h.source, from: src, split, hits: 1 });
                }
            }
        }
        lines.sort_by_key(|l| std::cmp::Reverse(l.total()));
        lines.truncate(LINES);
        (lines, all)
    }

    pub fn clear(&mut self) {
        self.hits.clear();
        self.next = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(tick: u32, source: u8, amount: u16, kind: u8, from: Source) -> Hit {
        Hit { tick, source, amount, kind, from: from.code() as u16 }
    }

    #[test]
    fn sums_by_unit_and_source_most_first_within_the_window() {
        let mut r = Recap::default();
        let q = Source::Ability { champ: 0, slot: 0 };
        // Long before the window: left out.
        r.hit(hit(0, 5, 999, 0, Source::Auto));
        let fell = 100 * TICK_HZ;
        r.hit(hit(fell - 5 * TICK_HZ, 5, 100, 0, Source::Auto));
        r.hit(hit(fell - 4 * TICK_HZ, 5, 120, 0, Source::Auto));
        r.hit(hit(fell - 3 * TICK_HZ, 5, 300, 1, q));
        r.hit(hit(fell - 2 * TICK_HZ, 7, 50, 2, Source::Turret));
        let (lines, all) = r.sum(fell);
        assert_eq!(all, 570);
        assert_eq!(lines.len(), 3);
        assert_eq!((lines[0].source, lines[0].from, lines[0].split), (5, q, [0, 300, 0]));
        assert_eq!((lines[1].from, lines[1].split, lines[1].hits), (Source::Auto, [220, 0, 0], 2));
        assert_eq!((lines[2].source, lines[2].split), (7, [0, 0, 50]));
    }

    #[test]
    fn the_ring_keeps_the_newest() {
        let mut r = Recap::default();
        for t in 0..(HITS as u32 * 2) {
            r.hit(hit(t, 1, 1, 0, Source::Auto));
        }
        let (_, all) = r.sum(HITS as u32 * 2);
        assert_eq!(all, HITS as u32);
    }
}
