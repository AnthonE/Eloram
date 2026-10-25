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

// A match from the queue, with the server's bots in the other nine seats (`--fill-after 1`):
// seated at level 3 with ARAM's gold on the platform, an augment picked, an item bought, a skill
// levelled and cast, a walk down the lane predicted exactly, and fog that hides the enemy until
// someone of ours can see them.
test("a match: queue, champion select, augment, shop, walk, cast, fog", async ({ page }, info) => {
  const logs = collectConsole(page);
  const shots = `artifacts/match-${info.project.name}`;
  await page.goto(`/?gfx=${info.project.name}&name=Tester`);
  let s: Record<string, any>;
  let chosen = "";
  try {
    // Champion select: a reroll benches the dealt champion; taking it back off the bench swaps
    // the new one onto it; ready starts the match (the bots in the other seats keep theirs).
    s = await until(page, "champion select", (s) => s.phase === "selecting" && s.select_champion_id >= 0, 150_000);
    expect(s.select_rerolls).toBe(2);
    const dealt = s.select_champion_id;
    await page.screenshot({ path: `${shots}-select.png` });
    await push(page, { t: "champ_reroll" });
    s = await until(page, "a reroll", (s) => s.select_rerolls === 1 && s.select_bench_ids === String(dealt));
    const rerolled = s.select_champion_id;
    expect(rerolled).not.toBe(dealt);
    await push(page, { t: "champ_swap", champ: dealt });
    s = await until(page, "the bench", (s) => s.select_champion_id === dealt && s.select_bench_ids === String(rerolled));
    chosen = s.select_champion;
    await push(page, { t: "ready" });
    s = await until(page, "a seat in a match", (s) => s.started === true, 60_000);
  } catch (e) {
    console.log(logs.slice(-40).join("\n"));
    throw e;
  }
  expect(s.phase).toBe("playing");
  expect(s.champion).toBe(chosen);
  expect(s.level).toBe(3);
  expect(s.gold).toBeGreaterThanOrEqual(1400);
  expect(s.in_shop).toBe(true);
  expect(s.enemy_champions).toBe(0);
  // The match's headliner, shown as it starts.
  expect(s.headliner).toBeTruthy();
  await page.screenshot({ path: `${shots}-start.png` });

  // The first augment screen is open at the start: take the first card.
  expect(s.screen_open).toBe(true);
  await push(page, { t: "pick", card: 0 });
  s = await until(page, "an augment", (s) => s.augments === 1 && s.screen_open === false);

  // The shop: an Iron Knife (350).
  const gold = s.gold;
  await push(page, { t: "shop", open: true });
  await push(page, { t: "buy", item: 0 });
  s = await until(page, "the item", (s) => s.items === "0" && s.gold <= gold - 349);
  await page.screenshot({ path: `${shots}-shop.png` });
  await push(page, { t: "shop", open: false });

  // A skill point into Q; Blink (D) down the lane.
  await push(page, { t: "level", slot: 0 });
  s = await until(page, "Q levelled", (s) => s.ranks.startsWith("1,"));
  // Q's indicator, as if its key were held with indicators on: its range and its shape.
  await push(page, { t: "aim", slot: 0 });
  s = await until(page, "Q's indicator", (s) => s.aiming === 0);
  await page.screenshot({ path: `${shots}-aim.png` });
  await push(page, { t: "aim", slot: null });
  s = await until(page, "the key let go", (s) => s.aiming === -1);
  const ahead = (x: number) => (s.team === 0 ? x : 15360 - x);
  const before = s.x;
  await push(page, { t: "cast", slot: 4, x: ahead(3000), y: 1920 });
  s = await until(page, "Blink on cooldown", (s) => Number(s.cds.split(",")[4]) > 0);
  expect(Math.abs(s.x - before)).toBeGreaterThan(200);

  // Walk down the lane to the outer turret: predicted exactly.
  await push(page, { t: "move", x: ahead(5000), y: 1920 });
  s = await until(page, "the walk", (s) => Math.abs(s.x - ahead(5000)) < 150, 90_000);
  await page.screenshot({ path: `${shots}-lane.png` });
  expect(s.walk_err_max).toBeLessThan(1);

  // Minions come at 0:50: the enemy's are seen only once ours meet them.
  s = await until(page, "the enemy in sight", (s) => s.enemy_mobile > 0, 120_000);
  const shot = await page.screenshot({ path: `${shots}-fight.png` });
  expect(luminanceStdDev(shot)).toBeGreaterThan(5);

  s = await el(page);
  console.log(`match status: ${JSON.stringify(s)}`);
  expect(s.errors).toBe(0);
  expect(s.datagrams).toBeGreaterThan(600);
  expect(s.orders_unacked).toBeLessThanOrEqual(2);
  const status = await (await page.request.get("/status")).json();
  expect(status.game.matches).toBe(1);
  expect(status.hot_path_allocations).toBe(0);
});

// League's WASD layout (`?keys=wasd`): a held key steers the champion, and it stops when the key
// comes up. (Exactness is the netcode tests': at this page's frame or so a second, the oldest
// commands of a long frame can reach the server after their tick, which then repeats the last
// steer a few ticks, so the stop is a few ticks long here.)
test("WASD: the keys walk the champion", async ({ page }, info) => {
  const logs = collectConsole(page);
  await page.goto(`/?gfx=${info.project.name}&name=Walker&keys=wasd`);
  let s: Record<string, any>;
  try {
    s = await until(page, "champion select", (s) => s.phase === "selecting", 150_000);
    await push(page, { t: "ready" });
    // The base's gates hold champions in for the first 15 s.
    s = await until(page, "the gates open", (s) => s.started === true && s.tick > 15 * 30 + 30, 120_000);
  } catch (e) {
    console.log(logs.slice(-40).join("\n"));
    throw e;
  }
  expect(s.keys).toBe("wasd");
  const before = s.x;
  // Down the lane: D for Dawn, A for Dusk.
  const key = s.team === 0 ? "d" : "a";
  await page.click("canvas");
  await page.keyboard.down(key);
  s = await until(page, "steering", (s) => s.steer === (s.team === 0 ? 1 : 5));
  s = await until(page, "the walk", (s) => Math.abs(s.x - before) > 400, 60_000);
  await page.keyboard.up(key);
  s = await until(page, "the stop", (s) => s.steer === 0);
  const stopped = s.x;
  await page.waitForTimeout(3_000);
  s = await el(page);
  // Held, three seconds would have been about a thousand units.
  expect(Math.abs(s.x - stopped)).toBeLessThan(150);
  expect(s.steer).toBe(0);
});
