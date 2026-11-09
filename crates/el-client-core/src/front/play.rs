//! What PLAY can do now, and why not when it can't: every state the button can be in, in words.

use super::PlayMode;

/// What the menu knows when it draws PLAY.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayInputs {
    /// Welcomed by the server (the link is up and the Hello answered).
    pub linked: bool,
    /// Still dialling, or signing in.
    pub connecting: bool,
    /// Why the link failed, if it did.
    pub failed: bool,
    /// Signed in as a wallet (not a guest, not a login-only account).
    pub wallet: bool,
    /// The server holds ranked matches (it said so when it welcomed us).
    pub ranked_here: bool,
    /// Already in a queue, and which.
    pub queued: Option<PlayMode>,
    pub mode: PlayMode,
}

/// The go button: its label, whether a click does something, and the line under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayView {
    pub label: &'static str,
    pub ready: bool,
    pub line: String,
}

pub fn play_view(i: PlayInputs) -> PlayView {
    let v = |label, ready, line: &str| PlayView { label, ready, line: line.to_string() };
    if let Some(q) = i.queued {
        return v(
            "LEAVE THE QUEUE",
            true,
            if q == PlayMode::Ranked { "In the ranked queue" } else { "Finding a match" },
        );
    }
    if i.failed {
        return v("RECONNECT", true, "The link to the game server dropped.");
    }
    if i.connecting || !i.linked {
        return v("PLAY", false, "Reaching the game server...");
    }
    match i.mode {
        PlayMode::Normal => v("PLAY", true, "Five a side, bots in the empty seats"),
        PlayMode::Ranked if !i.ranked_here => v("PLAY", false, "This server doesn't hold ranked matches."),
        PlayMode::Ranked if !i.wallet => {
            v("SIGN IN", true, "Ranked is for wallets: sign in with yours to queue.")
        }
        PlayMode::Ranked => v("PLAY", true, "Ten wallets, no bots, rated on Elo Pros"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_says_what_it_is() {
        let base = PlayInputs { linked: true, ranked_here: true, ..Default::default() };
        assert!(play_view(base).ready);
        let ranked = PlayInputs { mode: PlayMode::Ranked, ..base };
        assert_eq!(play_view(ranked).label, "SIGN IN");
        assert_eq!(play_view(PlayInputs { wallet: true, ..ranked }).label, "PLAY");
        assert!(!play_view(PlayInputs { ranked_here: false, wallet: true, ..ranked }).ready);
        assert!(!play_view(PlayInputs { linked: false, connecting: true, ..base }).ready);
        assert_eq!(play_view(PlayInputs { failed: true, ..base }).label, "RECONNECT");
        assert_eq!(
            play_view(PlayInputs { queued: Some(PlayMode::Ranked), ..base }).line,
            "In the ranked queue"
        );
    }
}
