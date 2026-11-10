//! Who is playing, as the identity chip says it: a name from Elo Pros (`/api/face/<id>`), else the
//! wallet shortened, else GUEST; and the line under it.

/// How the player got here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    Guest,
    /// Signed in in the browser (a wallet, a Solana wallet, or an Elo Pros pass).
    Browser,
    /// Signed in through the Elo launcher.
    Launcher,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Who {
    /// The id the server knows the player by (`0x...`, base58, or `acct_...`).
    pub id: Option<String>,
    pub source: Source,
    /// The name Elo Pros has for them, once asked.
    pub name: Option<String>,
    /// The launcher's streamer mode (or `?streamer=1`): no name, no wallet on the screen.
    pub streamer: bool,
}

/// `0x1234...abcd`: the first six and the last four.
pub fn short(id: &str) -> String {
    if id.len() <= 12 || !id.is_ascii() {
        return id.to_string();
    }
    format!("{}...{}", &id[..6], &id[id.len() - 4..])
}

/// A player's profile on Elo Pros (friends are there too).
pub fn profile_url(id: &str) -> String {
    let safe: String = id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    format!("https://elopros.com/profile.html?who={safe}")
}

impl Who {
    /// A wallet that may queue ranked and be rated (an account alone may not).
    pub fn wallet(&self) -> bool {
        self.id.as_deref().is_some_and(|i| !i.starts_with("acct_"))
    }

    pub fn shown_name(&self) -> String {
        if self.streamer {
            return "STREAMER MODE".into();
        }
        if let Some(n) = self.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            return n.to_string();
        }
        match &self.id {
            Some(id) => short(id),
            None => "GUEST".into(),
        }
    }

    pub fn line(&self) -> String {
        if self.streamer {
            return "name and wallet hidden".into();
        }
        match (&self.id, &self.source) {
            (None, _) => "playing as a guest: sign in to queue ranked".into(),
            (Some(id), Source::Launcher) => format!("{}, signed in with the Elo launcher", short(id)),
            (Some(id), _) if id.starts_with("acct_") => "signed in with Elo Pros, no wallet".into(),
            (Some(id), _) => format!("{}, signed in", short(id)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_then_a_wallet_then_guest() {
        let mut w =
            Who { id: Some("0x24445efddb08d4938e3e3627042b2cf4063d9092".into()), ..Default::default() };
        assert_eq!(w.shown_name(), "0x2444...9092");
        assert!(w.wallet());
        w.name = Some("Ryuzen".into());
        assert_eq!(w.shown_name(), "Ryuzen");
        w.streamer = true;
        assert_eq!(w.shown_name(), "STREAMER MODE");
        assert_eq!(Who::default().shown_name(), "GUEST");
        assert!(!Who { id: Some("acct_0123456789abcdef0123".into()), ..Default::default() }.wallet());
        assert!(Who::default().line().is_ascii());
        assert_eq!(profile_url("0xAb&x=1"), "https://elopros.com/profile.html?who=0xAbx1");
    }
}
