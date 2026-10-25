// Boots the Bevy client: picks the WebGPU or WebGL2 build and hands the launch config to Rust via
// window.EL_CONFIG. The game asks window.elDiscover() where to connect (a restarted dev server has
// a new certificate). `?mode=echo` is the transport smoke test.
const params = new URLSearchParams(location.search);

// Where to connect (window.elServer: config.js and the page's query, boot.js). A named server
// (`wt`) has a CA-issued certificate the browser checks itself: dialled as it is, nothing pinned.
// Otherwise the server's `cert-hash` says its port and the hash to pin (null when its certificate
// is CA-issued), fresh on every dial. Relative, so it works under a path prefix too.
window.elDiscover = async () => {
  const srv = window.elServer || { wt: "", pin: false, url: (p) => p };
  if (srv.wt && !srv.pin) return { wtUrl: `https://${srv.wt}/el`, certHash: null };
  const res = await fetch(srv.url("cert-hash"), { cache: "no-store" });
  if (!res.ok) throw new Error(`the server answered ${res.status}`);
  const info = await res.json();
  const host = srv.wt || `${new URL(srv.url(""), location.href).hostname}:${info.port}`;
  return { wtUrl: `https://${host}${info.path}`, certHash: info.hash };
};
const step = (name, line, fraction) => {
  if (window.elBoot) window.elBoot.step(name, line, fraction);
  else {
    const el = document.getElementById("boot-status");
    if (el) el.textContent = line;
  }
};

// Resolves to `fallback` if `promise` hasn't settled in `ms`.
const within = (promise, ms, fallback) =>
  Promise.race([promise, new Promise((done) => setTimeout(() => done(fallback), ms))]);

async function pickGraphics() {
  const forced = params.get("gfx");
  if (forced === "webgl2" || forced === "webgpu") return forced;
  try {
    // Some drivers never answer the adapter request: past a few seconds, WebGL 2 it is.
    if (navigator.gpu && (await within(navigator.gpu.requestAdapter(), 4000, null))) {
      // Only use WebGPU if that build was shipped.
      const probe = await fetch("./webgpu/el.js", { method: "HEAD" });
      if (probe.ok) return "webgpu";
    }
  } catch (_) {
    /* fall through */
  }
  return "webgl2";
}

// Fades the loading screen out, then hides it for good (called when the first frame is drawn).
window.elHideBoot = () => {
  const boot = document.getElementById("boot");
  if (window.elBoot) window.elBoot.done();
  if (!boot || boot.classList.contains("fading") || boot.classList.contains("failed")) return;
  boot.classList.add("fading");
  setTimeout(() => boot.classList.add("hidden"), 650);
};

async function main() {
  const config = {
    echo: params.get("mode") === "echo",
    name: params.get("name") || "",
  };
  if (!("WebTransport" in window)) {
    config.noWebTransport = true;
  } else {
    step("loader", "finding the server…");
    Object.assign(config, await window.elDiscover());
  }
  window.EL_CONFIG = config;
  step("graphics", "choosing WebGPU or WebGL 2…");
  const gfx = await pickGraphics();
  window.EL_CONFIG.gfx = gfx;
  step("client", `fetching the ${gfx} client…`);
  const mod = await import(`./${gfx}/el.js`);
  const wasm = fetchCounted(new URL(`./${gfx}/el_bg.wasm`, location.href), (got, total) =>
    step("client", `fetching the ${gfx} client · ${(got / 1048576).toFixed(1)} MB`, total ? got / total : null),
  );
  // Bevy's App::run hands control to the browser event loop by throwing a control-flow
  // exception on some targets; that is expected. The loading screen stays up until the client's
  // first frame (`elHideBoot`, from `dev_hooks`).
  try {
    await mod.default({ module_or_path: wasm });
  } catch (e) {
    if (!String(e).includes("Using exceptions for control flow")) throw e;
  }
}

// Fetches the client's wasm, telling `progress` the bytes so far (and the whole, when the server
// says it and the bytes aren't compressed in flight: then `total` is 0).
async function fetchCounted(url, progress) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url.pathname}: the server answered ${res.status}`);
  const compressed = !!res.headers.get("content-encoding");
  const total = compressed ? 0 : Number(res.headers.get("content-length")) || 0;
  const started = () => step("start", "starting the client…");
  if (!res.body) {
    started();
    return res;
  }
  const reader = res.body.getReader();
  let got = 0;
  const body = new ReadableStream({
    async pull(ctrl) {
      const { done, value } = await reader.read();
      if (done) {
        started();
        ctrl.close();
        return;
      }
      got += value.byteLength;
      progress(got, total);
      ctrl.enqueue(value);
    },
    cancel(reason) {
      return reader.cancel(reason);
    },
  });
  return new Response(body, { status: res.status, statusText: res.statusText, headers: res.headers });
}

main().catch((e) => {
  console.error(e);
  if (window.elBoot) window.elBoot.fail(String(e.message || e));
});
