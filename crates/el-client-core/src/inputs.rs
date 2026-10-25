//! History of the commands this client sent, by tick (for prediction replay and redundancy).

use el_proto::InputCmd;

const HISTORY: usize = 128;

#[derive(Clone, Debug)]
pub struct InputHistory {
    cmds: [InputCmd; HISTORY],
    valid: [bool; HISTORY],
    pub newest: u32,
}

impl Default for InputHistory {
    fn default() -> Self {
        Self { cmds: [InputCmd::default(); HISTORY], valid: [false; HISTORY], newest: 0 }
    }
}

impl InputHistory {
    pub fn push(&mut self, cmd: InputCmd) {
        let k = cmd.tick as usize % HISTORY;
        self.cmds[k] = cmd;
        self.valid[k] = true;
        self.newest = self.newest.max(cmd.tick);
    }

    pub fn get(&self, tick: u32) -> Option<InputCmd> {
        let k = tick as usize % HISTORY;
        (self.valid[k] && self.cmds[k].tick == tick).then_some(self.cmds[k])
    }

    /// The newest command for `tick` or before it that is still held: the one the server repeats
    /// through a gap after it.
    pub fn last_at_or_before(&self, tick: u32) -> Option<InputCmd> {
        (0..HISTORY as u32).map_while(|back| tick.checked_sub(back)).find_map(|t| self.get(t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(tick: u32) -> InputCmd {
        InputCmd { tick, ..InputCmd::default() }
    }

    #[test]
    fn finds_the_command_a_gap_repeats() {
        let mut h = InputHistory::default();
        for t in [10, 11, 12, 20] {
            h.push(cmd(t));
        }
        assert_eq!(h.last_at_or_before(12).map(|c| c.tick), Some(12));
        assert_eq!(h.last_at_or_before(15).map(|c| c.tick), Some(12));
        assert_eq!(h.last_at_or_before(25).map(|c| c.tick), Some(20));
        assert_eq!(h.last_at_or_before(9), None);
        // Overwritten slots don't count: tick 10's slot now holds 138.
        h.push(cmd(138));
        assert_eq!(h.last_at_or_before(10), None);
    }
}
