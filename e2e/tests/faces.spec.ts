import { expect, test } from "@playwright/test";
import { PNG } from "pngjs";
import * as fs from "fs";
import { collectConsole, el } from "./util";

// The champions' faces (`el-client/src/faces.rs`): practice as each champion in turn, a sitting
// (`{t:"face",on:true}`: the live portrait 256 pixels a side on a plain ground, the window's
// height a side at its left), and its picture cut out into `web/assets/portraits/<name>.jpg`, which ships as it is
// (`art/LICENSES.md`). On the high tier, the HUD's own light. `EL_FACES=kesh,mira` for a few.

const push = (page: any, cmd: Record<string, unknown>) =>
  page.evaluate((c: Record<string, unknown>) => ((window as any).__elInbox ??= []).push(c), cmd);

const until = async (page: any, what: string, pred: (s: Record<string, any>) => boolean, timeout = 120_000) => {
  const end = Date.now() + timeout;
  for (;;) {
    const s = await el(page);
    if (pred(s)) return s;
    if (Date.now() > end) throw new Error(`waited for ${what}: ${JSON.stringify(s).slice(0, 800)}`);
    await page.waitForTimeout(300);
  }
};

// `CHAMPIONS`' order (`el-sim/src/content/champions.rs`): the number practice takes.
const NAMES = [
  "vessa", "orrin", "brakka", "kesh", "liora", "talmar", "seren", "hollis", "vire", "oda",
  "marret", "nimra", "gorum", "sable", "pell", "ulla", "torek", "sefa", "brun", "mira",
];
const ONLY = (process.env.EL_FACES || "").split(",").filter(Boolean);
const SIDE = 256;
const OUT = "../web/assets/portraits";

// As tall as the picture: the sitting fills the window's height.
test.use({ viewport: { width: 400, height: 256 } });

test("a face a champion", async ({ page }) => {
  test.setTimeout(3_600_000);
  const logs = collectConsole(page);
  await page.addInitScript(() => localStorage.setItem("el.taught", "1"));
  fs.mkdirSync(OUT, { recursive: true });
  try {
    let first = true;
    for (const [champ, name] of NAMES.entries()) {
      if (ONLY.length && !ONLY.includes(name)) continue;
      if (first) {
        await page.goto(`/?gfx=webgl2&quality=high&name=Sitter&camera=locked&practice=1&champ=${champ}`);
        first = false;
      } else {
        await push(page, { t: "practice_champ", champ });
      }
      let s = await until(page, `${name} in practice`, (s) => s.champion_def === champ && s.started === true && s.practice === true, 300_000);
      await push(page, { t: "face", on: true });
      // Its model in and its portrait drawn, then a few frames for its stance to settle.
      s = await until(
        page,
        `${name}'s portrait`,
        (s) => s.portrait === true && String(s.champion_poses).split(",").some((p) => p.startsWith("0:")),
        300_000,
      );
      const f = s.frames ?? 0;
      await until(page, `${name}'s frames`, (s) => (s.frames ?? 0) >= f + 12, 300_000);
      const shot = await page.screenshot({ clip: { x: 0, y: 0, width: SIDE, height: SIDE }, timeout: 180_000 });
      // A face, not the ground alone: a good part of it isn't the ground's colour (its corner's).
      const img = PNG.sync.read(shot);
      const [r, g, b] = [img.data[0], img.data[1], img.data[2]];
      let lit = 0;
      for (let i = 0; i < img.data.length; i += 4) {
        if (Math.abs(img.data[i] - r) + Math.abs(img.data[i + 1] - g) + Math.abs(img.data[i + 2] - b) > 24) lit++;
      }
      fs.writeFileSync(`artifacts/face-${name}.png`, shot);
      expect(lit / (SIDE * SIDE), `${name}'s face fills its picture`).toBeGreaterThan(0.25);
      // The file is a JPEG (a tenth of the PNG: twenty of them are fetched as the page starts).
      const clip = { x: 0, y: 0, width: SIDE, height: SIDE };
      await page.screenshot({ path: `${OUT}/${name}.jpg`, type: "jpeg", quality: 90, clip, timeout: 180_000 });
    }
  } catch (e) {
    console.log(logs.slice(-30).join("\n"));
    throw e;
  }
});
