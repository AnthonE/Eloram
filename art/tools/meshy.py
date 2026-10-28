#!/usr/bin/env python3
"""Meshy's tasks for the art: the champions made, and every output fetched again.

    python3 -I art/tools/meshy.py champions [name ...]
    python3 -I art/tools/meshy.py minions [--previews] [name ...]
    python3 -I art/tools/meshy.py animations
    python3 -I art/tools/meshy.py props
    python3 -I art/tools/meshy.py fetch <dir>

`champions` takes each champion in art/meshy.json (`champions`) as far as it goes: its mesh from
its prompt (text-to-3D preview), its texture (refine), its rig. A task's id is written to
art/meshy.json the moment Meshy gives it, so a run that stops is taken up where it stopped and
nothing is paid for twice; a task that failed is made again. `minions` does the same for the
minions of art/meshy.json (`minions`); one with no `height` is not a body (the siege cart) and is
only made and textured, and `--previews` stops at the meshes, to be looked at before the rest is
paid for. `animations` has the clips of art/meshy.json (`animations`) applied to the rig they
name, the same way, and `props` makes the props' meshes (`props`: a preview, untextured;
art/tools/champions.mjs colours them).

`fetch` writes <dir>/<model>.glb and <dir>/<image>.png (the inputs of art/tools/models.sh and
art/tools/sprites.py), and for the champions <dir>/champions/<name>.glb (rigged),
<dir>/champions/<name>.run.glb (its run), <dir>/champions/<set>.anim.glb (the clips) and
<dir>/props/<prop>.glb, the inputs of art/tools/champions.mjs; and <dir>/minions/<name>.glb
(rigged, or textured for one that isn't) and <dir>/minions/<name>.run.glb.

Authenticates with MESHY_API_KEY when it's set (a proxy may add it instead).
"""
import json
import os
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
import urllib.error
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
TASKS = os.path.join(ROOT, "art", "meshy.json")
API = "https://api.meshy.ai/openapi/"


def call(path, body=None):
    headers = {"Content-Type": "application/json"}
    if os.environ.get("MESHY_API_KEY"):
        headers["Authorization"] = "Bearer " + os.environ["MESHY_API_KEY"]
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(API + path, data=data, headers=headers)
    for attempt in range(5):
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                return json.load(r)
        except urllib.error.HTTPError as e:
            if e.code == 429 or e.code >= 500:
                time.sleep(2 ** attempt * 5)
                continue
            sys.exit(f"{path}: {e.code} {e.read().decode(errors='replace')}")
        except urllib.error.URLError:
            time.sleep(2 ** attempt * 2)
    sys.exit(f"{path}: no answer")


def load():
    with open(TASKS) as f:
        return json.load(f)


LOCK = threading.Lock()


def save(tasks):
    with LOCK, open(TASKS + ".tmp", "w") as f:
        json.dump(tasks, f, indent=2)
        f.write("\n")
    os.replace(TASKS + ".tmp", TASKS)


def wait(kind, task):
    """The task once it's done: SUCCEEDED, or None when it failed."""
    while True:
        t = call(f"{kind}/{task}")
        if t["status"] == "SUCCEEDED":
            return t
        if t["status"] in ("FAILED", "CANCELED"):
            print(f"  {kind}/{task} {t['status']}: {t.get('task_error')}", flush=True)
            return None
        time.sleep(10)


# Each step: the field it keeps its task in, the API it calls, and the request from the champion's
# entry and the tasks before it.
STEPS = [
    ("preview_task", "v2/text-to-3d", lambda c: {
        "mode": "preview",
        "prompt": c["prompt"],
        "ai_model": c["ai_model"],
        "pose_mode": c.get("pose", "a-pose"),
        "should_remesh": True,
        "topology": "triangle",
        "target_polycount": c["polycount"],
        "target_formats": ["glb"],
    }),
    ("refine_task", "v2/text-to-3d", lambda c: {
        "mode": "refine",
        "preview_task_id": c["preview_task"],
        "texture_prompt": c["texture_prompt"],
        "target_formats": ["glb"],
    }),
    ("rig_task", "v1/rigging", lambda c: {
        "input_task_id": c["refine_task"],
        "height_meters": c["height"],
    }),
]


def advance(tasks, name, fields, steps):
    """Takes one entry through `steps`, keeping each task's id as soon as it's made."""
    entry = fields[name]
    for field, kind, body in steps:
        while True:
            if not entry.get(field):
                entry[field] = call(kind, body(entry))["result"]
                save(tasks)
                print(f"{name}: {field} {entry[field]}", flush=True)
            if wait(kind, entry[field]):
                break
            # Failed: nothing was charged; make it again.
            entry[field] = ""
            save(tasks)


