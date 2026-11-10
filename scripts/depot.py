#!/usr/bin/env python3
"""Packages Eloram's desktop build as an Elo Pros launcher depot: one build of the game as a flat
list of files, each with its sha256, and the one command that starts it. The launcher installs it
by fetching every file, hashing it, and only then putting it in place.

After Gates' `ci/depot.py` (AnthonE/Gates, MIT, MoreRight DAO), whose rules this keeps:

- **It does not compute the depot's digest** (the number notarized on chain): the launcher's
  `elo digest <depot.json>` is its one implementation. It prints the command.
- **It does not publish**: `scripts/publish-depot.py` does, and it is the operator's.
- **It bundles no system library** (the window's, the GPU's, the sound's come with the machine),
  but it does ship what Windows lacks: mingw's runtime DLLs, found by walking the program's imports
  and asking the toolchain which of them it has (a name Windows provides resolves to nothing). A
  Windows build without them is a build nobody can start.
- The files staged are the program, `git ls-files web/assets` (never a directory walk: untracked
  files don't ship) as `assets/`, minus the Opus files (the desktop plays the MP3s), and the
  licences.

    scripts/depot.py                        the Linux depot (build, stage, write depot.json)
    scripts/depot.py --platform win-x86_64  the Windows one (x86_64-pc-windows-gnu, mingw posix)
    scripts/depot.py --no-build             package what is already compiled
    scripts/depot.py --self-test            the packager's own rules, with no compiler

The build id is `<version>-g<9-character commit>` (`-dirty` with uncommitted changes); it names
the directory on the player's disk, and `EL_BUILD_ID` passes it to the build so the game's footer
and crash notes say the same.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SLUG = "eloram"
DEPOT_VERSION = 1
LAUNCH_ARGS = ["--server", "{server}", "--identity", "{wallet}", "--servers", "{servers}"]
# Baked in at package time and covered by the digest, so the origin never rewrites it.
ROOT_URL = "https://elopros.com/api/launcher/depot/{slug}/{build}/{platform}/files"

TARGETS = {
    "linux-x86_64": {
        "triple": None,
        "exec": "eloram",
        "strip": "strip",
        "method": "elf-dt-needed",
        "dlopen": "libxkbcommon-x11, libvulkan, libEGL, libasound",
    },
    "win-x86_64": {
        "triple": "x86_64-pc-windows-gnu",
        "exec": "eloram.exe",
        "strip": "x86_64-w64-mingw32-strip",
        "method": "pe-import-table",
        "dlopen": "vulkan-1.dll, d3d12.dll",
        "dll_probe": "x86_64-w64-mingw32-gcc",
        "pe_format": "pei-x86-64",
        "objdump": "x86_64-w64-mingw32-objdump",
    },
}


def run(cmd: list[str], **kw) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, check=True, text=True, capture_output=True, **kw)


def sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def build_id() -> str:
    version = re.search(r'^version\s*=\s*"([^"]+)"', (ROOT / "Cargo.toml").read_text(), re.M).group(1)
    sha = run(["git", "rev-parse", "--short=9", "HEAD"], cwd=ROOT).stdout.strip()
    dirty = run(["git", "status", "--porcelain", "--untracked-files=no"], cwd=ROOT).stdout.strip()
    return f"{version}-g{sha}" + ("-dirty" if dirty else "")


def needed_libs(binary: Path, platform: str) -> tuple[list[str], str]:
    """What the binary's loader will look for: ELF DT_NEEDED or the PE import table."""
    tool = TARGETS[platform].get("objdump", "objdump")
    if shutil.which(tool) is None:
        return [], f"no {tool} to read the binary's imports"
    p = subprocess.run([tool, "-p", str(binary)], text=True, capture_output=True)
    if p.returncode != 0:
        return [], f"{tool} could not read {binary.name}"
    pat = r"^\s*NEEDED\s+(\S+)" if platform.startswith("linux") else r"^\s*DLL Name:\s*(\S+)"
    return sorted(set(re.findall(pat, p.stdout, re.M))), ""


def toolchain_dll(name: str, probe: str) -> Path | None:
    """The toolchain's own copy of `name` (mingw's runtime), or None (Windows provides it)."""
    if shutil.which(probe) is None:
        return None
    p = subprocess.run([probe, f"-print-file-name={name}"], text=True, capture_output=True)
    got = Path(p.stdout.strip()) if p.returncode == 0 else None
    return got if got and got.is_absolute() and got.is_file() else None


