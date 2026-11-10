//! Signed calls to Elo Pros' API (`https://elopros.com/api`), blocking: run them through
//! `tokio::task::spawn_blocking`, never on the lobby's tick.

use std::time::Duration;

use super::key::ServerKey;

pub struct Api {
    /// The API's root, without a trailing slash (`https://elopros.com/api`).
    pub base: String,
    pub key: ServerKey,
    agent: ureq::Agent,
}

/// An answer: its status and body (a refusal is an answer, not an error).
pub struct Answer {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Answer {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }

    /// What the body says was wrong, or the status.
    pub fn why(&self) -> String {
        let v = self.json();
        ["why", "error", "detail"]
            .iter()
            .find_map(|k| v.get(*k).and_then(|x| x.as_str()).map(str::to_owned))
            .unwrap_or_else(|| format!("HTTP {}", self.status))
    }

    /// Worth sending again: the server's trouble or a rate limit, not a refusal of what was sent.
    pub fn retry(&self) -> bool {
        self.status >= 500 || self.status == 408 || self.status == 429
    }
}

impl Api {
    pub fn new(base: &str, key: ServerKey) -> Api {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(20)))
            .user_agent(concat!("el-server/", env!("CARGO_PKG_VERSION")))
            .build();
        Api { base: base.trim_end_matches('/').to_string(), key, agent: ureq::Agent::new_with_config(config) }
    }

    /// `POST <base><path>` with `body`, signed. `path` is the route without `/api`.
    pub fn post(&self, path: &str, body: &[u8]) -> Result<Answer, String> {
        let mut req =
            self.agent.post(format!("{}{path}", self.base)).header("content-type", "application/json");
        for (k, v) in self.key.headers("POST", path, body, crate::signin::unix_now(), None) {
            req = req.header(k, v);
        }
        Self::answer(req.send(body))
    }

    /// `GET <base><path>`, unsigned (the reads are public).
    pub fn get(&self, path: &str) -> Result<Answer, String> {
        Self::answer(self.agent.get(format!("{}{path}", self.base)).call())
    }

    fn answer(got: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<Answer, String> {
        let mut resp = got.map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().with_config().limit(1 << 20).read_to_vec().map_err(|e| e.to_string())?;
        Ok(Answer { status, body })
    }
}
