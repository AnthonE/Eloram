//! A bug report for Elo Pros' intake (`POST /api/bugs`, `meter/bugs.py`): the player's words, the
//! build, an optional screenshot, and for a crash the backtrace they chose to send. After Gates'
//! `report.rs`; checked here to the intake's own rules, so a report isn't refused for its shape.

use serde_json::{Value, json};

/// The intake.
pub fn url() -> String {
    super::api("/bugs")
}

/// A one-line field's most (the intake's 300, less room).
pub const TITLE_MAX: usize = 120;
pub const TEXT_MAX: usize = 4_000;
pub const TEXT_LINES_MAX: usize = 60;
/// A screenshot bigger than this is left off rather than sent to be refused.
pub const SHOT_MAX: usize = 7 << 20;
/// A backtrace's most (the intake's 256 KiB).
pub const DUMP_MAX: usize = 250 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Look,
    World,
    Net,
    Numbers,
    Other,
    Crash,
}

impl Kind {
    /// What the panel offers (a crash is offered by itself, after one).
    pub const OFFERED: [Kind; 5] = [Kind::Look, Kind::World, Kind::Net, Kind::Numbers, Kind::Other];

    pub fn slug(self) -> &'static str {
        match self {
            Kind::Look => "look",
            Kind::World => "world",
            Kind::Net => "net",
            Kind::Numbers => "numbers",
            Kind::Other => "other",
            Kind::Crash => "crash",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Look => "LOOKS WRONG",
            Kind::World => "PLAYS WRONG",
            Kind::Net => "LAG OR DROPS",
            Kind::Numbers => "WRONG NUMBERS",
            Kind::Other => "SOMETHING ELSE",
            Kind::Crash => "A CRASH",
        }
    }

    /// The doc a reader of this kind of report reads first.
    pub fn read_first(self) -> &'static str {
        match self {
            Kind::Look => "docs/ART.md",
            Kind::World | Kind::Numbers => "docs/DESIGN.md",
            Kind::Net => "docs/NETCODE.md",
            Kind::Other | Kind::Crash => "docs/ROADMAP.md",
        }
    }
}

/// Where the report comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    /// The build's name (`0.1.0-g48d37a3c7`): version and commit.
    pub name: String,
    /// `linux`, `windows`, `macos` or `web`.
    pub platform: &'static str,
    /// `desktop` or `browser`.
    pub runtime: &'static str,
}

impl Build {
    /// `(version, commit)` from a build name, each in the intake's alphabet.
    fn parts(&self) -> (String, String) {
        let ok = |s: &str| -> String {
            let t: String =
                s.chars().filter(|c| c.is_ascii_alphanumeric() || "._+-".contains(*c)).take(40).collect();
            if t.is_empty() { "unknown".into() } else { t }
        };
        match self.name.split_once("-g") {
            Some((v, c)) => (ok(v), ok(c)),
            None => (ok(&self.name), "unknown".into()),
        }
    }
}

