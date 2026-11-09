import { expect, test } from "@playwright/test";
import { collectConsole, el } from "./util";

// What's built once a match is in view (`el-client/src/frame.rs`; `docs/ROADMAP.md`, "The frame,
// measured where it can be"). On wasm Bevy builds a render pipeline in the frame that first needs
// it, so one first needed by a cast, or by the first wave, is a hitch right there. The page draws
// what a match needs before it (`warm.rs`): this plays one as a person would (champion select,
// the match in view, the abilities and Blink cast, down the lane to the first wave meeting the
// other) and counts every pipeline built, and every texture and mesh added, once the match was in
// view, naming each pipeline with the frame it was built in. Counts, not frame times: they hold in
// SwiftShader. The CPU's parts of the frame are logged as the match goes, for the record only.
// `EL_HITCH_QUALITY=high` plays it on the high tier (shadows, multisampling: pipelines of their own);
// `EL_HITCH_WARM=0` without the warm-up, to see what it saves (and the test fails).

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

const line = (s: Record<string, any>) =>
  `late: ${s.pipelines_late} pipelines (${Math.round(s.pipelines_late_ms)} ms of frames), ` +
  `${s.images_late} textures, ${s.meshes_late} meshes; ${s.pipelines} pipelines in all, ` +
  `${s.warm_spawned} warm-up pieces; cpu ${Number(s.cpu_p50).toFixed(1)} ms p50, ${Number(s.cpu_p99).toFixed(1)} p99, ` +
  `${s.cpu_spikes} spikes (${s.cpu_spike_parts}), ${s.cpu_hitches} hitches over ${s.match_frames} frames\n  parts: ${s.cpu_parts}` +
  (s.pipelines_late_what ? `\n  built late: ${s.pipelines_late_what}` : "");

const QUALITY = process.env.EL_HITCH_QUALITY || "low";
const WARM = process.env.EL_HITCH_WARM === "0" ? "&warm=0" : "";

test("nothing built once a match is in view", async ({ page }, info) => {
  test.setTimeout(1_200_000);
  const logs = collectConsole(page);
  await page.addInitScript(() => localStorage.setItem("el.taught", "1"));
  await page.goto(`/?gfx=${info.project.name}&quality=${QUALITY}${WARM}&name=Hitches&camera=locked`);
  let s: Record<string, any>;
  try {
    s = await until(page, "champion select", (s) => s.phase === "selecting" && s.select_champion_id >= 0, 150_000);
    // The team's models come in during champion select, and are warmed as they do.
    s = await until(page, "the team's models", (s) => s.champion_models >= 5, 120_000);
    s = await until(page, "the minions' models", (s) => s.minion_models === 4, 60_000);
    s = await until(page, "the warm-up drawn", (s) => s.pipelines > 0 && s.frames > 0, 60_000);
    const frames = s.frames;
    await until(page, "frames with the models warmed", (s) => s.frames >= frames + 8, 120_000);
    s = await el(page);
    console.log(`champion select: ${line(s)}`);
    await push(page, { t: "ready" });
    s = await until(page, "a seat in a match", (s) => s.started === true, 90_000);
  } catch (e) {
    console.log(logs.slice(-40).join("\n"));
    throw e;
  }
  console.log(`in view: ${line(s)}`);
  // The other team's models come with the match.
  s = await until(page, "the champions' models", (s) => s.champion_models >= 10, 120_000);
  const ahead = (x: number) => (s.team === 0 ? x : 15360 - x);
  console.log(`the other team in: ${line(s)}`);

  // The augment's card, a skill point in each of Q, W and E, Q's indicator held (Bevy's gizmos),
  // each cast down the lane, and Blink.
  if (s.screen_open) await push(page, { t: "pick", card: 0 });
  for (const slot of [0, 1, 2]) await push(page, { t: "level", slot });
  await until(page, "Q levelled", (s) => String(s.ranks).startsWith("1,"), 30_000);
  await push(page, { t: "aim", slot: 0 });
  s = await until(page, "Q's indicator", (s) => s.aiming === 0, 30_000);
  const held = s.frames;
  await until(page, "the indicator drawn", (s) => s.frames >= held + 4, 60_000);
  await push(page, { t: "aim", slot: null });
  await until(page, "the key let go", (s) => s.aiming === -1, 30_000);
  for (const slot of [0, 1, 2, 4]) {
    const before = await el(page);
    await push(page, { t: "cast", slot, x: ahead(2600 + 300 * slot), y: 1920 });
    await until(page, `${"QWE?D"[slot]} cast`, (s) => s.fx_cues > before.fx_cues, 30_000).catch(() => null);
    const f = (await el(page)).frames;
    await until(page, "its effects drawn", (s) => s.frames >= f + 6, 60_000).catch(() => null);
  }
  s = await el(page);
  console.log(`after the casts: ${line(s)}`);

  // Down the lane to where the waves meet: minions walking and swinging, and a hit's flash.
  await push(page, { t: "move", x: ahead(5000), y: 1920 });
  s = await until(page, "the walk", (s) => Math.abs(s.x - ahead(5000)) < 200, 120_000);
  const clip = (s: Record<string, any>, c: string) => new RegExp(`(^|,)${c}:`).test(String(s.minion_clips));
  s = await until(page, "the minions walking", (s) => s.minions_drawn > 0 && clip(s, "run"), 150_000);
  s = await until(page, "the enemy in sight", (s) => s.enemy_mobile > 0, 120_000);
  s = await until(page, "a minion's flash", (s) => s.minion_flashes > 0, 90_000);
  const f = s.frames;
  await until(page, "the fight drawn", (s) => s.frames >= f + 10, 120_000).catch(() => null);
  s = await el(page);
  console.log(`the first waves met: ${line(s)}`);

  expect(s.errors ?? 0).toBe(0);
  expect(s.pipelines_late).toBe(0);
});
