import { expect, test } from "@playwright/test";
import { collectConsole, el, luminanceStdDev } from "./util";

// Transport spike: the Bevy client, compiled to wasm, reaches the server over WebTransport
// (self-signed certificate pinned by hash), round-trips datagrams and a control-stream frame.
test("WebTransport echo from the Bevy client", async ({ page }, info) => {
  const logs = collectConsole(page);
  await page.goto(`/?mode=echo&gfx=${info.project.name}`);
  try {
    await page.waitForFunction(() => {
      const s = (window as any).__el;
      return s?.connected === true && s?.pongs >= 10 && s?.stream_ok === true;
    }, null, { timeout: 120_000 });
  } catch (e) {
    console.log(logs.join("\n"));
    console.log(JSON.stringify(await el(page)));
    throw e;
  }
  const status = await el(page);
  console.log(`spike status: ${JSON.stringify(status)}`);
  expect(status.rtt_ms).toBeLessThan(50);
  const shot = await page.screenshot({ path: `artifacts/spike-${info.project.name}.png` });
  expect(luminanceStdDev(shot)).toBeGreaterThan(2);
});

// The game server says Hello back: the client's Hello gets a Welcome with its session.
test("the game server welcomes a Hello", async ({ page }, info) => {
  const logs = collectConsole(page);
  const game = process.env.EL_GAME_URL ?? "http://127.0.0.1:8081";
  await page.goto(`${game}/?gfx=${info.project.name}&name=Spike`);
  try {
    await page.waitForFunction(() => {
      const s = (window as any).__el;
      return s?.connected === true && typeof s?.session === "number";
    }, null, { timeout: 120_000 });
  } catch (e) {
    console.log(logs.join("\n"));
    console.log(JSON.stringify(await el(page)));
    throw e;
  }
  const status = await el(page);
  expect(status.rejected).toBeUndefined();
  const players = await (await page.request.get(`${game}/status.json`)).json();
  expect(players.players).toBe(1);
});

// The page's module: WebGPU where the browser has an adapter (the `webgpu` project), WebGL 2 where
// it hasn't, and WebGL 2 once this WebGPU build has failed here (loader.js, `build.json`).
test("the page picks its module", async ({ page }, info) => {
  const picked = async () => {
    await page.waitForFunction(() => !!(window as any).EL_CONFIG?.gfx, null, { timeout: 60_000 });
    return page.evaluate(() => (window as any).EL_CONFIG.gfx);
  };
  await page.goto(`/?mode=echo`);
  const built = await page.evaluate(async () => (await fetch("./build.json", { cache: "no-store" })).json());
  if (info.project.name !== "webgpu" || !built.webgpu) {
    expect(await picked()).toBe("webgl2");
    return;
  }
  expect(await picked()).toBe("webgpu");
  await page.evaluate((hash) => localStorage.setItem("el.webgpu-failed", hash), built.webgpu);
  await page.reload();
  expect(await picked()).toBe("webgl2");
});