def runtime_dlls(binary: Path, platform: str) -> list[Path]:
    """Every DLL this build needs that Windows won't provide, and what those need in turn."""
    spec = TARGETS[platform]
    probe = spec.get("dll_probe")
    if not probe:
        return []
    if shutil.which(probe) is None:
        sys.exit(f"depot: no {probe}: the DLLs this build needs can't be found. Refusing to stage "
                 "a Windows build with no runtime beside it.")
    found: dict[str, Path] = {}
    queue, seen = [binary], set()
    while queue:
        imports, why = needed_libs(queue.pop(), platform)
        if why:
            sys.exit(f"depot: {why}. Refusing to stage a Windows build whose DLLs weren't measured.")
        for name in imports:
            if name.lower() in seen:
                continue
            seen.add(name.lower())
            src = toolchain_dll(name, probe)
            if src is None:
                continue
            fmt = subprocess.run([spec.get("objdump", "objdump"), "-f", str(src)], text=True, capture_output=True)
            got = re.search(r"file format (\S+)", fmt.stdout)
            if spec.get("pe_format") and (got.group(1) if got else "") != spec["pe_format"]:
                sys.exit(f"depot: {src} isn't {spec['pe_format']}: it would fail to load on the player's machine")
            found[name.lower()] = src
            queue.append(src)
    return [found[k] for k in sorted(found)]


def safe_relpath(raw: str) -> str:
    """The launcher's path rule: relative, forward slashes, nothing that climbs out."""
    if not raw or "\x00" in raw or "\\" in raw or len(raw) > 1024:
        sys.exit(f"depot: bad path {raw!r}")
    if raw.startswith("/") or raw.endswith("/") or (len(raw) > 1 and raw[1] == ":"):
        sys.exit(f"depot: paths must be relative files: {raw!r}")
    if any(p in ("", ".", "..") for p in raw.split("/")):
        sys.exit(f"depot: path escapes the build directory: {raw!r}")
    return raw


def depot_doc(stage: Path, build: str, platform: str, requires: dict | None = None) -> dict:
    exec_name = TARGETS[platform]["exec"]
    files, seen = [], set()
    for p in sorted(stage.rglob("*")):
        if not p.is_file() or p.is_symlink():
            continue
        rel = safe_relpath(p.relative_to(stage).as_posix())
        if rel.lower() in seen:
            sys.exit(f"depot: two files differ only by case: {rel!r}")
        seen.add(rel.lower())
        files.append({"path": rel, "sha256": sha256_file(p), "bytes": p.stat().st_size,
                      "executable": rel == exec_name})
    if exec_name not in {f["path"] for f in files}:
        sys.exit(f"depot: launch.exec {exec_name!r} is not one of the staged files")
    doc = {
        "depot_version": DEPOT_VERSION,
        "slug": SLUG,
        "build": build,
        "platform": platform,
        "root": ROOT_URL.format(slug=SLUG, build=build, platform=platform),
        "files": files,
        "launch": {"exec": exec_name, "args": LAUNCH_ARGS, "cwd": ".", "env": {}},
    }
    if requires:
        doc["requires"] = requires
    return doc


def tracked_assets() -> list[str]:
    out = run(["git", "ls-files", "-z", "--", "web/assets"], cwd=ROOT).stdout
    files = [f for f in out.split("\0") if f and not f.endswith(".ogg")]
    if not files:
        sys.exit("depot: git lists no files under web/assets: nothing to ship the game with")
    return files


