import { expect, test } from "@playwright/test";
import { collectConsole, el } from "./util";

// The UI's look, a picture a screen (`docs/DESIGN.md`, "The look of the UI"): the main menu's
// pages, the queue's card, champion select, the HUD at rest and with an ability cooling down, the
// augment cards, a tooltip, the shop, the scoreboard, the Esc menu's pages. Pictures to look at,
// at the design's own 1600 by 900 (`EL_UI_SIZE=960x540` for a small window's), into
// `artifacts/ui-<project>-<name>.png`; what it holds is only that each screen came up and nothing
// threw. `EL_UI_ONLY=menu` stops after the menu's pages.

const push = (page: any, cmd: Record<string, unknown>) =>
  page.evaluate((c: Record<string, unknown>) => ((window as any).__elInbox ??= []).push(c), cmd);

const until = async (page: any, what: string, pred: (s: Record<string, any>) => boolean, timeout = 60_000) => {
  const end = Date.now() + timeout;
  for (;;) {
    const s = await el(page);
    if (pred(s)) return s;
    if (Date.now() > end) throw new Error(`waited for ${what}: ${JSON.stringify(s)}`);
    await page.waitForTimeout(300);
  }
};

const [W, H] = (process.env.EL_UI_SIZE || "1600x900").split("x").map(Number);
const ONLY = process.env.EL_UI_ONLY || "";
const QUALITY = process.env.EL_UI_QUALITY || "low";

test.use({ viewport: { width: W, height: H } });

test("the UI, a picture a screen", async ({ page }, info) => {
  test.setTimeout(1_500_000);
  const logs = collectConsole(page);
  const shots = `artifacts/ui-${info.project.name}`;
  // A few frames drawn (SwiftShader's are slow), then the picture.
  const shoot = async (name: string, frames = 3) => {
    const f = (await el(page)).frames ?? 0;
    await until(page, `${name}'s frames`, (s) => (s.frames ?? 0) >= f + frames, 120_000).catch(() => null);
    await page.screenshot({ path: `${shots}-${name}.png` });
  };
  await page.addInitScript(() => localStorage.setItem("el.taught", "1"));
  await page.goto(`/?gfx=${info.project.name}&quality=${QUALITY}&name=Tour&camera=locked`);
  let s: Record<string, any>;
  try {
    s = await until(page, "the menu", (s) => s.front === true && s.phase === "lobby" && s.play_label === "PLAY", 180_000);
    await page.waitForTimeout(1500);
    await shoot("menu-play");
    // The pointer over PLAY, for its hover.
    await page.mouse.move(W * 0.17, H * 0.5);
    for (const nav of ["practice", "watch", "friends", "news", "store", "profile", "options"]) {
      await push(page, { t: "front", nav });
      await until(page, `the ${nav} page`, (s) => s.front_nav === nav, 20_000);
      await page.waitForTimeout(800);
      await shoot(`menu-${nav}`);
    }
    if (ONLY === "menu") return;

    // The queue's card, then champion select.
    await push(page, { t: "front", nav: "play" });
    await push(page, { t: "play", mode: "normal" });
    await until(page, "the queue or select", (s) => s.phase === "queued" || s.phase === "selecting", 60_000);
    await shoot("queue", 1);
    s = await until(page, "champion select", (s) => s.phase === "selecting" && s.select_champion_id >= 0, 150_000);
    // At once: the bots are ready already, and the select's seconds are few.
    await shoot("select", 2);
    await push(page, { t: "ready" });
    s = await until(page, "a seat in a match", (s) => s.phase === "playing" && s.started === true, 120_000);
    await until(page, "the champions' models", (s) => s.champion_models >= 10, 120_000).catch(() => null);
    await page.waitForTimeout(2500);
    await shoot("hud-start");

    // The augment cards, while their screen is open.
    if (s.screen_open || (await el(page)).screen_open) {
      await shoot("cards");
      await push(page, { t: "pick", card: 0 });
      await until(page, "the card taken", (s) => !s.screen_open, 30_000).catch(() => null);
    }
    for (const slot of [0, 1, 2]) await push(page, { t: "level", slot });
    await until(page, "Q levelled", (s) => String(s.ranks).startsWith("1,"), 30_000).catch(() => null);
    s = await el(page);
    const ahead = (x: number) => (s.team === 0 ? x : 15360 - x);
    // The shop (open only on the platform: before Blink leaves it), and the scoreboard while Tab
    // is held.
    await push(page, { t: "shop", open: true });
    await page.waitForTimeout(600);
    await shoot("shop");
    await push(page, { t: "shop", open: false });
    await page.keyboard.down("Tab");
    await page.waitForTimeout(600);
    await shoot("scoreboard");
    await page.keyboard.up("Tab");

    // Abilities cooling down: their sweeps and seconds.
    for (const slot of [0, 1, 2, 4]) await push(page, { t: "cast", slot, x: ahead(2600 + 300 * slot), y: 1920 });
    await page.waitForTimeout(900);
    await shoot("hud-cooldowns", 2);

    // A tooltip: the pointer over Q (the bottom panel's middle, a little left).
    await page.mouse.move(W * 0.5 - 100 * (W / 1600), H - 78 * (H / 900));
    await page.waitForTimeout(600);
    await shoot("tooltip", 2);
    await page.mouse.move(W * 0.5, H * 0.4);

    // The Esc menu's pages.
    for (const tab of ["controls", "graphics", "sound"]) {
      await push(page, { t: "menu", open: true, tab });
      await until(page, `the menu's ${tab}`, (s) => s.menu_open === true && s.menu_tab === tab, 20_000).catch(() => null);
      await page.waitForTimeout(500);
      await shoot(`esc-${tab}`);
    }
    await push(page, { t: "menu", open: false });

    // Down the lane, the HUD over a fight; and dead, if the walk ends that way.
    if (ONLY !== "hud") {
      await push(page, { t: "move", x: ahead(6200), y: 1920 });
      await until(page, "the enemy in sight", (s) => s.enemy_mobile > 0, 150_000).catch(() => null);
      await shoot("hud-lane");
      await push(page, { t: "move", x: ahead(12500), y: 1920 });
      const dead = await until(page, "the death screen", (s) => s.death_screen === true, 150_000).catch(() => null);
      if (dead) {
        await page.waitForTimeout(1500);
        await shoot("death");
      }
    }
    s = await el(page);
    expect(s.errors ?? 0).toBe(0);
  } catch (e) {
    console.log(logs.slice(-40).join("\n"));
    throw e;
  }
});
