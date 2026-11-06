import { execFileSync } from "node:child_process";
import { createPrivateKey, sign as edSign } from "node:crypto";
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

  // The card asks how (the e2e server takes passes too).
  const ethereum = () => page.getByRole("button", { name: "ETHEREUM WALLET" }).click();

  // Declined: said so, and nobody is on.
  await page.goto(url);
  await ethereum();
  let s = await until("the refusal", (s) => s.phase === "failed");
  expect(s.session_error).toContain("didn't sign in");
  expect((await (await request.get("/online.json")).json()).online).toEqual([]);

  // Signed: welcomed as the wallet, on the store's roster.
  await page.goto(url);
  await ethereum();
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

// A player with only a Solana wallet, and one with only an Elo Pros login. A stub Phantom signs with
// `signin_check.json`'s throwaway key (Ed25519 seed 07…07); a stub `EloSignIn` mints a pass with Elo
// Pros' test key (`account_token.json`, its `seed_hex`; `scripts/e2e.sh` names its public half).
const SOL = "GmaDrppBC7P5ARKV8g3djiwP89vz1jLK23V2GBjuAEGB";
const ACCT = "acct_0123456789abcdef0123";
const ed25519 = (seedHex: string) =>
  createPrivateKey({
    key: Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), Buffer.from(seedHex, "hex")]),
    format: "der",
    type: "pkcs8",
  });
const b64url = (b: Buffer) => b.toString("base64url");
function mintPass(game: string, nonce: string) {
  const text = `elo account\nid: ${ACCT}\nfor: play:${game}\nshard: 127.0.0.1\nnonce: ${nonce}\nwallet: none\nexp: ${Math.floor(Date.now() / 1000) + 600}`;
  const key = ed25519("2f3e65db2d7c08d3d45cb88040eca8badb2b6287066b2d30a1861756938b6536");
  return `${b64url(Buffer.from(text))}.${b64url(edSign(null, Buffer.from(text), key))}`;
}
const signedIn = async (page: any) => {
  for (const end = Date.now() + 120_000; Date.now() < end; await page.waitForTimeout(250)) {
    const s = await el(page);
    if (s.signed_in === true && s.phase === "lobby") return;
  }
  throw new Error(`not signed in: ${JSON.stringify(await el(page))}`);
};
const wallets = async (request: any) =>
  ((await (await request.get("/online.json")).json()).online as any[]).map((e) => e.wallet);

test("sign in with only a Solana wallet", async ({ context, page, request }, info) => {
  await context.exposeFunction("__solSign", (hex: string) =>
    Array.from(edSign(null, Buffer.from(hex, "hex"), ed25519("07".repeat(32)))),
  );
  await context.addInitScript((sol) => {
    const pk = { toString: () => sol };
    (window as any).phantom = {
      solana: {
        isPhantom: true,
        publicKey: null as any,
        async connect() {
          this.publicKey = pk;
          return { publicKey: pk };
        },
        async signMessage(bytes: Uint8Array) {
          const hex = Array.from(bytes).map((b) => b.toString(16).padStart(2, "0")).join("");
          return { signature: Uint8Array.from(await (window as any).__solSign(hex)), publicKey: pk };
        },
      },
    };
  }, SOL);
  await page.goto(`/?gfx=${info.project.name}&name=Sol&signin=1&queue=0`);
  await page.getByRole("button", { name: "PHANTOM" }).click();
  await signedIn(page);
  expect(await wallets(request)).toContain(SOL);
});

test("sign in with only an Elo Pros login, and come back on the token", async ({ context, page, request }, info) => {
  await context.exposeFunction("__mintPass", mintPass);
  await context.addInitScript(() => {
    (window as any).EloSignIn = {
      popup: async ({ game, nonce }: { game: string; nonce: string }) => ({ pass: await (window as any).__mintPass(game, nonce) }),
    };
  });
  await page.goto(`/?gfx=${info.project.name}&name=Acct&signin=1&queue=0`);
  await page.getByRole("button", { name: "SIGN IN WITH ELO PROS" }).click();
  await signedIn(page);
  expect(await wallets(request)).toContain(ACCT);
  // A reload needs no second pass.
  await page.reload();
  await signedIn(page);
  expect(await wallets(request)).toContain(ACCT);
});
