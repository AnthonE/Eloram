//! The build's name, for the menu's footer, the crash note and the bug report:
//! `<version>-g<9-character commit>` (`-dirty` with uncommitted changes), or `EL_BUILD_ID` when the
//! packager sets it (`scripts/depot.py`). And on a Windows target, the `.exe`'s icon.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    println!("cargo::rerun-if-env-changed=EL_BUILD_ID");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let id = std::env::var("EL_BUILD_ID").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| {
        match git(&["rev-parse", "--short=9", "HEAD"]) {
            Some(sha) => {
                let dirty =
                    git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
                format!("{version}-g{sha}{}", if dirty { "-dirty" } else { "" })
            }
            None => version.clone(),
        }
    });
    println!("cargo::rustc-env=EL_BUILD={id}");
    // A new commit (or a checkout) names a new build.
    if let Some(dir) = git(&["rev-parse", "--git-dir"]) {
        println!("cargo::rerun-if-changed={dir}/HEAD");
        println!("cargo::rerun-if-changed={dir}/packed-refs");
        if let Some(r) = git(&["symbolic-ref", "-q", "HEAD"]) {
            println!("cargo::rerun-if-changed={dir}/{r}");
        }
    }
    windows_icon();
}

/// The `.exe`'s icon (`icon/eloram.rc`, made by `art/tools/icon.py`): mingw's windres, or none with
/// a warning (an icon is never a reason the build fails).
fn windows_icon() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let rc = format!("{dir}/icon/eloram.rc");
    println!("cargo::rerun-if-changed={rc}");
    if !std::path::Path::new(&rc).exists() {
        return;
    }
    let out = format!("{}/eloram-icon.o", std::env::var("OUT_DIR").unwrap_or_default());
    let made = ["x86_64-w64-mingw32-windres", "windres"].iter().any(|tool| {
        Command::new(tool)
            .args([
                "--input",
                &rc,
                "--output-format=coff",
                "--output",
                &out,
                "--include-dir",
                &format!("{dir}/icon"),
            ])
            .status()
            .is_ok_and(|s| s.success())
    });
    if made {
        println!("cargo::rustc-link-arg-bins={out}");
    } else {
        println!("cargo::warning=no windres: the .exe has no icon");
    }
}
