#!/usr/bin/env python3
"""Publishes a packaged desktop build to the Elo Pros launcher: the operator's act, never CI's.
After Gates' `ci/publish_depot.py` (AnthonE/Gates, MIT, MoreRight DAO), in its order:

1. the tree is clean (a published build id names a commit a player could check out);
2. the build speaks the live server's protocol (`/status.json`'s `proto`), or it couldn't play;
3. `scripts/depot.py` packages it;
4. rsync to `<depots>/eloram/<build>/<platform>/` on the origin;
5. every file re-hashed where it landed;
6. `published.json` names it, **last**, merged (the other platform's pointer kept, a `.bak` beside);
7. the live manifest asked: this platform's row must show this build, published;
8. with `--notarize`, its digest sealed on the build notary on chain 4663, from the origin, with the
   key that never leaves it (`cast send … notarize(bytes32,string,string)`, label
   `eloram-<platform>`).

The store reads the manifest once at start: after the first publish of a platform, restart the
meter and purge the edge (scry-forge's release line).

    scripts/publish-depot.py --platform linux-x86_64 [--no-build] [--notarize] [--dry-run]
    scripts/publish-depot.py --local --depots /data/apps/scry-data/depots   (on the origin itself)
"""
from __future__ import annotations

import argparse
import json
import shlex
import subprocess
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))
import importlib.util  # noqa: E402

_spec = importlib.util.spec_from_file_location("depot", ROOT / "scripts" / "depot.py")
depot = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(depot)

SLUG = depot.SLUG
NOTARY = "0x0C15fA7829458118e3d26229F58FE0443f8b792c"
PROTO_RS = ROOT / "crates" / "el-proto" / "src" / "lib.rs"


def run(cmd: list[str], **kw) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, check=True, text=True, capture_output=True, **kw)


def sh(host: str | None, script: str) -> str:
    """The script on the origin (over ssh), or here with --local."""
    cmd = ["bash", "-c", script] if host is None else ["ssh", "-o", "BatchMode=yes", host, script]
    return run(cmd).stdout.strip()


def protocol() -> int:
    import re
    return int(re.search(r"PROTOCOL_VERSION:\s*u16\s*=\s*(\d+)", PROTO_RS.read_text()).group(1))


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--platform", default="linux-x86_64", choices=sorted(depot.TARGETS))
    ap.add_argument("--host", default="morr", help="ssh alias of the origin")
    ap.add_argument("--local", action="store_true", help="run on the origin itself")
    ap.add_argument("--depots", default="/data/apps/scry-data/depots")
    ap.add_argument("--api", default="https://elopros.com/api")
    ap.add_argument("--status-url", default="https://game.elopros.com/eloram/status.json")
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument("--notarize", action="store_true")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--strand", action="store_true", help="publish a build the live server can't play with")
    a = ap.parse_args()
    host = None if a.local else a.host

    if run(["git", "status", "--porcelain"], cwd=ROOT).stdout.strip():
        sys.exit("publish: the tree has uncommitted changes; a published build names a commit. Commit first.")
    with urllib.request.urlopen(a.status_url, timeout=20) as r:
        live = json.load(r).get("proto")
    ours = protocol()
    if live != ours and not a.strand:
        sys.exit(f"publish: the live server speaks protocol {live}, this build {ours}: it couldn't join. "
                 "Deploy the server first (or --strand).")

    cmd = [sys.executable, str(ROOT / "scripts" / "depot.py"), "--platform", a.platform]
    if a.no_build:
        cmd.append("--no-build")
    print("== package ·", " ".join(cmd[1:]))
    subprocess.run(cmd, cwd=ROOT, check=True)
    build = depot.build_id()
    stage = ROOT / "target" / "depot" / a.platform / build
    doc = json.loads((stage / "depot.json").read_text())
    if doc.get("build") != build or doc.get("platform") != a.platform:
        sys.exit(f"publish: {stage}/depot.json names {doc.get('build')}/{doc.get('platform')}")

    remote = f"{a.depots}/{SLUG}/{build}/{a.platform}"
    print(f"== upload · {stage} → {remote}")
    if not a.dry_run:
        sh(host, f"mkdir -p {shlex.quote(remote)}")
        run(["rsync", "-a", "--info=stats1", f"{stage}/", f"{remote}/" if host is None else f"{host}:{remote}/"])
        checker = (
            "import json,hashlib,pathlib\n"
            "d=json.load(open('depot.json'));bad=[]\n"
            "for f in d['files']:\n"
            "    p=pathlib.Path(f['path'])\n"
            "    if not p.is_file(): bad.append((f['path'],'missing')); continue\n"
            "    if hashlib.sha256(p.read_bytes()).hexdigest()!=f['sha256']: bad.append((f['path'],'sha256'))\n"
            "    elif f['executable'] and not (p.stat().st_mode & 0o111): bad.append((f['path'],'mode'))\n"
            "print(json.dumps({'n':len(d['files']),'bad':bad}))\n"
        )
        r = json.loads(sh(host, f"cd {shlex.quote(remote)} && python3 -c {shlex.quote(checker)}").splitlines()[-1])
        if r["bad"]:
            sys.exit(f"publish: the origin doesn't hold what was packaged: {r['bad'][:10]}")
        print(f"   {r['n']} files verified at the origin")

    pointer = f"{a.depots}/{SLUG}/published.json"
    print(f"== publish · {a.platform} → {build}")
    if not a.dry_run:
        writer = (
            "import json,pathlib,shutil\n"
            f"p=pathlib.Path({pointer!r})\n"
            "d=json.loads(p.read_text()) if p.exists() else {}\n"
            "before=dict(d)\n"
            "if p.exists(): shutil.copy2(p,str(p)+'.bak')\n"
            f"d[{a.platform!r}]={build!r}\n"
            "p.write_text(json.dumps(d))\n"
            "print(json.dumps({'before':before,'after':d}))\n"
        )
        print("  ", sh(host, f"python3 -c {shlex.quote(writer)}").splitlines()[-1])
        with urllib.request.urlopen(f"{a.api}/launcher/manifest/{SLUG}", timeout=30) as r:
            rows = json.load(r).get("builds", [])
        row = next((b for b in rows if b.get("platform") == a.platform), None)
        if not row or row.get("version") != build or row.get("depot_state") != "published":
            sys.exit(f"publish: the live manifest's {a.platform} row reads {row}: restart the meter "
                     "(it reads manifests at start) and purge the edge, then look again")
        digest = row.get("depot_digest")
        print(f"   live: {build}, digest {digest}")
        if a.notarize and digest:
            label, memo = f"{SLUG}-{a.platform}", f"{SLUG} {build} {a.platform} depot"
            send = (
                "set -a; . /data/apps/secrets/keys.env; set +a; "
                'POOL="${ELO_RH_RPC_POOL:-$SCRY_RH_RPC_POOL}"; '
                'for u in $(printf %s "$POOL" | tr "," " "); do '
                '  [ "$(~/.foundry/bin/cast chain-id --rpc-url "$u" 2>/dev/null)" = 4663 ] && RPC=$u && break; '
                "done; "
                f"~/.foundry/bin/cast send {NOTARY} 'notarize(bytes32,string,string)' {digest} "
                f"{shlex.quote(label)} {shlex.quote(memo)} --rpc-url \"$RPC\" --private-key \"$PRIVATE_KEY\" --json"
            )
            out = sh(host, send)
            print("   notarized:", json.loads(out.splitlines()[-1]).get("transactionHash"))


if __name__ == "__main__":
    main()
