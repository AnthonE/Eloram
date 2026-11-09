//! What the browser gives the page and the desktop finds for itself, behind one set of names: the
//! clock, a place to keep the settings and the tokens, HTTP, opening a page elsewhere, starting
//! over, and state the network's tasks share with the frame.
//!
//! In the browser these are the window's (`performance.now`, `localStorage`, `sessionStorage`,
//! `fetch`, `window.open`, `location.reload`); on the desktop a monotonic clock, a settings file
//! and secret files under the player's config directory, ureq on a thread, the Elo launcher's
//! `open`, and the program started again.

use std::sync::{Arc, Mutex};

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::*;

#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

/// The clock everything runs on, s: the client core, the datagram pump, the HUD's timers.
pub fn now_s() -> f64 {
    now_ms() / 1_000.0
}

/// What an HTTP request came back with: its body, or why there is none.
pub type Answer = Result<Vec<u8>, String>;

/// Where a request's answer lands, for a frame to take when it's there.
#[derive(Clone, Default)]
pub struct Slot(Arc<Mutex<Option<Answer>>>);

impl Slot {
    /// The answer, once (`None` while it's on its way, or after it was taken).
    pub fn take(&self) -> Option<Answer> {
        self.0.lock().ok().and_then(|mut a| a.take())
    }

    pub(crate) fn put(&self, a: Answer) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(a);
        }
    }
}

/// A request: GET, or POST with a JSON body; an answer longer than `cap` is refused.
#[derive(Clone, Debug)]
pub struct Req {
    pub url: String,
    pub post: Option<Vec<u8>>,
    pub cap: usize,
}

impl Req {
    pub fn get(url: impl Into<String>) -> Self {
        Self { url: url.into(), post: None, cap: 256 * 1024 }
    }

    pub fn post(url: impl Into<String>, json: Vec<u8>) -> Self {
        Self { url: url.into(), post: Some(json), cap: 256 * 1024 }
    }

    pub fn cap(mut self, cap: usize) -> Self {
        self.cap = cap;
        self
    }
}

/// What opening a page elsewhere came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opened {
    Opened,
    /// The browser blocked the window, or the launcher said no.
    Refused,
    /// The desktop has no launcher to open it in (a game never opens pages behind its back).
    NoLauncher,
}

/// The settings a page (or a desktop's file) keeps that are secrets: never in the settings file,
/// never logged.
pub const SECRET_KEYS: [&str; 2] = ["el.resume", "el.rejoin"];
