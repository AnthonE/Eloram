//! The main menu's words and decisions (the client's `front/` draws them): the entries and their
//! order, what PLAY can do now and why not, the launcher-backed pages' states, the title's
//! manifest, who the player is, and the records a WATCH page lists. All of it tested here, after
//! Gates' (`AnthonE/Gates`, MIT, MoreRight DAO: `render/menu.rs`'s `Nav`, `ui/hub.rs`,
//! `manifest.rs`, `ui/names.rs`), in Eloram's words.
//!
//! Every word is ASCII: the menu's face (Roboto Condensed) has more, but the same words show up in
//! the HUD's default font too.

pub mod hub;
pub mod manifest;
pub mod play;
pub mod records;
pub mod who;

/// An entry in the nav column, in the order drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Nav {
    Play,
    Practice,
    Watch,
    Friends,
    News,
    Store,
    Profile,
    Options,
    Quit,
}

impl Nav {
    const ALL: [Nav; 9] = [
        Nav::Play,
        Nav::Practice,
        Nav::Watch,
        Nav::Friends,
        Nav::News,
        Nav::Store,
        Nav::Profile,
        Nav::Options,
        Nav::Quit,
    ];

    /// The entries a build shows: QUIT only on the desktop (a page is closed as a tab is).
    pub fn all(desktop: bool) -> impl Iterator<Item = Nav> {
        Self::ALL.into_iter().filter(move |n| desktop || *n != Nav::Quit)
    }

    pub fn label(self) -> &'static str {
        match self {
            Nav::Play => "PLAY",
            Nav::Practice => "PRACTICE",
            Nav::Watch => "WATCH",
            Nav::Friends => "FRIENDS",
            Nav::News => "NEWS",
            Nav::Store => "STORE",
            Nav::Profile => "PROFILE",
            Nav::Options => "OPTIONS",
            Nav::Quit => "QUIT",
        }
    }

    /// A gap before it: the play group, the people, the store's pages, then the settings and the door.
    pub fn gap_before(self) -> bool {
        matches!(self, Nav::Friends | Nav::News | Nav::Options | Nav::Quit)
    }

    /// Whether it opens a page in the pane (OPTIONS opens the settings over it; QUIT leaves).
    pub fn is_pane(self) -> bool {
        !matches!(self, Nav::Options | Nav::Quit)
    }

    /// The name a test says (`{t:"front",nav}`).
    pub fn key(self) -> &'static str {
        match self {
            Nav::Play => "play",
            Nav::Practice => "practice",
            Nav::Watch => "watch",
            Nav::Friends => "friends",
            Nav::News => "news",
            Nav::Store => "store",
            Nav::Profile => "profile",
            Nav::Options => "options",
            Nav::Quit => "quit",
        }
    }

    pub fn parse(s: &str) -> Option<Nav> {
        Self::ALL.into_iter().find(|n| n.key() == s)
    }
}

/// Which queue PLAY joins.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlayMode {
    /// Five a side, bots in the empty seats; unrated.
    #[default]
    Normal,
    /// Ten people with wallets, no bots, through the Elo Pros queue; rated.
    Ranked,
}

impl PlayMode {
    pub fn label(self) -> &'static str {
        match self {
            PlayMode::Normal => "NORMAL",
            PlayMode::Ranked => "RANKED",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            PlayMode::Normal => {
                "Five against five, a random champion each. Bots take the empty seats after half a minute. Unrated."
            }
            PlayMode::Ranked => {
                "Ten people with wallets, no bots, matched by the Elo Pros queue. Rated on Elo Pros, the replay as evidence."
            }
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            PlayMode::Normal => "normal",
            PlayMode::Ranked => "ranked",
        }
    }

    pub fn parse(s: &str) -> Option<PlayMode> {
        match s {
            "normal" => Some(PlayMode::Normal),
            "ranked" => Some(PlayMode::Ranked),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_reads_as_itself_and_quit_is_the_desktops() {
        let web: Vec<Nav> = Nav::all(false).collect();
        let desk: Vec<Nav> = Nav::all(true).collect();
        assert!(!web.contains(&Nav::Quit) && desk.last() == Some(&Nav::Quit));
        let mut labels: Vec<&str> = desk.iter().map(|n| n.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), desk.len());
        for n in desk {
            assert_eq!(Nav::parse(n.key()), Some(n));
            assert!(n.label().is_ascii() && !n.label().is_empty());
        }
        assert!(!Nav::Options.is_pane() && !Nav::Quit.is_pane() && Nav::Play.is_pane());
        for m in [PlayMode::Normal, PlayMode::Ranked] {
            assert_eq!(PlayMode::parse(m.key()), Some(m));
            assert!(m.blurb().is_ascii());
        }
    }
}
