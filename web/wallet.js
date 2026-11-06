// The browser's wallets, for signing in: the game builds the text and checks nothing here; the
// server rebuilds the text from its own challenge and checks the signature. Never logs a signature.
//   elWallet.use("evm" | "solana")    which one signs (signin.js picks; alone, whichever is here)
//   elWallet.kinds()                  the kinds this browser has
//   elWallet.connect()                the account's address ("0x…", or base58 exactly as written)
//   elWallet.sign(text, address)      its personal_sign signature over text ("0x…", 65 bytes)
//   elWallet.signSolana(text, address) a Solana wallet's over the text's bytes ("0x…", 64 bytes)
(() => {
  const said = (err) => (err && (err.message || err.reason)) || String(err);
  const provider = () => {
    const p = window.ethereum;
    if (!p || typeof p.request !== "function") throw new Error("this browser has no wallet extension");
    return p;
  };
  // Phantom's own object, or the one in `window.solana` (Backpack, Solflare…).
  const solana = () => {
    const s = (window.phantom && window.phantom.solana) || window.solana;
    return s && typeof s.signMessage === "function" && typeof s.connect === "function" ? s : null;
  };
  let chosen = "";
  window.elWallet = {
    use(kind) {
      chosen = kind;
    },
    kinds() {
      const out = [];
      if (window.ethereum && typeof window.ethereum.request === "function") out.push("evm");
      if (solana()) out.push("solana");
      return out;
    },
    solanaName() {
      const s = solana();
      return s && s.isPhantom ? "Phantom" : "Solana wallet";
    },
    async connect() {
      const kind = chosen || this.kinds()[0];
      if (kind === "solana") {
        try {
          const got = await solana().connect();
          const a = String((got && got.publicKey) || solana().publicKey || "");
          if (!a) throw new Error("the wallet gave no account");
          return a;
        } catch (err) {
          throw new Error(said(err));
        }
      }
      let accounts;
      try {
        accounts = await provider().request({ method: "eth_requestAccounts" });
      } catch (err) {
        throw new Error(said(err));
      }
      if (!accounts || !accounts[0]) throw new Error("the wallet gave no account");
      return String(accounts[0]);
    },
    async sign(text, address) {
      const hex = "0x" + Array.from(new TextEncoder().encode(text)).map((b) => b.toString(16).padStart(2, "0")).join("");
      let sig;
      try {
        sig = await provider().request({ method: "personal_sign", params: [hex, address] });
      } catch (err) {
        throw new Error(said(err));
      }
      // A smart-contract wallet answers with a longer proof the server can't check against the
      // address: say so, rather than "isn't a signature".
      if (typeof sig !== "string" || sig.replace(/^0x/, "").length !== 130) {
        throw new Error("this wallet signs in a way the game can't check yet: use a regular account");
      }
      return sig;
    },
    async signSolana(text, address) {
      const s = solana();
      if (!s) throw new Error("this browser has no Solana wallet");
      let got;
      try {
        got = await s.signMessage(new TextEncoder().encode(text));
      } catch (err) {
        throw new Error(said(err));
      }
      // Base58 is compared as written: case is part of it.
      if (got && got.publicKey && String(got.publicKey) !== address) {
        throw new Error("the wallet signed with another account than it gave");
      }
      const bytes = got && got.signature ? got.signature : got;
      return "0x" + Array.from(bytes).map((b) => b.toString(16).padStart(2, "0")).join("");
    },
  };
})();
