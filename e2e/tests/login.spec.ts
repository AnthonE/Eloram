import { execFileSync } from "node:child_process";
import { expect, test } from "@playwright/test";
import { collectConsole, el } from "./util";

// Wallet sign-in in the browser. A stub wallet (window.ethereum) signs with a throwaway test key
// through the el-auth example, the same path a real one takes: personal_sign over the text the
// game writes, checked by the server. A declined signature fails cleanly; then a sign-in puts the
// wallet on the store's roster (/online.json); a reload comes back on the resume token without
// asking the wallet again.
const SIGNER = `${process.env.CARGO_TARGET_DIR ?? "../target"}/release/examples/sign`;
// Test key 1's address.
const ADDRESS = "0x7e5f4552091a69125d5dfcb7b8c2659029395bdf";

test("sign in with a wallet, then come back on the token", async ({ context, page, request }, info) => {
  let signatures = 0;
  let decline = true;
  await context.exposeFunction("__signHex", (hex: string) => {
    if (decline) {
      decline = false;
      return { error: "User rejected the request." };
    }
    signatures += 1;
    const message = Buffer.from(hex.slice(2), "hex");
    return { signature: execFileSync(SIGNER, ["1"], { input: message }).toString().trim() };
  });
  await context.addInitScript((address) => {
    (window as any).ethereum = {
      async request({ method, params }: { method: string; params?: any[] }) {
        if (method === "eth_accounts" || method === "eth_requestAccounts") return [address];
        if (method === "personal_sign") {
          const out = await (window as any).__signHex(params![0]);
          // Wallets reject with a plain object ({ code, message }), not an Error.
          if (out.error) throw { code: 4001, message: out.error };
          return out.signature;
        }
        throw { code: 4200, message: `unsupported: ${method}` };
      },
      on() {},
    };
  }, ADDRESS);
  const logs = collectConsole(page);
  const until = async (what: string, pred: (s: Record<string, any>) => boolean, timeout = 120_000) => {
    const end = Date.now() + timeout;
    for (;;) {
      const s = await el(page);
      if (pred(s)) return s;
      if (Date.now() > end) {
        console.log(logs.slice(-30).join("\n"));
        throw new Error(`waited for ${what}: ${JSON.stringify(s)}`);
      }
      await page.waitForTimeout(250);
    }
  };
  const url = `/?gfx=${info.project.name}&name=Wallet&signin=1&queue=0`;

  // Declined: said so, and nobody is on.
  await page.goto(url);
  let s = await until("the refusal", (s) => s.phase === "failed");
  expect(s.session_error).toContain("didn't sign in");
  expect((await (await request.get("/online.json")).json()).online).toEqual([]);

  // Signed: welcomed as the wallet, on the store's roster.
  await page.goto(url);
  s = await until("signed in", (s) => s.signed_in === true && s.phase === "lobby");
  expect(signatures).toBe(1);
  const online = (await (await request.get("/online.json")).json()).online;
  expect(online).toHaveLength(1);
  expect(online[0]).toMatchObject({ wallet: ADDRESS, name: "Wallet", agent: false });

  // Back on the token: no second signature.
  await page.reload();
  s = await until("resumed", (s) => s.signed_in === true && s.phase === "lobby");
  expect(signatures).toBe(1);
});
