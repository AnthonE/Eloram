//! Per-seat input jitter buffer: commands arrive a few ticks early (and redundantly); the match
//! takes exactly the one for the tick it is simulating.

/// What the buffer holds: anything stamped with the tick it is for.
pub trait Ticked: Copy + Default {
    fn tick(&self) -> u32;
}

const SLOTS: usize = 64;

#[derive(Clone, Copy)]
pub struct JitterBuffer<T: Ticked> {
    cmds: [T; SLOTS],
    valid: u64,
    /// Newest command tick received.
    pub newest: u32,
}

impl<T: Ticked> Default for JitterBuffer<T> {
    fn default() -> Self {
        Self { cmds: [T::default(); SLOTS], valid: 0, newest: 0 }
    }
}

impl<T: Ticked> JitterBuffer<T> {
    pub fn clear(&mut self) {
        self.valid = 0;
        self.newest = 0;
    }

    /// Stores a command unless it is for a tick already simulated (`next` is the next tick to run)
    /// or absurdly far in the future.
    #[inline]
    pub fn insert(&mut self, cmd: T, next: u32) -> bool {
        let t = cmd.tick();
        if t < next || t >= next + (SLOTS as u32 - 2) {
            return false;
        }
        let k = t as usize % SLOTS;
        self.cmds[k] = cmd;
        self.valid |= 1 << k;
        self.newest = self.newest.max(t);
        true
    }

    /// Removes and returns the command for `tick`, if it arrived.
    #[inline]
    pub fn take(&mut self, tick: u32) -> Option<T> {
        let k = tick as usize % SLOTS;
        if self.valid & (1 << k) != 0 && self.cmds[k].tick() == tick {
            self.valid &= !(1 << k);
            Some(self.cmds[k])
        } else {
            None
        }
    }

    /// Commands held for `tick` and later: the server's buffer gauge, which every snapshot reports
    /// so the client's clock can steer on it (Gates' `buffered_depth`).
    pub fn depth(&self, tick: u32) -> u32 {
        let mut n = 0;
        for k in 0..SLOTS as u32 {
            let i = (tick + k) as usize % SLOTS;
            if self.valid & (1 << i) != 0 && self.cmds[i].tick() >= tick {
                n += 1;
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Default, Debug, PartialEq)]
    struct Cmd(u32, u8);
    impl Ticked for Cmd {
        fn tick(&self) -> u32 {
            self.0
        }
    }

    #[test]
    fn takes_each_tick_once_and_refuses_the_past_and_the_far_future() {
        let mut j = JitterBuffer::<Cmd>::default();
        assert!(j.insert(Cmd(10, 1), 10));
        assert!(j.insert(Cmd(11, 2), 10));
        assert!(j.insert(Cmd(11, 3), 10), "a repeat replaces");
        assert!(!j.insert(Cmd(9, 0), 10));
        assert!(!j.insert(Cmd(10 + 62, 0), 10));
        assert_eq!(j.depth(10), 2);
        assert_eq!(j.take(10), Some(Cmd(10, 1)));
        assert_eq!(j.take(10), None);
        assert_eq!(j.take(11), Some(Cmd(11, 3)));
        assert_eq!(j.depth(12), 0);
        assert_eq!(j.newest, 11);
    }
}
