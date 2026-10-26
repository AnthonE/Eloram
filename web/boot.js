// The boot watchdog: the loading screen names the step it's on, and a client that never starts
// says so (Before Colony's, after Mecha Royale: a load that dies quietly reads as "loading"
// forever). A plain script ahead of everything, so it works when nothing else does: it hears
// the loader's steps (window.elBoot.step), errors anywhere on the page, and a panic's message
// (console.error), and it gives up on a step only once that step has stopped moving: a slow
// download still arriving is never called broken. A file of its own (and no inline handlers on
// the page), so the page runs under a CSP of `script-src 'self'` (the store's).
(function () {
  var STALL_MS = { loader: 20000, graphics: 15000, client: 30000, start: 60000 };
  var state = { step: "loader", detail: "", failed: false, done: false, errors: [], at: Date.now() };
  var timer = 0;
  function el(id) { return document.getElementById(id); }
  function arm() {
    clearTimeout(timer);
    var ms = STALL_MS[state.step];
    if (ms && !state.done && !state.failed) {
      timer = setTimeout(function () {
        fail(state.step === "client" ? "the download stalled" : "nothing happened for " + ms / 1000 + " s",
          "stalled at: " + state.step);
      }, ms);
    }
  }
  function fail(message, detail) {
    if (state.failed || state.done) return;
    // A WebGPU client that fails before its first frame goes back to WebGL 2 (loader.js), once.
    if (window.elGpuFallBack && window.elGpuFallBack(message)) {
      state.failed = true;
      return;
    }
    state.failed = true;
    clearTimeout(timer);
    var boot = el("boot");
    if (boot) boot.classList.add("failed");
    var s = el("boot-status");
    if (s) s.textContent = "failed: " + message;
    var why = el("boot-why");
    if (why) why.textContent = [detail || ""].concat(state.errors.slice(-3)).filter(Boolean).join("\n");
    var box = el("boot-error");
    if (box) box.classList.remove("hidden");
    var gl = el("boot-webgl2");
    if (gl && window.EL_CONFIG && window.EL_CONFIG.gfx === "webgl2") gl.classList.add("hidden");
  }
  window.elBoot = {
    // A step begins (its line shows) or moves on (`fraction` of it done: the meter, re-arming the
    // stall timer).
    step: function (name, line, fraction) {
      if (state.failed || state.done) return;
      state.step = name;
      var s = el("boot-status");
      if (s && line) s.textContent = line;
      var fill = el("boot-fill");
      if (fill) {
        fill.classList.toggle("unknown", fraction == null);
        fill.style.width = fraction == null ? "" : Math.round(Math.min(1, fraction) * 100) + "%";
      }
      arm();
    },
    fail: fail,
    // The first frame is drawn: the watch is over.
    done: function () { state.done = true; clearTimeout(timer); },
    failed: function () { return state.failed; },
    report: function () {
      var c = window.EL_CONFIG || {};
      return ["Eloram boot report", "step: " + state.step, "after: " + (Date.now() - state.at) + " ms",
        "graphics: " + (c.gfx || "?") + " (" + (c.quality || "?") + ")", "renderer: " + (c.renderer || "?"),
        "page: " + location.href, "browser: " + navigator.userAgent, ""].concat(state.errors).join("\n");
    },
  };
  // A panic's message reaches the console before the trap that ends it: keep the last few.
  var error = console.error;
  console.error = function () {
    try {
      state.errors.push(Array.prototype.map.call(arguments, String).join(" ").slice(0, 600));
      if (state.errors.length > 8) state.errors.shift();
    } catch (_) {}
    return error.apply(console, arguments);
  };
  // Errors anywhere while the loading screen is up (resource errors don't bubble: the capture
  // phase hears them). Winit's control-flow exception is how Bevy hands the browser its loop.
  // Only the game's own files count: a script someone else puts on the page (a CDN's analytics
  // beacon, which the store's CSP blocks) failing, or throwing, is no reason to stop.
  function own(url) {
    if (!url) return true;
    try { return new URL(url, location.href).origin === location.origin; } catch (_) { return false; }
  }
  window.addEventListener("error", function (e) {
    var t = e.target;
    // A script of ours that didn't load (the loader's, or the client's): any other element's is no
    // reason to stop.
    var msg = t && t.tagName
      ? (t.tagName === "SCRIPT" && own(t.src) ? "could not load " + t.src : "")
      : (own(e.filename) ? e.message : "");
    if (!msg || /exceptions for control flow/.test(msg)) return;
    fail(msg);
  }, true);
  window.addEventListener("unhandledrejection", function (e) {
    var msg = String(e.reason && e.reason.message || e.reason);
    if (!/exceptions for control flow/.test(msg)) fail(msg);
  });
  arm();
})();

// The loading screen's buttons (wired here rather than inline, for the CSP).
document.addEventListener("DOMContentLoaded", function () {
  function on(id, fn) {
    var b = document.getElementById(id);
    if (b) b.addEventListener("click", fn);
  }
  on("boot-reload", function () { location.reload(); });
  on("boot-webgl2", function () {
    var u = new URL(location.href);
    u.searchParams.set("gfx", "webgl2");
    location.href = u.href;
  });
  on("boot-copy", function () {
    var b = this;
    if (navigator.clipboard) {
      navigator.clipboard.writeText(window.elBoot.report()).then(function () { b.textContent = "COPIED"; }, function () {});
    }
  });
});

// Where the game server is (window.elServer), for the loader's dial and the page's `status`:
// config.js's EL_SERVER, overridden by `?server=host:port` / `?http=url` / `?pin=1`. With no `wt`
// the page dials the server that served it, asking its `cert-hash` for the port (and the hash to
// pin: null when its certificate is CA-issued).
(function () {
  var params = new URLSearchParams(location.search);
  var baked = window.EL_SERVER || {};
  var wt = params.get("server") || baked.wt || "";
  var http = params.get("http") || baked.http || "";
  if (http && !/\/$/.test(http)) http += "/";
  window.elServer = {
    wt: wt,
    http: http,
    pin: params.has("pin") ? params.get("pin") !== "0" : !!baked.pin,
    // A path on the game server's HTTP side (relative to the page when it's the page's own).
    url: function (path) { return http + path; },
  };
})();
