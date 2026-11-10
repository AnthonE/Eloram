//! The records a server keeps (`/records.json`), as WATCH lists them: live matches a minute behind
//! first, then finished ones to replay. Practice is left out.

/// One match WATCH can show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchRow {
    pub id: String,
    pub live: bool,
    /// Minutes played (a tick is 1/30 s).
    pub minutes: u32,
    /// The winning team (0 Dawn, 1 Dusk), once over.
    pub winner: Option<u8>,
    pub ranked: bool,
    /// Every seat's champion, when the server names them.
    pub picks: Vec<u8>,
}

/// Parses `/records.json`'s rows; a row that doesn't read is skipped, practice is left out.
pub fn rows(bytes: &[u8]) -> Vec<WatchRow> {
    let Ok(serde_json::Value::Array(list)) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return Vec::new();
    };
    let mut out: Vec<WatchRow> = list
        .iter()
        .filter(|r| !r.get("practice").and_then(|p| p.as_bool()).unwrap_or(false))
        .filter_map(|r| {
            let id = r.get("id")?.as_str()?;
            if id.is_empty() || id.len() > 32 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            Some(WatchRow {
                id: id.to_string(),
                live: r.get("live").and_then(|v| v.as_bool()).unwrap_or(false),
                minutes: (r.get("ticks").and_then(|v| v.as_u64()).unwrap_or(0) / (30 * 60)) as u32,
                winner: r.get("winner").and_then(|v| v.as_u64()).filter(|w| *w <= 1).map(|w| w as u8),
                ranked: r.get("ranked").and_then(|v| v.as_bool()).unwrap_or(false),
                picks: r
                    .get("picks")
                    .and_then(|p| p.as_array())
                    .map(|a| a.iter().filter_map(|c| c.as_u64().map(|c| c as u8)).collect())
                    .unwrap_or_default(),
            })
        })
        .collect();
    // Live first, the longest-running first among them; then the finished, as the server listed.
    out.sort_by_key(|r| (!r.live, if r.live { u32::MAX - r.minutes } else { 0 }));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_first_practice_left_out_garbage_skipped() {
        let got = rows(
            br#"[{"id":"aa","live":false,"ticks":54000,"winner":1},
                {"id":"bb","live":true,"ticks":18000,"picks":[1,2,3]},
                {"id":"cc","live":true,"ticks":36000,"ranked":true},
                {"id":"dd","live":true,"practice":true},
                {"id":"not hex"},{"live":true}]"#,
        );
        let ids: Vec<&str> = got.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["cc", "bb", "aa"]);
        assert_eq!((got[0].minutes, got[0].ranked), (20, true));
        assert_eq!(got[2].winner, Some(1));
        assert_eq!(got[1].picks, [1, 2, 3]);
        assert!(rows(b"{}").is_empty());
    }
}
