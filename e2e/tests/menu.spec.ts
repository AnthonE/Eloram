import { expect, test } from "@playwright/test";
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
    await page.waitForTimeout(250);
  }
};

// The main menu (`el-client/src/front/`): a plain load opens it, signed in or not; its pages; RANKED
// as a guest says why not; PLAY queues and the menu goes as champion select comes; LEAVE MATCH
// lets the seat go for good, back to the menu, and a reload doesn't pull the player back in.
test("the main menu: its pages, PLAY, and leaving a match for it", async ({ page }, info) => {
  test.setTimeout(420_000);
  const logs = collectConsole(page);
  const shots = `artifacts/menu-${info.project.name}`;
  await page.goto(`/?gfx=${info.project.name}&name=Menu`);
  try {
    let s = await until(page, "the menu", (s) => s.front === true && s.phase === "lobby" && s.play_label === "PLAY", 120_000);
    expect(s.identity_name).toBe("GUEST");
    expect(s.front_nav).toBe("play");
    await page.waitForTimeout(1500);
    await page.screenshot({ path: `${shots}-play.png` });

    // A server without Elo Pros holds no ranked matches, and says so.
    await push(page, { t: "play", mode: "ranked" });
    s = await until(page, "ranked refused", (s) => /ranked/i.test(s.front_status), 30_000);
    expect(s.phase).toBe("lobby");
    expect(s.front).toBe(true);

    // WATCH reads the server's records.json (an empty list here, but read).
    await push(page, { t: "front", nav: "watch" });
    await until(page, "the watch page", (s) => s.front_nav === "watch", 20_000);
    await page.waitForTimeout(3000);
    s = await el(page);
    expect(s.watch_error).toBe("");
    for (const nav of ["watch", "practice", "store", "profile"]) {
      await push(page, { t: "front", nav });
      await until(page, `the ${nav} page`, (s) => s.front_nav === nav, 20_000);
      await page.waitForTimeout(1200);
      await page.screenshot({ path: `${shots}-${nav}.png` });
    }

    // PLAY: the queue, then champion select takes the menu away.
    await push(page, { t: "front", nav: "play" });
    await push(page, { t: "play", mode: "normal" });
    s = await until(page, "champion select", (s) => s.phase === "selecting", 120_000);
    expect(s.front).toBe(false);
    await push(page, { t: "ready" });
    s = await until(page, "a seat in a match", (s) => s.phase === "playing" && s.started === true, 120_000);

    // LEAVE MATCH: back at the menu, the seat let go.
    await push(page, { t: "leave" });
    s = await until(page, "the menu again", (s) => s.front === true && s.phase === "lobby", 30_000);
    await page.reload();
    await until(page, "the menu after a reload", (s) => s.front === true && s.phase === "lobby", 120_000);
    await page.waitForTimeout(4000);
    s = await el(page);
    expect(s.phase).toBe("lobby");
  } catch (e) {
    console.log(logs.slice(-40).join("\n"));
    throw e;
  }
});
