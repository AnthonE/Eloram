import { spawn } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
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

// The server (scripts/e2e.sh starts it on a records directory) is killed outright mid-match and
// started again: the next one takes the match up from its last checkpoint and its record, and the
// page, dialling again on its own, is back in the same seat of the same match, walking predicted
// exactly.
test("a server killed mid-match: the page back in its seat on the next one", async ({ page, request }, info) => {
  const logs = collectConsole(page);
  const cmd = process.env.EL_SERVER_CMD!.split(/\s+/).filter(Boolean);
  const pidfile = process.env.EL_PIDFILE!;
  await page.goto(`/?gfx=${info.project.name}&name=Survivor`);
  let s: Record<string, any>;
  try {
    s = await until(page, "champion select", (s) => s.phase === "selecting", 150_000);
    await push(page, { t: "ready" });
    // Long enough for a checkpoint (every 5 s here) to be verified.
    s = await until(page, "the match", (s) => s.started === true && s.tick > 450, 180_000);
    const { seat, champion, session, tick } = s;
    const before = await (await request.get("/status")).json();
    expect(before.game.referee.checkpoints).toBeGreaterThan(0);

    process.kill(Number(readFileSync(pidfile, "utf8")), "SIGKILL");
    const next = spawn(cmd[0], cmd.slice(1), { detached: true, stdio: "ignore" });
    writeFileSync(pidfile, String(next.pid));
    next.unref();

    s = await until(
      page,
      "back in the match",
      (s) => s.phase === "playing" && s.started === true && s.session !== session,
      120_000,
    );
    expect(s.seat).toBe(seat);
    expect(s.champion).toBe(champion);
    expect(s.tick).toBeGreaterThan(tick - 60);
    expect(s.errors).toBe(0);
    const status = await (await request.get("/status")).json();
    expect(status.game.matches).toBe(1);
    expect(status.game.referee.divergences).toBe(0);

    // A walk on the new server, predicted exactly.
    const x = s.team === 0 ? s.x + 1500 : s.x - 1500;
    await push(page, { t: "move", x, y: s.y });
    s = await until(page, "the walk", (s) => Math.abs(s.x - x) < 150, 90_000);
    expect(s.walk_err_max).toBeLessThan(1);
    console.log(`recover status: ${JSON.stringify(s)}`);
  } catch (e) {
    console.log(logs.slice(-40).join("\n"));
    throw e;
  }
});
