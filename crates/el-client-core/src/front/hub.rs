//! The pages that live on Elo Pros, not in the game (NEWS, STORE): where each is, said honestly
//! in every state, and opened in the player's browser (the launcher's `open` on the desktop, a new
//! tab in the browser). After Gates' `ui/hub.rs`: a store never takes a payment in a game's window.

/// A page the menu hands off to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    News,
    Store,
}

impl Section {
    pub fn label(self) -> &'static str {
        match self {
            Section::News => "NEWS",
            Section::Store => "STORE",
        }
    }

    /// What the page is, in the panel.
    pub fn blurb(self) -> &'static str {
        match self {
            Section::News => "Eloram's updates and patch notes, on its page on Elo Pros.",
            Section::Store => {
                "Eloram sells nothing: no skins, no coin, nothing that changes a match. Its page on Elo Pros is where the listing lives."
            }
        }
    }

    /// Why it opens elsewhere.
    pub fn why_elsewhere(self) -> &'static str {
        match self {
            Section::News => "News lives on Elo Pros, where the game is listed: it opens in your browser.",
            Section::Store => {
                "A game never takes a payment in its own window: the store opens in your browser, where your wallet is."
            }
        }
    }
}

/// Where a section is, as far as the menu knows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// The desktop with no Elo launcher running: nothing can be opened.
    NoLauncher,
    /// The title's manifest is on its way.
    Fetching,
    /// The manifest couldn't be read: why.
    NoManifest(String),
    /// The manifest names no link for it.
    NoLink,
    Ready(String),
}

impl Status {
    /// The status line, distinct in every state.
    pub fn line(&self) -> String {
        match self {
            Status::NoLauncher => "The Elo launcher isn't running: start Eloram from it to open this.".into(),
            Status::Fetching => "Asking Elo Pros where this lives...".into(),
            Status::NoManifest(why) => format!("Elo Pros didn't say where this lives: {why}"),
            Status::NoLink => {
                "Elo Pros names no page for this yet. Nothing is wrong: it isn't published.".into()
            }
            Status::Ready(url) => url.clone(),
        }
    }

    pub fn key(&self) -> &'static str {
        match self {
            Status::NoLauncher => "no_launcher",
            Status::Fetching => "fetching",
            Status::NoManifest(_) => "no_manifest",
            Status::NoLink => "no_link",
            Status::Ready(_) => "ready",
        }
    }

    /// A section's state: no launcher (on the desktop) comes first, then the manifest's answer.
    pub fn of(
        launcher: bool,
        manifest: Option<&Result<super::manifest::Manifest, String>>,
        s: Section,
    ) -> Status {
        if !launcher {
            return Status::NoLauncher;
        }
        match manifest {
            None => Status::Fetching,
            Some(Err(e)) => Status::NoManifest(e.clone()),
            Some(Ok(m)) => match m.url(s) {
                Some(u) => Status::Ready(u.to_string()),
                None => Status::NoLink,
            },
        }
    }
}

/// What opening it came to, in words.
pub fn opened_line(s: Section, opened: bool, refused: bool) -> String {
    match (opened, refused) {
        (true, _) => format!("{} is open in your browser.", title(s)),
        (false, true) => format!("{} didn't open: the browser or the launcher said no.", title(s)),
        (false, false) => "The Elo launcher isn't running: nothing was opened.".into(),
    }
}

fn title(s: Section) -> &'static str {
    match s {
        Section::News => "The news",
        Section::Store => "The store",
    }
}

#[cfg(test)]
mod tests {
    use super::super::manifest::Manifest;
    use super::*;

    #[test]
    fn no_launcher_wins_and_every_state_says_something_different() {
        let m: Result<Manifest, String> =
            Manifest::parse(br#"{"news":{"url":"https://elopros.com/games/eloram/news/"}}"#);
        assert_eq!(Status::of(false, Some(&m), Section::News), Status::NoLauncher);
        assert!(matches!(Status::of(true, Some(&m), Section::News), Status::Ready(_)));
        assert_eq!(Status::of(true, Some(&m), Section::Store), Status::NoLink);
        assert_eq!(Status::of(true, None, Section::Store), Status::Fetching);
        let lines: Vec<String> = [
            Status::NoLauncher,
            Status::Fetching,
            Status::NoManifest("404".into()),
            Status::NoLink,
            Status::Ready("https://x.example/".into()),
        ]
        .iter()
        .map(Status::line)
        .collect();
        let mut d = lines.clone();
        d.sort();
        d.dedup();
        assert_eq!(d.len(), lines.len());
        assert!(Section::Store.blurb().contains("sells nothing"));
        assert!(lines.iter().all(|l| l.is_ascii()));
    }
}
