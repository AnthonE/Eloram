#!/usr/bin/env python3
"""Meshy's outputs again, from the tasks art/meshy.json names (while Meshy keeps them).

    python3 -I art/tools/meshy.py fetch <dir>

Writes <dir>/<model>.glb and <dir>/<image>.png, the inputs of art/tools/models.sh and
art/tools/sprites.py. Authenticates with MESHY_API_KEY when it's set (a proxy may add it instead).
"""
import json
import os
import sys
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
API = "https://api.meshy.ai/openapi/"


def get(path):
    headers = {}
    if os.environ.get("MESHY_API_KEY"):
        headers["Authorization"] = "Bearer " + os.environ["MESHY_API_KEY"]
    with urllib.request.urlopen(urllib.request.Request(API + path, headers=headers), timeout=60) as r:
        return json.load(r)


def main():
    if len(sys.argv) != 3 or sys.argv[1] != "fetch":
        sys.exit(__doc__)
    out = sys.argv[2]
    os.makedirs(out, exist_ok=True)
    tasks = json.load(open(os.path.join(ROOT, "art", "meshy.json")))
    for name, m in tasks["models"].items():
        url = get(f"v2/text-to-3d/{m['refine_task']}")["model_urls"]["glb"]
        urllib.request.urlretrieve(url, os.path.join(out, f"{name}.glb"))
        print(name)
    for name, m in tasks["images"].items():
        url = get(f"v1/text-to-image/{m['task']}")["image_urls"][0]
        urllib.request.urlretrieve(url, os.path.join(out, f"{name}.png"))
        print(name)


if __name__ == "__main__":
    main()
