"""The main menu's backdrop (web/assets/menu/causeway.jpg): one of the Causeway pictures the
backdrop suite shoots from a match on the high tier, the fog's darkness and the HUD off
(e2e/tests/backdrop.spec.ts), made quiet enough to read the menu over.

    scripts/e2e.sh backdrop webgl2
    python3 -I art/tools/menu_backdrop.py [e2e/artifacts/backdrop-<spot>.png]

It crops the picture to 16:9, takes the saturation and the light down a little, darkens the left
where the nav column and the panel sit (the menu's scrim does the rest), and writes a 1920x1080
JPEG at quality 82. The menu drifts it slowly (crates/el-client/src/front/ui.rs) and shows the
plain ink when it's missing. Without an argument it takes the turret's picture.
"""

import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageEnhance

ROOT = Path(__file__).resolve().parents[2]
DEFAULT = ROOT / "e2e" / "artifacts" / "backdrop-turret.png"
OUT = ROOT / "web" / "assets" / "menu" / "causeway.jpg"
W, H = 1920, 1080


def make(src: Path) -> Image.Image:
    im = Image.open(src).convert("RGB")
    # 16:9 from the middle, then to size.
    w, h = im.size
    if w / h > W / H:
        cw = round(h * W / H)
        im = im.crop(((w - cw) // 2, 0, (w - cw) // 2 + cw, h))
    else:
        ch = round(w * H / W)
        im = im.crop((0, (h - ch) // 2, w, (h - ch) // 2 + ch))
    im = im.resize((W, H), Image.LANCZOS)
    im = ImageEnhance.Color(im).enhance(0.85)
    im = ImageEnhance.Brightness(im).enhance(0.9)
    # Darker to the left, where the nav and the panel are: 0.55 at the edge to 1 past 45%.
    a = np.asarray(im).astype(np.float32)
    x = np.linspace(0.0, 1.0, W, dtype=np.float32)
    shade = np.clip(0.55 + 0.45 * (x / 0.45), 0.55, 1.0)
    a *= shade[None, :, None]
    return Image.fromarray(np.clip(a, 0, 255).astype(np.uint8))


def main() -> None:
    src = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT
    if not src.is_file():
        sys.exit(f"{src} isn't there: scripts/e2e.sh backdrop webgl2 shoots it")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    make(src).save(OUT, "JPEG", quality=82, optimize=True, progressive=True)
    print(f"wrote {OUT.relative_to(ROOT)} ({OUT.stat().st_size // 1024} KB) from {src.name}")


if __name__ == "__main__":
    main()
