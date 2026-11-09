//! Eloram's client. In the browser this bin is the page's module (`scripts/build-web.sh`); the
//! desktop's is `src/bin/eloram.rs`.

#[cfg(target_arch = "wasm32")]
fn main() {
    el_client::app::run_web();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!(
        "el-client is the browser's module (scripts/build-web.sh); the desktop client is \
         `cargo run -p el-client --features desktop --release --bin eloram`"
    );
}
