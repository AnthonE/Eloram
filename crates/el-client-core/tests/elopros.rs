//! The Elo Pros pieces against the platform's own vectors (copied from AnthonE/scry-forge's
//! `sdk/testdata/`; `scripts/elo-drift.sh` holds the copies to the originals).

use el_client_core::elopros::{face, friends, profile};
use serde_json::{Value, json};

fn data(name: &str) -> Value {
    let path = format!("{}/tests/testdata/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn every_friends_text_is_the_platforms() {
    let v = data("friends_texts.json");
    for (op, key) in v["ops"].as_object().unwrap() {
        assert_eq!(friends::Op::parse(op).unwrap().key(), key.as_str().unwrap(), "{op}");
    }
    let cases = v["cases"].as_array().unwrap();
    assert!(cases.len() >= 8);
    for c in cases {
        let op = friends::Op::parse(c["op"].as_str().unwrap()).unwrap();
        let (me, other, at) =
            (c["wallet"].as_str().unwrap(), c["other"].as_str().unwrap(), c["at"].as_u64().unwrap());
        assert_eq!(friends::utc_day(at), c["day"].as_str().unwrap());
        assert_eq!(friends::detail(op, other, at), c["detail"].as_str().unwrap());
        let text = friends::sign_text(me, op, other, at).unwrap();
        assert_eq!(text, c["text"].as_str().unwrap());
        // What prepare would hand back is taken; anything else isn't signed.
        let offered = json!({"sign_this": text, "at": at, "op": op.name(), "args": c["args"]}).to_string();
        assert_eq!(
            friends::check_offered(me, op, other, at + 5, offered.as_bytes()).unwrap(),
            (text.clone(), at)
        );
        let other_words =
            json!({"sign_this": format!("{text} "), "at": at, "op": op.name(), "args": c["args"]});
        assert!(friends::check_offered(me, op, other, at, other_words.to_string().as_bytes()).is_err());
        assert!(
            friends::check_offered(me, op, other, at + 4000, offered.as_bytes()).is_err(),
            "the clocks too far apart"
        );
        let someone_else = json!({"sign_this": text, "at": at, "op": op.name(), "args": {op.key(): "0x9999999999999999999999999999999999999999"}});
        assert!(friends::check_offered(me, op, other, at, someone_else.to_string().as_bytes()).is_err());
    }
}

#[test]
fn the_platforms_friends_list_reads() {
    let body = data("friends_view.json").to_string();
    let view = friends::parse(body.as_bytes()).unwrap();
    assert!(!view.friends.is_empty());
    assert!(!view.asks_in.is_empty());
    let rows = view.ordered();
    assert!(rows[0].online, "online first");
    assert!(!rows.last().unwrap().online);
    let now = 1_761_003_000;
    for f in &rows {
        let line = friends::presence_line(f, now);
        assert!(!line.is_empty());
        assert!(!f.label().is_empty());
    }
    let offline = rows.iter().find(|f| !f.online && f.last_online.is_some()).unwrap();
    assert!(friends::presence_line(offline, now).starts_with("last seen"));
    // An act's answer carries the list.
    let acted = json!({"ok": true, "op": "accept", "view": data("friends_view.json")}).to_string();
    assert_eq!(friends::parse_act(acted.as_bytes()).unwrap(), view);
}

#[test]
fn a_solana_wallet_keeps_its_case_and_an_account_signs_nothing() {
    let sol = "GmaDrppBC7P5ARKV8g3djiwP89vz1jLK23V2GBjuAEGB";
    let text = friends::sign_text(
        "0xABCDEFabcdefABCDEFabcdefABCDEFabcdefABCD",
        friends::Op::Ask,
        sol,
        1_761_003_600,
    )
    .unwrap();
    assert!(text.contains("wallet: 0xabcdefabcdefabcdefabcdefabcdefabcdefabcd"));
    assert!(text.contains(sol), "base58 as written");
    assert!(friends::sign_text("acct_0123456789abcdef0123", friends::Op::Ask, sol, 1).is_err());
}

#[test]
fn a_face_reads_set_and_unset() {
    let v = data("face.json");
    let set = face::parse(v["set"].to_string().as_bytes()).unwrap();
    assert_eq!(set.name.as_deref(), v["set"]["name"].as_str());
    assert!(set.picture.as_deref().unwrap().starts_with("/api/face/"));
    let unset = face::parse(v["unset"].to_string().as_bytes()).unwrap();
    assert_eq!(unset.picture, None);
}

#[test]
fn a_profile_says_what_it_found_and_nothing_it_didnt() {
    let hours = json!({"games": [{"game": "gates", "seconds": 99}, {"game": "eloram", "seconds": 5400, "last_played": 7}]});
    assert_eq!(
        profile::hours(hours.to_string().as_bytes()),
        profile::Read::Got(profile::Hours { seconds: 5400, last_played: Some(7) })
    );
    assert_eq!(profile::hours(br#"{"games":[]}"#), profile::Read::None);
    let rated = json!({"ladders": {"ranked": {"rated": true, "rating": 1612.4, "rd": 80.2, "games": 12, "wins": 7, "placed": true, "rank": 3}}});
    let profile::Read::Got(r) = profile::rating(rated.to_string().as_bytes()) else { panic!() };
    assert_eq!(r.line(), "Ranked 1612 +/- 80, #3 on the ladder");
    assert_eq!(profile::rating(br#"{"ladders":{"ranked":{"rated":false}}}"#), profile::Read::None);
    let stats = json!({
        "on_record": true,
        "stats": [{"id": "wins", "name": "Wins", "value": 4}, {"id": "kills", "name": "Takedowns", "value": null}],
        "achievements": [
            {"name": "First light", "earned": true, "unlocked_at": 5},
            {"name": "Onto the Causeway", "earned": true, "unlocked_at": 2},
            {"name": "Ten dawns", "earned": false}
        ],
        "boards": [{"name": "Fastest win", "score": 754000, "rank": 9, "format": "time_ms"}],
        "totals": {"earned": 2, "total": 7}
    });
    let profile::Read::Got(s) = profile::stats(stats.to_string().as_bytes()) else { panic!() };
    assert_eq!(s.stats, vec![("Wins".to_string(), 4)], "a null is not a zero");
    assert_eq!(s.recent, ["First light", "Onto the Causeway"]);
    assert_eq!(profile::score_text(s.boards[0].1, &s.boards[0].3), "12:34");
    assert_eq!(profile::stats(br#"{"on_record":false}"#), profile::Read::None);
    assert!(
        profile::state_line(&profile::Read::<()>::Unreachable("down".into()), "your rating")
            .unwrap()
            .contains("down")
    );
}
