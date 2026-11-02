import { test } from "@playwright/test";
import { collectConsole, el } from "./util";

// The draw calls a frame (`docs/ART.md`, "Not measured: the draw-call count"): WebGL2's draws
// counted in the page, a frame at a time, on each tier, with the camera over a few places in a
// match. A draw's count doesn't depend on the GPU, so SwiftShader's is a real GPU's. Each draw is
// keyed by its shader program and its element count, which tells one mesh drawn many times apart
// from many meshes; the commonest keys are printed, and `artifacts/draws-<tier>.json` keeps them.
const PROBE = () => {
  const P = (window as any).WebGL2RenderingContext?.prototype;
  if (!P) return;
  const ids = new Map<unknown, number>();
  // Each program's name: the functions its shaders define (naga keeps WGSL's names), so a key
  // can be told for the ground's, a unit's, the UI's.
  const names = new Map<unknown, string>();
  const shaders = new Map<unknown, string>();
  const attached = new Map<unknown, unknown[]>();
  const source = P.shaderSource;
  P.shaderSource = function (sh: unknown, src: string) {
    shaders.set(sh, src);
    return source.call(this, sh, src);
  };
  const attach = P.attachShader;
  P.attachShader = function (p: unknown, sh: unknown) {
    attached.set(p, [...(attached.get(p) ?? []), sh]);
    return attach.call(this, p, sh);
  };
  const label = (p: unknown) => {
    const fns = new Set<string>();
    for (const sh of attached.get(p) ?? []) {
      for (const m of (shaders.get(sh) ?? "").matchAll(/^\s*[\w<>]+\s+(\w+)\s*\([^;{]*\)\s*\{/gm)) {
        if (m[1] !== "main") fns.add(m[1]);
      }
    }
    return [...fns].slice(-4).join(",");
  };
  let prog: unknown = null;
  let cur: { draws: number; instances: number; by: Record<string, number> } | null = null;
  const frames: (typeof cur)[] = [];
  const use = P.useProgram;
  P.useProgram = function (p: unknown) {
    if (p && !ids.has(p)) {
      ids.set(p, ids.size + 1);
      names.set(ids.size, label(p));
    }
    prog = p;
    return use.call(this, p);
  };
  const wrap = (name: string, countAt: number, instAt: number) => {
    const f = P[name];
    P[name] = function (...a: any[]) {
      if (cur) {
        cur.draws++;
        cur.instances += instAt >= 0 ? a[instAt] : 1;
        const key = `p${ids.get(prog)}:${a[countAt]}${instAt >= 0 ? "x" + a[instAt] : ""}`;
        cur.by[key] = (cur.by[key] ?? 0) + 1;
      }
      return f.apply(this, a);
    };
  };
  wrap("drawElements", 1, -1);
  wrap("drawArrays", 2, -1);
  wrap("drawElementsInstanced", 1, 4);
  wrap("drawArraysInstanced", 2, 3);
  wrap("drawRangeElements", 3, -1);
  // A frame is what's drawn between two of the page's animation frames.
  const tick = () => {
    if (cur) frames.push(cur);
    cur = { draws: 0, instances: 0, by: {} };
    if (frames.length > 64) frames.shift();
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
  (window as any).__draws = () => frames.slice();
  (window as any).__drawPrograms = () => Object.fromEntries(names);
  (window as any).__drawsReset = () => frames.splice(0);
};

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

const MID = 1920;
const SPOTS: [string, number, number][] = [
  ["base", 2300, MID],
  ["outer-turret", 5200, MID],
  ["middle", 7680, MID],
];

for (const tier of (process.env.EL_DRAWS_TIERS ?? "low,high").split(",")) {
  test(`draw calls a frame, ${tier}`, async ({ page }, info) => {
    test.setTimeout(2_400_000);
    const logs = collectConsole(page);
    await page.addInitScript(PROBE);
    await page.setViewportSize({ width: 960, height: 540 });
    await page.goto(`/?gfx=${info.project.name}&quality=${tier}&name=Counter`);
    await until(page, "champion select", (s) => s.phase === "selecting", 300_000);
    await push(page, { t: "ready" });
    await until(page, "a seat in a match", (s) => s.started === true, 600_000);
    await until(page, "the art", (s) => s.art === true, 600_000);
    await push(page, { t: "pick", card: 0 });
    await until(page, "an augment", (s) => s.augments === 1 && s.screen_open === false);
    // The whole map in sight, as the fog's off: what's drawn is everything the camera can see.
    await push(page, { t: "fog", on: false });
    const out: Record<string, unknown> = {};
    for (const [name, x, y] of SPOTS) {
      await push(page, { t: "camera", x, y, zoom: 1.0 });
      await page.waitForTimeout(6_000);
      await page.evaluate(() => (window as any).__drawsReset());
      await page.waitForTimeout(8_000);
      const frames: { draws: number; instances: number; by: Record<string, number> }[] = await page.evaluate(
        () => (window as any).__draws(),
      );
      const full = frames.filter((f) => f.draws > 0);
      if (!full.length) throw new Error(`no frames drawn at ${name}: ${logs.slice(-10).join("\n")}`);
      const last = full[full.length - 1];
      const keys = Object.entries(last.by).sort((a, b) => b[1] - a[1]);
      const draws = full.map((f) => f.draws).sort((a, b) => a - b);
      console.log(
        `${tier} ${name}: ${draws[draws.length >> 1]} draws a frame (${draws[0]} to ${draws[draws.length - 1]} over ${full.length} frames), ` +
          `${keys.length} distinct; commonest: ${keys
            .slice(0, 12)
            .map(([k, n]) => `${k}×${n}`)
            .join(" ")}`,
      );
      const programs: Record<string, string> = await page.evaluate(() => (window as any).__drawPrograms());
      const used = new Set(keys.slice(0, 12).map(([k]) => k.split(":")[0].slice(1)));
      console.log([...used].map((p) => `  p${p}: ${programs[p]}`).join("\n"));
      out[name] = { draws, last: last.by, programs };
    }
    const fs = await import("node:fs");
    fs.mkdirSync("artifacts", { recursive: true });
    fs.writeFileSync(`artifacts/draws-${tier}.json`, JSON.stringify(out, null, 1));
  });
}
