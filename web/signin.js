// How a player signs in, before the game starts (`?signin=1`, with no token to come back on):
// Sign in with Elo Pros (`elo-signin-1.js`: Steam, Discord, Google, GitHub or a wallet on either
// chain), a Solana wallet here, or an Ethereum one. One wallet and no other way: no card, it's used.
//   elSignIn.choose(http)   resolves { signIn, pass } once they've chosen
// The server hands out the nonce a pass carries (`/nonce`); the popup has to open on the click
// itself, so it's fetched ahead. The pass goes with the Hello, and the server checks it offline
// (`el_auth::verify_pass`). A guest gets the link back to here (`#signin-link`).
(() => {
  const $ = (id) => document.getElementById(id);
  const hide = (el, on) => el.classList.toggle("hidden", on);
  const GAME = "eloram";
  window.elSignIn = {
    async nonce(http) {
      try {
        return (await (await fetch(http + "nonce")).json()).nonce || "";
      } catch {
        return "";
      }
    },
    choose(http) {
      return new Promise(async (done) => {
        const wallets = window.elWallet.kinds();
        let nonce = await this.nonce(http);
        if (!nonce && wallets.length === 1) {
          window.elWallet.use(wallets[0]);
          return done({ signIn: true, pass: "" });
        }
        const card = $("signin");
        const note = $("signin-note");
        const list = $("signin-choices");
        const finish = (answer) => {
          hide(card, true);
          done(answer);
        };
        const row = (name, go) => {
          const b = document.createElement("button");
          b.type = "button";
          b.textContent = name;
          b.addEventListener("click", go);
          list.append(b);
        };
        if (nonce) {
          row("SIGN IN WITH ELO PROS", async () => {
            const sdk = window.EloSignIn;
            if (!sdk) {
              note.textContent = "Sign-in with Elo Pros is unavailable right now.";
              return;
            }
            try {
              const res = await sdk.popup({ game: GAME, nonce });
              finish({ signIn: true, pass: res.pass });
            } catch (e) {
              note.textContent = e && e.code === "cancelled" ? "" : (e && e.message) || "That didn't finish.";
              nonce = await this.nonce(http);
            }
          });
        }
        if (wallets.includes("solana")) {
          row(window.elWallet.solanaName().toUpperCase(), () => {
            window.elWallet.use("solana");
            finish({ signIn: true, pass: "" });
          });
        }
        if (wallets.includes("evm")) {
          row("ETHEREUM WALLET", () => {
            window.elWallet.use("evm");
            finish({ signIn: true, pass: "" });
          });
        }
        $("signin-guest").addEventListener("click", () => finish({ signIn: false, pass: "" }), { once: true });
        if (!nonce && !wallets.length) note.textContent = "No wallet here, and Elo Pros isn't taking sign-ins: play as a guest.";
        hide(card, false);
      });
    },
    // The way back to here, for a guest, until a match starts.
    offerLink() {
      const link = $("signin-link");
      const u = new URL(location.href);
      u.searchParams.set("signin", "1");
      link.href = u.pathname + u.search;
      hide(link, false);
      const timer = setInterval(() => {
        const phase = window.__el && window.__el.phase;
        if (phase === "playing" || phase === "selecting") {
          hide(link, true);
          clearInterval(timer);
        }
      }, 1000);
    },
  };
})();
