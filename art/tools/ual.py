#!/usr/bin/env python3
"""Quaternius's Universal Animation Library, volumes 1 and 2 (the Standard sets, CC0), fetched once
into art/cache/ual/ (not in git) for art/tools/champions.mjs --ual (docs/CHAMPIONS.md, "The second
clips"). Each zip is checked against its hash and its licence file (CC0) and only its glTF and its
licence are taken out; nothing in it is run.

    python3 -I art/tools/ual.py [--cache dir]

Writes <cache>/ual1.glb and <cache>/ual2.glb (one rig, every clip of the set on it).
"""
import argparse
import hashlib
import os
import sys
import urllib.request
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OGA = "https://opengameart.org/sites/default/files"
SETS = {
    "ual1": (
        "universal_animation_librarystandard.zip",
        "18ff1a7215f4852b320203e8aaf02a1578b5c8eef9027fbaedfcedc7b85a3ac2",
        "Animation Library[Standard]/Godot/AnimationLibrary_Godot_Standard.glb",
        "Animation Library[Standard]/License.txt",
    ),
    "ual2": (
        "universal_animation_library_2standard.zip",
        "ec0e40d6d78fe9aaad59e322f40865a8675c22f0745e291622e54520391a9217",
        "Universal Animation Library 2 [Standard]/Unreal-Godot/UAL2_Standard.glb",
        "Universal Animation Library 2 [Standard]/License.txt",
    ),
}


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--cache", default=os.path.join(ROOT, "art", "cache", "ual"))
    args = ap.parse_args()
    zips = os.path.join(args.cache, "zips")
    os.makedirs(zips, exist_ok=True)
    for name, (file, sha, glb, licence) in SETS.items():
        z = os.path.join(zips, file)
        if not os.path.exists(z) or sha256(z) != sha:
            print(f"fetching {file}")
            urllib.request.urlretrieve(f"{OGA}/{file}", z)
        got = sha256(z)
        if got != sha:
            sys.exit(f"{file}: sha256 {got}, not {sha}")
        with zipfile.ZipFile(z) as zf:
            text = zf.read(licence).decode("utf-8", "replace")
            if "CC0" not in text:
                sys.exit(f"{file}: its licence doesn't say CC0")
            with open(os.path.join(args.cache, f"{name}.glb"), "wb") as out:
                out.write(zf.read(glb))
            with open(os.path.join(args.cache, f"{name}.LICENSE.txt"), "w") as out:
                out.write(text)
        print(f"{name}.glb from {file}")


if __name__ == "__main__":
    main()
