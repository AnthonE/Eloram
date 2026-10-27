import { expect, test } from "@playwright/test";
import { collectConsole, el, luminanceStdDev } from "./util";

const until = async (page: any, what: string, pred: (s: Record<string, any>) => boolean, timeout = 60_000) => {
  const end = Date.now() + timeout;
  for (;;) {
    const s = await el(page);
    if (pred(s)) return s;
    if (Date.now() > end) throw new Error(`waited for ${what}: ${JSON.stringify(s)}`);
    await page.waitForTimeout(500);
  }
};

// An agent plays (scripts/e2e.sh starts one): the page watches its match from the record, a
// minute behind, every hash checked on the way; then plays the same record back from the start.
test("watching a live match, then its replay", async ({ page, request }, info) => {
  const logs = collectConsole(page);
  // A record live long enough to show more than its header.
  let rec: any;
  const end = Date.now() + 150_000;
  for (;;) {
    const list = await (await request.get("/records.json")).json();
    // Served a minute behind, with a checkpoint (every 10 s here) inside what's served.
    rec = list.find((r: any) => r.live && r.ticks > 60 * 30 + 450 && r.served_checkpoint > 0);
    if (rec || Date.now() > end) break;
    await new Promise((r) => setTimeout(r, 2_000));
  }
  expect(rec, "a live match on the server").toBeTruthy();

  await page.goto(`/?gfx=${info.project.name}&watch=${rec.id}`);
  let s: Record<string, any>;
  try {
    s = await until(page, "the match, watched", (s) => s.phase === "watching" && s.watch_tick > 300 && s.watch_units > 20, 120_000);
  } catch (e) {
    console.log(logs.slice(-30).join("\n"));
    throw e;
  }
  expect(s.watch_live).toBe(true);
  // From the server's checkpoint, not the record's first tick.
  expect(s.watch_from).toBeGreaterThan(0);
  // A minute behind: never past what the server serves.
  expect(s.watch_tick).toBeLessThanOrEqual(rec.ticks);
  const shot = await page.screenshot({ path: `artifacts/watch-${info.project.name}.png` });
  expect(luminanceStdDev(shot)).toBeGreaterThan(5);
  s = await until(page, "hashes checked", (s) => s.watch_hashes >= 3);
  expect(s.watch_error).toBeUndefined();

  // The same record, from the start.
  await page.goto(`/?gfx=${info.project.name}&replay=${encodeURIComponent(`/records/${rec.id}`)}`);
  s = await until(page, "the replay", (s) => s.phase === "watching" && s.watch_hashes >= 2, 120_000);
  expect(s.watch_live).toBe(false);
  expect(s.watch_error).toBeUndefined();
  await page.screenshot({ path: `artifacts/replay-${info.project.name}.png` });
  console.log(`watch status: ${JSON.stringify(s)}`);
});
