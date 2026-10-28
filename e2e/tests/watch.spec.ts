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
  test.setTimeout(660_000);
  const logs = collectConsole(page);
  // A record live long enough to show more than its header.
  let rec: any;
  const end = Date.now() + 210_000;
  for (;;) {
    const list = await (await request.get("/records.json")).json();
    // Served a minute behind, with a checkpoint (every 10 s here) inside what's served, taken
    // once the first waves were out (0:50), so there are minions to draw from the start.
    rec = list.find((r: any) => r.live && r.ticks > 60 * 30 + 450 && r.served_checkpoint > 56 * 30);
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
  // The waves, drawn as their models.
  s = await until(page, "the minions drawn", (s) => s.minion_models === 4 && s.minions_drawn > 0);
  const shot = await page.screenshot({ path: `artifacts/watch-${info.project.name}.png` });
  expect(luminanceStdDev(shot)).toBeGreaterThan(5);
  s = await until(page, "hashes checked", (s) => s.watch_hashes >= 3);
  expect(s.watch_error).toBeUndefined();
  // Shown and heard as a player has it: the director's cues, missiles in flight, the champions
  // casting; none of it a watcher's own.
  s = await until(page, "the effects watched", (s) => s.fx_cues > 50 && s.fx_missiles > 0 && s.champion_casts > 0, 90_000);
  expect(s.audio_failed).toBe(0);

  // The same record, from the start.
  await page.goto(`/?gfx=${info.project.name}&replay=${encodeURIComponent(`/records/${rec.id}`)}`);
  s = await until(page, "the replay", (s) => s.phase === "watching" && s.watch_hashes >= 2, 120_000);
  expect(s.watch_live).toBe(false);
  expect(s.watch_error).toBeUndefined();
  // Run on to the first fights (`]` doubles the speed), then shown and heard as the live match was.
  await page.click("canvas");
  for (const speed of [2, 4, 8, 16]) {
    await page.keyboard.press("BracketRight");
    s = await until(page, `the replay at ${speed}x`, (s) => s.watch_speed === speed, 20_000);
  }
  s = await until(page, "the replay's effects", (s) => s.fx_cues > 50 && s.fx_missiles > 0 && s.champion_casts > 0, 120_000);
  expect(s.watch_error).toBeUndefined();
  // The kill feed, naming the champions (a record names no players).
  s = await until(page, "a champion's fall in the feed", (s) => s.feed_deaths > 0, 150_000);
  expect(String(s.feed_last)).toMatch(/^[A-Z][a-z]+ (slew [A-Z][a-z]+|fell)$/);
  await page.screenshot({ path: `artifacts/replay-${info.project.name}.png` });
  console.log(`watch status: ${JSON.stringify(s)}`);
});
