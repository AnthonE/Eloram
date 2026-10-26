#!/usr/bin/env python3
"""The ground's textures: ambientCG's CC0 sets, graded to the Causeway's cold palette and stacked
into three arrays (five layers, 512 px each, in this order: paving, snow, gravel, dressed slabs,
cliff rock), and the runner's inlay, drawn here.

    python3 -I art/tools/textures.py <cache dir>

Writes web/assets/ground/{albedo,normal,rha}.jpg (rha: roughness, height, ambient occlusion) and
web/assets/ground/runner.png. The cache keeps the downloads (they don't go in git).
"""
import io
import math
import os
import sys
import urllib.request
import zipfile

import numpy as np
from PIL import Image, ImageFilter

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "web", "assets", "ground")
SIZE = 512

# (ambientCG id, tint the luminance takes, how much of the source's own colour stays, blur)
LAYERS = [
    ("Bricks108A", (0.66, 0.74, 0.88), 0.12, 0.5),
    ("Snow010A", (0.84, 0.89, 0.97), 0.0, 0.0),
    ("Gravel043", (0.62, 0.68, 0.78), 0.15, 0.4),
    ("PavingStones136", (0.66, 0.72, 0.84), 0.10, 0.4),
    ("Rock035", (0.62, 0.70, 0.86), 0.35, 0.3),
]


def fetch(cache, asset):
    d = os.path.join(cache, asset)
    if not os.path.isdir(d):
        url = f"https://ambientcg.com/get?file={asset}_1K-JPG.zip"
        print("fetching", url)
        data = urllib.request.urlopen(url, timeout=120).read()
        zipfile.ZipFile(io.BytesIO(data)).extractall(d)
    return lambda m: Image.open(os.path.join(d, f"{asset}_1K-JPG_{m}.jpg"))


def grade(img, tint, keep, blur):
    a = np.asarray(img.convert("RGB"), dtype=np.float32) / 255.0
    lum = a @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
    # A little more contrast, then the cold tint; a share of the source's colour stays for life.
    lum = np.clip((lum - 0.5) * 1.15 + 0.5, 0.0, 1.0)
    cold = lum[..., None] * np.array(tint, dtype=np.float32)[None, None, :] * 1.2
    out = cold * (1.0 - keep) + a * keep * np.array(tint) / max(tint) * 1.1
    im = Image.fromarray((np.clip(out, 0, 1) * 255).astype(np.uint8))
    return im.filter(ImageFilter.GaussianBlur(blur)) if blur > 0 else im


def layers(cache):
    albedo, normal, rha = [], [], []
    for asset, tint, keep, blur in LAYERS:
        get = fetch(cache, asset)
        rs = lambda im: im.resize((SIZE, SIZE), Image.LANCZOS)
        albedo.append(rs(grade(get("Color"), tint, keep, blur)))
        normal.append(rs(get("NormalGL").convert("RGB")))
        rough = rs(get("Roughness").convert("L"))
        height = rs(get("Displacement").convert("L"))
        try:
            ao = rs(get("AmbientOcclusion").convert("L"))
        except FileNotFoundError:
            ao = Image.new("L", (SIZE, SIZE), 255)
        rha.append(Image.merge("RGB", (rough, height, ao)))
    return albedo, normal, rha


def stack(ims):
    out = Image.new("RGB", (SIZE, SIZE * len(ims)))
    for k, im in enumerate(ims):
        out.paste(im, (0, k * SIZE))
    return out


def runner():
    """The runner's tile: 5 m along the lane by 5 m across, 512 px. Across it: a rim, a band of
    meander (a key pattern cut into the stone), a rim, and a sunken panel of big slabs; the same
    the other side. R: height, G: the panel (0) or the border's stone (1), B: coverage."""
    n = SIZE
    h = np.zeros((n, n), dtype=np.float32)
    kind = np.zeros((n, n), dtype=np.float32)
    v = (np.arange(n) + 0.5) / n
    u = (np.arange(n) + 0.5) / n
    U, V = np.meshgrid(u, v)
    # Distance from the strip's edge, mirrored: 0 at the edge, 0.5 at the middle.
    e = np.minimum(V, 1.0 - V)
    rim = (e < 0.035) | ((e > 0.175) & (e < 0.205))
    band = (e >= 0.035) & (e <= 0.175)
    panel = e > 0.205
    h[rim] = 1.0
    kind[rim | band] = 1.0
    # The meander: a key pattern on a grid of 7 by 7 cells a period, its groove cut 0.45 deep.
    period = 1.0 / 5.0  # five periods a tile
    bu = (U % period) / period  # 0..1 along a period
    bv = np.clip((e - 0.035) / 0.14, 0.0, 0.9999)  # 0..1 across the band
    cu = np.floor(bu * 7).astype(int)
    cv = np.floor(bv * 7).astype(int)
    key = np.array(
        [
            [1, 1, 1, 1, 1, 1, 0],
            [1, 0, 0, 0, 0, 1, 0],
            [1, 0, 1, 1, 0, 1, 0],
            [1, 0, 1, 0, 0, 1, 0],
            [1, 0, 1, 1, 1, 1, 0],
            [1, 0, 0, 0, 0, 0, 0],
            [1, 1, 1, 1, 1, 1, 1],
        ],
        dtype=np.float32,
    )
    raised = key[cv, cu]
    h[band] = np.where(raised[band] > 0.5, 0.85, 0.40)
    # The panel: sunken slabs, a joint every 1.25 m along and one down the middle.
    pu = (U * 4.0) % 1.0
    joint = (np.minimum(pu, 1.0 - pu) < 0.012) | (np.abs(V - 0.5) < 0.004)
    h[panel] = 0.55
    h[panel & joint] = 0.25
    # Soft edges on everything, so the normals have slopes to show.
    hi = Image.fromarray((h * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(1.6))
    cover = np.ones((n, n), dtype=np.float32)
    out = np.stack([np.asarray(hi, dtype=np.float32) / 255.0, kind, cover], axis=-1)
    return Image.fromarray((out * 255).astype(np.uint8))


def main():
    cache = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "target", "art-cache")
    os.makedirs(OUT, exist_ok=True)
    albedo, normal, rha = layers(cache)
    stack(albedo).save(os.path.join(OUT, "albedo.jpg"), quality=88, optimize=True)
    stack(normal).save(os.path.join(OUT, "normal.jpg"), quality=92, optimize=True)
    stack(rha).save(os.path.join(OUT, "rha.jpg"), quality=90, optimize=True)
    runner().save(os.path.join(OUT, "runner.png"), optimize=True)
    for f in sorted(os.listdir(OUT)):
        print(f, os.path.getsize(os.path.join(OUT, f)) // 1024, "KiB")


if __name__ == "__main__":
    main()