def champions(names):
    tasks = load()

    def one(name):
        advance(tasks, name, tasks["champions"], STEPS)
        print(f"{name}: rigged", flush=True)

    # Each champion's steps in order; the champions side by side.
    with ThreadPoolExecutor(8) as pool:
        list(pool.map(one, names or list(tasks["champions"])))


def minions(args):
    previews = "--previews" in args
    names = [a for a in args if a != "--previews"]
    tasks = load()

    def one(name):
        m = tasks["minions"][name]
        # A body is rigged; the cart is only textured.
        steps = STEPS[:1] if previews else STEPS if m.get("height") else STEPS[:2]
        advance(tasks, name, tasks["minions"], steps)
        print(f"{name}: {steps[-1][0]} done", flush=True)

    with ThreadPoolExecutor(8) as pool:
        list(pool.map(one, names or list(tasks["minions"])))


def animations():
    tasks = load()
    sets = tasks["animations"]
    for name, s in sets.items():
        rig = tasks["champions"][s["rig"]]["rig_task"]
        step = ("task", "v1/animations", lambda s: {
            "rig_task_id": rig,
            "action_ids": [a for _, a in s["clips"]],
        })
        advance(tasks, name, sets, [step])
        print(f"{name}: animated", flush=True)


def props():
    tasks = load()
    step = ("task", "v2/text-to-3d", lambda p: {
        "mode": "preview",
        "prompt": p["prompt"],
        "ai_model": "meshy-6-lite",
        "should_remesh": True,
        "topology": "triangle",
        "target_polycount": p["polycount"],
        "target_formats": ["glb"],
    })

    def one(name):
        advance(tasks, name, tasks["props"], [step])
        print(f"{name}: made", flush=True)

    with ThreadPoolExecutor(8) as pool:
        list(pool.map(one, list(tasks["props"])))


def get(url, path):
    urllib.request.urlretrieve(url, path)
    print(os.path.relpath(path), flush=True)


def fetch(out):
    os.makedirs(os.path.join(out, "champions"), exist_ok=True)
    tasks = load()
    for name, m in tasks["models"].items():
        get(call(f"v2/text-to-3d/{m['refine_task']}")["model_urls"]["glb"], os.path.join(out, f"{name}.glb"))
    for name, m in tasks["images"].items():
        get(call(f"v1/text-to-image/{m['task']}")["image_urls"][0], os.path.join(out, f"{name}.png"))
    for name, c in tasks.get("champions", {}).items():
        if c.get("rig_task"):
            r = call(f"v1/rigging/{c['rig_task']}")["result"]
            get(r["rigged_character_glb_url"], os.path.join(out, "champions", f"{name}.glb"))
            get(r["basic_animations"]["running_glb_url"], os.path.join(out, "champions", f"{name}.run.glb"))
    for name, s in tasks.get("animations", {}).items():
        if s.get("task"):
            r = call(f"v1/animations/{s['task']}")["result"]
            get(r["animation_glb_url"], os.path.join(out, "champions", f"{name}.anim.glb"))
    os.makedirs(os.path.join(out, "minions"), exist_ok=True)
    for name, m in tasks.get("minions", {}).items():
        if m.get("rig_task"):
            r = call(f"v1/rigging/{m['rig_task']}")["result"]
            get(r["rigged_character_glb_url"], os.path.join(out, "minions", f"{name}.glb"))
            get(r["basic_animations"]["running_glb_url"], os.path.join(out, "minions", f"{name}.run.glb"))
        elif m.get("refine_task") and not m.get("height"):
            get(call(f"v2/text-to-3d/{m['refine_task']}")["model_urls"]["glb"], os.path.join(out, "minions", f"{name}.glb"))
    os.makedirs(os.path.join(out, "props"), exist_ok=True)
    for name, p in tasks.get("props", {}).items():
        if p.get("task"):
            get(call(f"v2/text-to-3d/{p['task']}")["model_urls"]["glb"], os.path.join(out, "props", f"{name}.glb"))


def main():
    args = sys.argv[1:]
    if args[:1] == ["champions"]:
        champions(args[1:])
    elif args[:1] == ["minions"]:
        minions(args[1:])
    elif args == ["animations"]:
        animations()
    elif args == ["props"]:
        props()
    elif len(args) == 2 and args[0] == "fetch":
        fetch(args[1])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
