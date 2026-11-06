//! What the fx stage can fire (`scripts/fx.sh list`): every name, how far it reaches, how long
//! it lasts, and what it is; `knobs` lists the knobs a recipe is retuned with.
//!
//!   cargo run -q -p el-client-core --example fxstage -- list [filter]
//!   cargo run -q -p el-client-core --example fxstage -- knobs

use el_client_core::fx::stage::{KNOBS, list};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("knobs") => {
            for (k, what) in KNOBS {
                println!("{k:<16} {what}");
            }
        }
        Some("list") | None => {
            let filter = args.get(1).map(|s| s.to_ascii_lowercase());
            for s in list() {
                if filter.as_deref().is_some_and(|f| !s.name.contains(f)) {
                    continue;
                }
                println!("{:<24} r={:<5.2} span={:<4.2} {}", s.name, s.radius, s.span, s.what);
            }
        }
        Some(other) => {
            eprintln!("fxstage: list [filter] | knobs (not {other})");
            std::process::exit(2);
        }
    }
}