/// Characters the intake refuses in a line: controls and the invisible ones.
fn unseen(c: char) -> bool {
    c.is_control()
        || ('\u{200b}'..='\u{200f}').contains(&c)
        || ('\u{202a}'..='\u{202e}').contains(&c)
        || ('\u{2060}'..='\u{2064}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
        || c == '\u{feff}'
}

/// One line of at most `max` characters, spaces for breaks.
pub fn one_line(s: &str, max: usize) -> String {
    let flat: String =
        s.chars().map(|c| if c == '\n' || c == '\t' { ' ' } else { c }).filter(|c| !unseen(*c)).collect();
    flat.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

/// At most `max_lines` lines of at most `max` characters in all.
pub fn many_lines(s: &str, max: usize, max_lines: usize) -> String {
    let lines: Vec<String> = s
        .lines()
        .take(max_lines)
        .map(|l| {
            l.chars().map(|c| if c == '\t' { ' ' } else { c }).filter(|c| !unseen(*c)).collect::<String>()
        })
        .collect();
    lines.join("\n").trim_end().chars().take(max).collect()
}

/// The report's fingerprint: what groups the same trouble from many players (FNV-1a over its
/// kind, the build, and a crash's place).
pub fn fingerprint(kind: Kind, build: &Build, crash_at: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for s in [kind.slug(), build.name.as_str(), build.platform, build.runtime, crash_at] {
        for b in s.as_bytes() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// `YYYY-MM-DDTHH:MM:SSZ` of unix second `at`.
pub fn iso(at: u64) -> String {
    let day = super::friends::utc_day(at);
    let s = at % 86_400;
    format!("{day}T{:02}:{:02}:{:02}Z", s / 3600, s / 60 % 60, s % 60)
}

/// What the player wrote, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub kind: Kind,
    pub title: String,
    pub text: String,
    pub build: Build,
    pub at: u64,
    /// For a crash: where it happened (the panic's place).
    pub crash_at: String,
    /// What the player was doing (`menu`, `playing`...).
    pub phase: String,
}

impl Report {
    pub fn fingerprint(&self) -> String {
        fingerprint(self.kind, &self.build, &self.crash_at)
    }

    /// The report object the intake checks.
    pub fn json(&self) -> Value {
        let (version, commit) = self.build.parts();
        let title = one_line(&self.title, TITLE_MAX);
        let title = if title.is_empty() { self.kind.label().to_lowercase() } else { title };
        let mut r = json!({
            "format": 1,
            "kind": self.kind.slug(),
            "fingerprint": self.fingerprint(),
            "issue_title": one_line(&format!("Eloram: {title}"), 300),
            "read_first": self.kind.read_first(),
            "reported": iso(self.at),
            "build": {"version": version, "commit": commit, "platform": self.build.platform, "runtime": self.build.runtime},
            "title": title,
            "text": many_lines(&self.text, TEXT_MAX, TEXT_LINES_MAX),
            "place": {"phase": one_line(&self.phase, 40)},
        });
        if self.kind == Kind::Crash {
            r["crash"] = json!({"location": one_line(&self.crash_at, 200)});
        }
        r
    }

    /// The body POSTed: the report, the screenshot if it fits, and a crash's backtrace (only
    /// when the player chose to send it: that choice is the `consent`).
    pub fn submission(&self, shot_png: Option<&[u8]>, backtrace: Option<&[u8]>) -> Vec<u8> {
        use base64::Engine as _;
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        let mut body = json!({"game": super::GAME, "report": self.json()});
        if let Some(png) = shot_png.filter(|p| !p.is_empty() && p.len() <= SHOT_MAX) {
            body["shot_png_b64"] = b64(png).into();
        }
        if self.kind == Kind::Crash
            && let Some(bt) = backtrace.filter(|b| !b.is_empty())
        {
            let bt = &bt[..bt.len().min(DUMP_MAX)];
            body["dump"] = json!({"format": "backtrace", "b64": b64(bt), "consent": true});
        }
        body.to_string().into_bytes()
    }
}

/// The intake's answer: its id for the report, or why not.
pub fn receipt(bytes: &[u8]) -> Result<String, String> {
    let v: Value = serde_json::from_slice(bytes).map_err(|_| "the answer didn't read".to_string())?;
    match v["id"].as_str() {
        Some(id) => Ok(one_line(id, 40)),
        None => Err(v["error"].as_str().map_or_else(|| "no id in the answer".into(), |e| one_line(e, 100))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build() -> Build {
        Build { name: "0.1.0-g48d37a3c7-dirty".into(), platform: "linux", runtime: "desktop" }
    }

    /// The intake's own checks (`meter/bugs.py` `report_error`), as far as a report's shape goes.
    fn intake_takes(r: &Value) -> Result<(), String> {
        let one = |k: &str| r[k].as_str().is_some_and(|s| s.len() <= 300 && !s.chars().any(unseen));
        let ver = |s: &str| {
            !s.is_empty()
                && s.len() <= 40
                && s.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c))
        };
        (r["format"] == 1).then_some(()).ok_or("format")?;
        ["look", "world", "verb", "net", "numbers", "crash", "other"]
            .contains(&r["kind"].as_str().unwrap_or(""))
            .then_some(())
            .ok_or("kind")?;
        let fp = r["fingerprint"].as_str().unwrap_or("");
        (fp.len() == 16 && fp.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()))
            .then_some(())
            .ok_or("fingerprint")?;
        for k in ["issue_title", "read_first", "reported"] {
            one(k).then_some(()).ok_or(k)?;
        }
        ver(r["build"]["version"].as_str().unwrap_or("")).then_some(()).ok_or("version")?;
        ver(r["build"]["commit"].as_str().unwrap_or("")).then_some(()).ok_or("commit")?;
        ["linux", "windows", "macos", "web"]
            .contains(&r["build"]["platform"].as_str().unwrap_or(""))
            .then_some(())
            .ok_or("platform")?;
        ["desktop", "browser", "agent"]
            .contains(&r["build"]["runtime"].as_str().unwrap_or(""))
            .then_some(())
            .ok_or("runtime")?;
        Ok(())
    }

    #[test]
    fn a_report_is_what_the_intake_takes() {
        let r = Report {
            kind: Kind::Look,
            title: "the turret\u{202e} glows\npink".into(),
            text: "line one\n\u{200b}line two\r\n".into(),
            build: build(),
            at: 1_761_003_600,
            crash_at: String::new(),
            phase: "playing".into(),
        };
        let j = r.json();
        intake_takes(&j).unwrap();
        assert_eq!(j["title"], "the turret glows pink");
        assert_eq!(j["build"]["version"], "0.1.0");
        assert_eq!(j["build"]["commit"], "48d37a3c7-dirty");
        assert_eq!(j["reported"], "2025-10-20T23:40:00Z");
        let body: Value = serde_json::from_slice(&r.submission(Some(b"\x89PNG...."), Some(b"bt"))).unwrap();
        assert_eq!(body["game"], "eloram");
        assert!(body["shot_png_b64"].is_string());
        assert!(body.get("dump").is_none(), "a backtrace only with a crash");
        let crash = Report { kind: Kind::Crash, crash_at: "src/x.rs:1:2".into(), ..r.clone() };
        let body: Value = serde_json::from_slice(&crash.submission(None, Some(b"panicked at"))).unwrap();
        assert_eq!(body["dump"]["consent"], true);
        intake_takes(&body["report"]).unwrap();
        assert_ne!(crash.fingerprint(), r.fingerprint());
        assert_eq!(Report { title: String::new(), ..r }.json()["title"], "looks wrong");
    }

    #[test]
    fn the_answer_reads() {
        assert_eq!(receipt(br#"{"ok":true,"id":"b-12","seen":1}"#).unwrap(), "b-12");
        assert_eq!(receipt(br#"{"ok":false,"error":"not today"}"#).unwrap_err(), "not today");
    }
}
