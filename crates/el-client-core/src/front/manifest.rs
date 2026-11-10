//! The title's manifest on Elo Pros (`/api/launcher/manifest/eloram`, or where the launcher's
//! `title` verb says): the menu reads only where its NEWS and STORE go, each `{url, label}`.
//! Refused rather than truncated past its cap; a link that isn't `https://` to a host isn't one.

use super::hub::Section;

/// The most a manifest may be (it carries the builds and the notary's note besides).
pub const MAX_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Link {
    pub url: Option<String>,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    pub news: Link,
    pub store: Link,
    /// The server list's url.
    pub servers: Option<String>,
}

/// An `https://` url naming a host, at most 256 bytes, with nothing a browser would trip on.
pub fn check_url(u: &str) -> bool {
    let Some(rest) = u.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    u.len() <= 256 && !host.is_empty() && !u.chars().any(|c| c.is_whitespace() || c.is_control())
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Manifest, String> {
        if bytes.len() > MAX_BYTES {
            return Err(format!("the manifest is {} bytes, past {MAX_BYTES}", bytes.len()));
        }
        let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| format!("not JSON: {e}"))?;
        let obj = v.as_object().ok_or("not a JSON object")?;
        let link = |k: &str| {
            let s = obj.get(k);
            Link {
                url: s
                    .and_then(|s| s.get("url"))
                    .and_then(|u| u.as_str())
                    .filter(|u| check_url(u))
                    .map(str::to_owned),
                label: s
                    .and_then(|s| s.get("label"))
                    .and_then(|l| l.as_str())
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && l.len() <= 40)
                    .map(str::to_owned),
            }
        };
        Ok(Manifest {
            news: link("news"),
            store: link("store"),
            servers: obj
                .get("servers")
                .and_then(|s| s.get("url"))
                .and_then(|u| u.as_str())
                .filter(|u| check_url(u))
                .map(str::to_owned),
        })
    }

    pub fn url(&self, s: Section) -> Option<&str> {
        match s {
            Section::News => self.news.url.as_deref(),
            Section::Store => self.store.url.as_deref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_https_to_a_host_and_the_rest_is_ignored() {
        let m = Manifest::parse(
            br#"{"slug":"eloram","news":{"url":"https://elopros.com/games/eloram/news/","label":"News"},
                "store":{"url":"http://elopros.com/"},"servers":{"url":"https://elopros.com/api/launcher/servers/eloram"},
                "builds":[1,2,3]}"#,
        )
        .unwrap();
        assert_eq!(m.url(Section::News), Some("https://elopros.com/games/eloram/news/"));
        assert_eq!(m.news.label.as_deref(), Some("News"));
        assert_eq!(m.url(Section::Store), None, "http is not a link");
        assert!(m.servers.is_some());
        assert!(Manifest::parse(b"[1]").is_err());
        assert!(Manifest::parse(&vec![b' '; MAX_BYTES + 1]).is_err());
        assert!(!check_url("https:///x") && !check_url("https://a b/") && check_url("https://elopros.com"));
    }
}
