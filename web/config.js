// Where this page's game server is. Empty, as here (scripts/dev.sh, and a server serving its own
// page): the server that served the page. A build published to another site rewrites this file,
// e.g. for a copy on the store's site:
//   window.EL_SERVER = { wt: "game.example.com:4433", http: "https://game.example.com/eloram/" };
// `wt`: the WebTransport host:port, whose certificate is CA-issued (nothing pinned, unless `pin`);
// `http`: the game server's HTTP side (`status`, `cert-hash`), ending in "/".
// `?server=host:port` and `?http=url` on the page's address override them; `?pin=1` pins the
// certificate the server's `cert-hash` names (a self-signed dev server on another host).
window.EL_SERVER = window.EL_SERVER || {};
