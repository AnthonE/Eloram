//! Generational handles into fixed-capacity pools.

/// Slot index plus a generation counter that changes every time the slot is reused, so a stale
/// handle can never alias a new occupant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Handle {
    pub idx: u16,
    pub generation: u16,
}

impl Handle {
    pub const NONE: Handle = Handle { idx: u16::MAX, generation: 0 };

    #[inline]
    pub fn is_none(self) -> bool {
        self.idx == u16::MAX
    }
}
