#!/usr/bin/env python3
"""The brush's sprites: Meshy text-to-image's frosted shrubs (art/meshy.json), each drawn on black,
keyed to alpha, cropped to the plant and put side by side, its base on the cell's bottom edge.

    python3 -I art/tools/sprites.py <bush0.png> <bush1.png> <bush2.png>

Writes web/assets/brush.png (three 384 px cells in a row).
"""
import os
import sys

import numpy as np
from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CELL = 384


def cut(path):
    a = np.asarray(Image.open(path).convert("RGB"), dtype=np.float32) / 255.0
    # On black, the brightest channel says how much plant there is.
    m = a.max(axis=-1)
    alpha = np.clip((m - 0.07) / 0.25, 0.0, 1.0)
    # The colour unmixed from the black it was drawn on.
    rgb = np.clip(a / np.maximum(alpha, 1e-3)[..., None], 0.0, 1.0)
    rgba = np.concatenate([rgb, alpha[..., None]], axis=-1)
    ys, xs = np.nonzero(alpha > 0.02)
    y0, y1, x0, x1 = ys.min(), ys.max() + 1, xs.min(), xs.max() + 1
    im = Image.fromarray((rgba[y0:y1, x0:x1] * 255).astype(np.uint8), "RGBA")
    # Fit the cell, centred, standing on its bottom.
    k = CELL / max(im.size)
    im = im.resize((max(1, int(im.width * k)), max(1, int(im.height * k))), Image.LANCZOS)
    cell = Image.new("RGBA", (CELL, CELL), (0, 0, 0, 0))
    cell.paste(im, ((CELL - im.width) // 2, CELL - im.height))
    return cell


def main():
    cells = [cut(p) for p in sys.argv[1:]]
    out = Image.new("RGBA", (CELL * len(cells), CELL))
    for k, c in enumerate(cells):
        out.paste(c, (k * CELL, 0))
    path = os.path.join(ROOT, "web", "assets", "brush.png")
    out.save(path, optimize=True)
    print(path, os.path.getsize(path) // 1024, "KiB")


if __name__ == "__main__":
    main()
