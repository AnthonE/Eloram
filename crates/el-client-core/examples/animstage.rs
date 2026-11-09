//! What the anim stage can stage (`scripts/anim.sh list`): every champion's every act, how long it
//! lasts, and what it is; `knobs` lists the motion's knobs with the game's values.
//!
//!   cargo run -q -p el-client-core --example animstage -- list [filter]
//!   cargo run -q -p el-client-core --example animstage -- knobs

use el_client_core::anim::list;
use el_client_core::motion::Feel;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("knobs") => {
            for k in Feel::KNOBS {
                println!("{k:<14} {}", Feel::GAME.get(k).unwrap_or_default());
            }
        }
        Some("list") | None => {
            let filter = args.get(1).map(|s| s.to_ascii_lowercase());
            for s in list() {
                if filter.as_deref().is_some_and(|f| !s.name.contains(f)) {
                    continue;
                }
                println!("{:<20} span={:<4.2} {}", s.name, s.span, s.act.what());
            }
        }
        Some(other) => {
            eprintln!("animstage: list [filter] | knobs (not {other})");
            std::process::exit(2);
        }
    }
}
