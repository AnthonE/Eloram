#!/usr/bin/env python3
"""Writes Eloram's server list for the Elo Pros launcher and store (`elo-shardlist-v1`), after
Gates' `ci/shardlist.py` (AnthonE/Gates, MIT, MoreRight DAO).

The store serves `$SCRY_DEPOTS_DIR/eloram/servers.json` byte for byte at
`https://elopros.com/api/launcher/servers/eloram`; the operator copies this file there **before**
the manifest names that url (a url that 404s is an error in every launcher; none is an honest
"nothing published"). Each row names the server's own `status.json`, which the launcher, the store
and the game poll for its players, and beside which the store reads `online.json` for playtime.

    scripts/shardlist.py                 print the public list
    scripts/shardlist.py --out FILE      write it
    scripts/shardlist.py --check FILE    a list is in the shape the launcher reads
"""
from __future__ import annotations

import argparse
import json
import re
import sys

SERVERS = [
    {
        "id": "causeway-1",
        "name": "Eloram, the Causeway",
        "addr": "game.elopros.com:61260",
        "max_players": 512,
        "map": "The Causeway",
        "status_url": "https://game.elopros.com/eloram/status.json",
    }
]


def check(doc: dict) -> list[str]:
    """The launcher's own limits (`elo-depot`'s `shardlist.rs`): what a row may say."""
    bad = []
    rows = doc.get("servers")
    if not isinstance(rows, list) or not rows or len(rows) > 64:
        return ["servers: a list of 1 to 64 rows"]
    for r in rows:
        for k in ("id", "name", "addr"):
            if not isinstance(r.get(k), str) or not r[k] or len(r[k].encode()) > 96:
                bad.append(f"{r.get('id')}: {k} is 1 to 96 bytes")
        if not re.fullmatch(r"[A-Za-z0-9.\-\[\]:]+:\d{1,5}", r.get("addr", "")):
            bad.append(f"{r.get('id')}: addr is host:port")
        url = r.get("status_url")
        if url is not None:
            host = r.get("addr", "").rsplit(":", 1)[0]
            if not url.startswith(f"https://{host}/") or len(url) > 256:
                bad.append(f"{r.get('id')}: status_url is https on the row's own host")
    return bad


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out")
    ap.add_argument("--check")
    a = ap.parse_args()
    if a.check:
        problems = check(json.load(open(a.check)))
        print("\n".join(problems) or "ok")
        sys.exit(1 if problems else 0)
    doc = {"kind": "elo-shardlist-v1", "servers": SERVERS}
    assert not check(doc), check(doc)
    text = json.dumps(doc, indent=2) + "\n"
    if a.out:
        open(a.out, "w").write(text)
        print(f"wrote {a.out}: copy it to $SCRY_DEPOTS_DIR/eloram/servers.json on the origin")
    else:
        sys.stdout.write(text)


if __name__ == "__main__":
    main()
