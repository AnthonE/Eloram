// The browser's wallet (EIP-1193, window.ethereum), for signing in: the game builds the text and
// checks nothing here; the server rebuilds the text from its own challenge and checks the
// signature. Never logs a signature.
//   elWallet.connect()            -> the account's address ("0x…")
//   elWallet.sign(text, address)  -> its personal_sign signature over text ("0x…", 65 bytes)
(() => {
  const said = (err) => (err && (err.message || err.reason)) || String(err);
  const provider = () => {
    const p = window.ethereum;
    if (!p || typeof p.request !== "function") throw new Error("this browser has no wallet extension");
    return p;
  };
  window.elWallet = {
    async connect() {
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
  };
})();
