import { expect, test } from "@playwright/test";
import { collectConsole, el, luminanceStdDev } from "./util";

// The effects, champion by champion (`docs/FX.md`, "How we know it's done"): a practice match
// (`scripts/e2e.sh fx` starts a server with `--practice`), each ability of each champion cast at
// what it's for (the enemy dummy in front, an ally, a point, itself), each drawn and heard as the
// player's own; two pictures of each, as it's cast and a moment after, and a contact sheet a
// champion in `artifacts/` with all of them on one more.

const until = async (page: any, what: string, pred: (s: Record<string, any>) => boolean, timeout = 60_000) => {
  const end = Date.now() + timeout;
  for (;;) {
    const s = await el(page);
    if (pred(s)) return s;
    if (Date.now() > end) throw new Error(`waited for ${what}: ${JSON.stringify(s)}`);
    await page.waitForTimeout(400);
  }
};

const push = (page: any, cmd: Record<string, unknown>) =>
  page.evaluate((c: Record<string, unknown>) => ((window as any).__elInbox ??= []).push(c), cmd);

// `el_sim::practice::PLACES`: the enemy dummy standing in front, and an ally beside the player.
const MID = 1920;
const ENEMY = { unit: 5, x: 7680 - 200, y: MID };
const ALLY = { unit: 1 };
const CHAMPIONS = Number(process.env.EL_FX_CHAMPIONS || 20);

test("every ability of every champion, cast in practice", async ({ page, context }, info) => {
  // About five minutes a champion: SwiftShader takes many seconds a picture.
  test.setTimeout(10_800_000);
  const logs = collectConsole(page);
  const dir = `artifacts/fx-${info.project.name}`;
  // Smaller than the other suites' pages: a picture's cost is its pixels.
  await page.setViewportSize({ width: 800, height: 450 });
  await page.goto(`/?gfx=${info.project.name}&practice=1&champ=0&name=Practice`);
  try {
    // The first frames compile every pipeline: minutes on a CPU rasteriser.
    await until(page, "a practice match", (s) => s.phase === "playing" && s.started === true && s.practice === true, 600_000);
    await until(page, "the art", (s) => s.art === true, 600_000);
  } catch (e) {
    console.log(logs.slice(-30).join("\n"));
    throw e;
  }
  const missed: string[] = [];
  const sheet: { champ: string; slot: number; kit: string; shots: string[] }[] = [];
  // The contact sheets: a champion's eight pictures as it's done, then every one on one page.
  const fs = await import("node:fs");
  const img = (p: string) => `data:image/png;base64,${fs.readFileSync(p).toString("base64")}`;
  const tiles = (rows: typeof sheet, w: number) =>
    rows
      .map(
        (r) =>
          `<figure><div>${r.shots.map((p) => `<img src="${img(p)}" width="${w}">`).join("")}</div>` +
          `<figcaption>${r.champ} ${"QWER"[r.slot]} · ${r.kit}</figcaption></figure>`,
      )
      .join("");
  const css =
    "body{margin:0;padding:8px;background:#111;color:#ddd;font:12px sans-serif;display:flex;flex-wrap:wrap;gap:6px}" +
    "figure{margin:0}img{display:block;float:left;margin-right:2px}figcaption{clear:both;padding:2px 0}";
  const sheetPage = await context.newPage();
  await sheetPage.setViewportSize({ width: 1960, height: 600 });
  const write = async (rows: typeof sheet, w: number, file: string) => {
    await sheetPage.setContent(`<style>${css}</style>${tiles(rows, w)}`);
    await sheetPage.screenshot({ path: file, fullPage: true });
  };
  for (let champ = 0; champ < CHAMPIONS; champ++) {
    const switched = (s: Record<string, any>) => s.champion_def === champ && s.started === true && s.practice === true;
    let s: Record<string, any> = {};
    // Asked again if it hasn't come (once a run, the page went on with the last match).
    for (let tries = 0; ; tries++) {
      if (champ > 0) await push(page, { t: "practice_champ", champ });
      try {
        s = await until(page, `champion ${champ} in practice`, switched, 45_000);
        break;
      } catch (e) {
        if (tries === 2) {
          console.log(logs.filter((l) => /seated|lobby|error|panic/i.test(l)).slice(-20).join("\n"));
          throw e;
        }
      }
    }
    // Its model in and standing (the pose's seat 0), its family's sounds loaded (a new match's
    // models and sounds stall SwiftShader's page for a while), the camera over the dummies.
    s = await until(page, `${s.champion}'s model`, (s) => String(s.champion_poses).split(",").some((p) => p.startsWith("0:")), 120_000);
    for (let loaded = -1; ; ) {
      await page.waitForTimeout(3_000);
      s = await el(page);
      if (s.audio_loaded === loaded) break;
      loaded = s.audio_loaded;
    }
    // Between the player (6780) and the dummy in front (7480): both close enough to judge.
    await push(page, { t: "camera", x: 7130, y: MID - 60, zoom: 0.45 });
    await page.waitForTimeout(2_500);
    const name = String(s.champion);
    const kit = String(s.kit).split(",");
    for (let slot = 0; slot < 4; slot++) {
      const before = await el(page);
      const aim =
        kit[slot] === "enemy"
          ? { unit: ENEMY.unit }
          : kit[slot] === "ally"
            ? { unit: ALLY.unit }
            : kit[slot] === "caster"
              ? {}
              : { x: ENEMY.x, y: ENEMY.y };
      const a = `${dir}-${name}-${slot}-cast.png`;
      const heard = (s: Record<string, any>) =>
        s.champion_def === champ && s.fx_own_casts > before.fx_own_casts && s.audio_own_casts > before.audio_own_casts;
      // Cast, and once more if the first was lost (a page that stalled and dialled again).
      let ok = false;
      for (let tries = 0; tries < 2 && !ok; tries++) {
        await push(page, { t: "cast", slot, ...aim });
        await page.waitForTimeout(700);
        await page.screenshot({ path: a });
        try {
          await until(page, `${name}'s ${"QWER"[slot]} drawn and heard`, heard, 30_000);
          ok = true;
        } catch (e) {
          if (tries === 1) missed.push(`${name} ${"QWER"[slot]} (${kit[slot]}): ${(e as Error).message.slice(0, 160)}`);
        }
      }
      await page.waitForTimeout(1_500);
      const b = `${dir}-${name}-${slot}-after.png`;
      const shot = await page.screenshot({ path: b });
      expect(luminanceStdDev(shot)).toBeGreaterThan(5);
      sheet.push({ champ: name, slot, kit: kit[slot] + (ok ? "" : " (missed)"), shots: [a, b] });
    }
    await write(sheet.filter((r) => r.champ === name), 400, `${dir}-sheet-${name}.png`);
    console.log(`${name}: ${sheet.filter((r) => r.champ === name).map((r) => r.kit).join(", ")}`);
  }
  const s = await el(page);
  console.log(`fx status: ${JSON.stringify(s)}`);
  await write(sheet, 160, `${dir}-sheet.png`);
  await sheetPage.close();
  // Every cast drawn and heard; no sound failed to load, and the voice cap never went past.
  expect(missed, missed.join("\n")).toEqual([]);
  expect(s.audio_failed).toBe(0);
  expect(s.audio_peak).toBeLessThanOrEqual(32);
  expect(s.errors ?? 0).toBe(0);
});