def stage_build(out: Path, platform: str, do_build: bool, do_strip: bool) -> Path:
    spec = TARGETS[platform]
    build = build_id()
    if do_build:
        cmd = ["cargo", "build", "-p", "el-client", "--features", "desktop", "--release", "--bin", "eloram"]
        if spec["triple"]:
            cmd += ["--target", spec["triple"]]
        print("==", " ".join(cmd))
        subprocess.run(cmd, cwd=ROOT, check=True, env={**os.environ, "EL_BUILD_ID": build})
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    binary = target / (spec["triple"] or "") / "release" / spec["exec"]
    if not binary.is_file():
        sys.exit(f"depot: {binary} isn't built (drop --no-build)")
    staging = out / ".staging"
    shutil.rmtree(staging, ignore_errors=True)
    staging.mkdir(parents=True)
    exe = staging / spec["exec"]
    shutil.copy2(binary, exe)
    if do_strip and shutil.which(spec["strip"]):
        run([spec["strip"], str(exe)])
    bundled = []
    for dll in runtime_dlls(exe, platform):
        shutil.copy2(dll, staging / dll.name)
        bundled.append(dll.name)
    if bundled:
        notice = ROOT / "ops" / "licenses" / "MINGW-RUNTIME.txt"
        if not notice.is_file():
            sys.exit(f"depot: {notice} is missing: the mingw runtime ships with its notice or not at all")
        shutil.copy2(notice, staging / "LICENSE-MINGW-RUNTIME.txt")
    for rel in tracked_assets():
        dst = staging / "assets" / Path(rel).relative_to("web/assets")
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / rel, dst)
    for name, src in [("LICENSE", "LICENSE"), ("NOTICE", "NOTICE"), ("ART-LICENSES.md", "art/LICENSES.md"),
                      ("LICENSE-ROBOTO.txt", "crates/el-client/fonts/LICENSE-ROBOTO.txt")]:
        p = ROOT / src
        if not p.is_file():
            sys.exit(f"depot: {src} is missing: the game ships with its licences")
        shutil.copy2(p, staging / name)
    libs, why = needed_libs(exe, platform)
    requires = {"libs": [l for l in libs if l not in bundled], "bundled": bundled,
                "method": spec["method"], "measured": not why, "complete": False,
                "why": f"loaded at run time, not linked: {spec['dlopen']}"}
    dest = out / build
    shutil.rmtree(dest, ignore_errors=True)
    staging.rename(dest)
    doc = depot_doc(dest, build, platform, requires)
    (dest / "depot.json").write_text(json.dumps(doc, indent=2) + "\n")
    return dest


def self_test() -> None:
    with tempfile.TemporaryDirectory() as t:
        stage = Path(t)
        (stage / "assets" / "models").mkdir(parents=True)
        (stage / "eloram").write_bytes(b"\x7fELF")
        (stage / "assets" / "models" / "a.glb").write_bytes(b"glb")
        doc = depot_doc(stage, "0.1.0-gabcdef123", "linux-x86_64")
        paths = [f["path"] for f in doc["files"]]
        assert paths == ["assets/models/a.glb", "eloram"], paths
        assert [f["executable"] for f in doc["files"]] == [False, True]
        assert doc["root"] == "https://elopros.com/api/launcher/depot/eloram/0.1.0-gabcdef123/linux-x86_64/files"
        assert doc["launch"]["args"] == LAUNCH_ARGS
        assert all(isinstance(f["bytes"], int) for f in doc["files"])
        for bad in ["../x", "/abs", "a\\b", "a//b", "C:x"]:
            try:
                safe_relpath(bad)
            except SystemExit:
                continue
            raise AssertionError(f"{bad!r} was accepted")
        # Every placeholder is one the launcher fills.
        assert set(re.findall(r"\{(\w+)\}", " ".join(LAUNCH_ARGS))) <= {"server", "wallet", "build_dir", "host", "servers"}
    print("depot.py self-test: ok")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--platform", default="linux-x86_64", choices=sorted(TARGETS))
    ap.add_argument("--out", default=str(ROOT / "target" / "depot"))
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument("--no-strip", action="store_true")
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args()
    if a.self_test:
        self_test()
        return
    out = Path(a.out) / a.platform
    dest = stage_build(out, a.platform, not a.no_build, not a.no_strip)
    doc = json.loads((dest / "depot.json").read_text())
    size = sum(f["bytes"] for f in doc["files"])
    print(f"== {SLUG} {doc['build']} {a.platform}: {len(doc['files'])} files, {size / 1e6:.1f} MB → {dest}")
    print(f"   requires: {', '.join(doc['requires']['libs']) or 'nothing named'}; bundled: {', '.join(doc['requires']['bundled']) or 'nothing'}")
    print(f"   digest:  elo digest {dest / 'depot.json'}")
    print(f"   publish: scripts/publish-depot.py --platform {a.platform} --no-build (the operator's)")


if __name__ == "__main__":
    main()
