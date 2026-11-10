"""Eloram's icon, drawn from web/favicon.svg's shapes at any size: the window's
(crates/el-client/icon/eloram-256.png, set by the desktop client) and the .exe's
(eloram.ico with its 16 to 256 pixel sizes, and the eloram.rc that names it for
the build script's windres).

    python3 -I art/tools/icon.py

The favicon is a 32-unit square: a dark rounded tile, the lane as a pale chevron
from (4,22) up to (16,8) and down to (28,22), and the two teams' bases as dots
at its feet, Dawn blue on the left and Dusk red on the right. Drawn four times
larger than wanted and scaled down, for smooth edges.
"""

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "crates" / "el-client" / "icon"
TILE, LANE, DAWN, DUSK = "#05070c", "#e9eef5", "#4d9df2", "#f25a40"


def draw(size: int) -> Image.Image:
    k = size * 4 / 32
    big = Image.new("RGBA", (size * 4, size * 4), (0, 0, 0, 0))
    d = ImageDraw.Draw(big)
    d.rounded_rectangle([0, 0, 32 * k - 1, 32 * k - 1], radius=6 * k, fill=TILE)
    d.line([(4 * k, 22 * k), (16 * k, 8 * k), (28 * k, 22 * k)], fill=LANE, width=round(3 * k), joint="curve")
    for x in (4, 28):  # round the chevron's ends, as stroke-linejoin and the dots do
        r = 1.5 * k
        d.ellipse([x * k - r, 22 * k - r, x * k + r, 22 * k + r], fill=LANE)
    for x, colour in ((7, DAWN), (25, DUSK)):
        d.ellipse([(x - 3) * k, 19 * k, (x + 3) * k, 25 * k], fill=colour)
    return big.resize((size, size), Image.LANCZOS)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    draw(256).save(OUT / "eloram-256.png")
    draw(256).save(OUT / "eloram.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    (OUT / "eloram.rc").write_text('1 ICON "eloram.ico"\n')
    print(f"wrote {OUT}/eloram-256.png, eloram.ico, eloram.rc")


if __name__ == "__main__":
    main()
