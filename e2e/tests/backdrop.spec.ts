import { test } from "@playwright/test";
import { collectConsole, el } from "./util";

const push = (page: any, cmd: Record<string, unknown>) =>
  page.evaluate((c: Record<string, unknown>) => {
    const w = window as any;
    (w.__elInbox ||= []).push(c);
  }, cmd);

const until = async (page: any, what: string, pred: (s: Record<string, any>) => boolean, timeout = 60_000) => {
  const end = Date.now() + timeout;
  for (;;) {
    const s = await el(page);
    if (pred(s)) return s;
    if (Date.now() > end) throw new Error(`waited for ${what}: ${JSON.stringify(s)}`);
    await page.waitForTimeout(500);
  }
};

// The main menu's backdrop (`web/assets/menu/causeway.jpg`, `art/tools/menu_backdrop.py`): the
// Causeway on the high tier at 1920x1080, the fog's darkness and the HUD off, from a few places;
// pictures to e2e/artifacts/backdrop-<spot>.png, one of which the tool makes the backdrop.
const MID = 1920;
const SPOTS: [string, number, number, number][] = [
  ["nexus", 2300, MID, 0.8],
  ["turret", 5000, MID, 0.75],
  ["middle", 7680, MID, 0.7],
  ["dusk", 15360 - 2600, MID, 0.8],
];

test("the menu's backdrop", async ({ page }) => {
  test.setTimeout(2_400_000);
  const logs = collectConsole(page);
  await page.setViewportSize({ width: 1920, height: 1080 });
  await page.addInitScript(() => localStorage.setItem("el.taught", "1"));
  await page.goto(`/?gfx=webgl2&quality=high&name=Painter&queue=1`);
  try {
    await until(page, "champion select", (s) => s.phase === "selecting", 300_000);
    await push(page, { t: "ready" });
    await until(page, "a seat in a match", (s) => s.started === true, 600_000);
    await until(page, "the art", (s) => s.art === true, 600_000);
    await push(page, { t: "pick", card: 0 });
    await push(page, { t: "fog", on: false });
    await push(page, { t: "hud", on: false });
    // The first wave on its way down the lane.
    await page.waitForTimeout(60_000);
    for (const [name, x, y, zoom] of SPOTS) {
      await push(page, { t: "camera", x, y, zoom });
      await page.waitForTimeout(8_000);
      await page.screenshot({ path: `artifacts/backdrop-${name}.png` });
    }
  } catch (e) {
    console.log(logs.slice(-40).join("\n"));
    throw e;
  }
});
