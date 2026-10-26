import { expect, test } from "@playwright/test";
import { collectConsole, el, luminanceStdDev } from "./util";

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
    await page.waitForTimeout(250);
  }
};

// The map's look (docs/ART.md): a match from the queue, the art loaded, then the camera put over
// each place worth looking at, the fog's darkness off so the whole of it shows, and one look with
// the fog on. Pictures to e2e/artifacts/art-*.png, to put beside the references.
const MID = 1920;
const SPOTS: [string, number, number, number][] = [
  ["fountain", 1000, MID, 1.0],
  ["nexus", 2300, MID, 1.0],
  ["inhibitor", 3500, MID, 1.0],
  ["outer-turret", 5200, MID, 1.0],
  ["brush", 5900, MID - 400, 1.0],
  ["middle", 7680, MID, 1.0],
  ["far-edge", 6200, 2940, 1.0],
  ["near-edge", 4300, 900, 1.0],
  ["dusk-base", 15360 - 2300, MID, 1.25],
];

test("the map's look", async ({ page }, info) => {
  // SwiftShader takes a minute or more a picture at this size.
  test.setTimeout(1_500_000);
  const logs = collectConsole(page);
  const shots = `artifacts/art-${info.project.name}`;
  await page.setViewportSize({ width: 1280, height: 720 });
  // The high tier, which software rendering would otherwise not get.
  await page.goto(`/?gfx=${info.project.name}&quality=high&name=Painter`);
  try {
    await until(page, "champion select", (s) => s.phase === "selecting", 150_000);
    await push(page, { t: "ready" });
    await until(page, "a seat in a match", (s) => s.started === true, 120_000);
    await until(page, "the art", (s) => s.art === true, 240_000);
    // The first augment's cards, out of the way.
    await push(page, { t: "pick", card: 0 });
    await until(page, "an augment", (s) => s.augments === 1 && s.screen_open === false);
    // Under the fog, at the champion, as a player first sees it.
    await page.waitForTimeout(8_000);
    await page.screenshot({ path: `${shots}-start.png` });
    await push(page, { t: "fog", on: false });
    for (const [name, x, y, zoom] of SPOTS) {
      await push(page, { t: "camera", x, y, zoom });
      // SwiftShader draws about a frame a second: a few, for the fog to clear and the frame to settle.
      await page.waitForTimeout(5_000);
      const png = await page.screenshot({ path: `${shots}-${name}.png` });
      expect(luminanceStdDev(png), name).toBeGreaterThan(8);
    }
  } finally {
    const errors = logs.filter((l) => l.startsWith("[error]") || l.startsWith("[pageerror]"));
    if (errors.length) console.log(errors.join("\n"));
  }
});
