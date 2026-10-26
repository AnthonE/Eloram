// The sound's panel (docs/FX.md, "Sound"): a button in the corner that opens a volume for the
// master and each bus. What it sets is kept in localStorage ("el.audio", the client reads it at
// start) and told to the running client through window.__elInbox ({t:"volume"}, {t:"mute"}).
// A file of its own, no inline handlers: the page runs under `script-src 'self'`.
(function () {
  var KEY = "el.audio";
  var BUSES = [
    ["master", "Master", 0.8],
    ["effects", "Effects", 1.0],
    ["voice", "Announcer", 1.0],
    ["ui", "Interface", 0.8],
    ["ambience", "Ambience", 0.7],
    ["music", "Music", 0.6],
  ];
  function read() {
    try {
      return JSON.parse(localStorage.getItem(KEY) || "{}") || {};
    } catch (_) {
      return {};
    }
  }
  function write(v) {
    try {
      localStorage.setItem(KEY, JSON.stringify(v));
    } catch (_) {}
  }
  function tell(cmd) {
    (window.__elInbox = window.__elInbox || []).push(cmd);
  }
  function build() {
    var v = read();
    var wrap = document.createElement("div");
    wrap.id = "sound";
    var toggle = document.createElement("button");
    toggle.type = "button";
    toggle.id = "sound-toggle";
    toggle.title = "Sound";
    toggle.setAttribute("aria-label", "Sound settings");
    var panel = document.createElement("div");
    panel.id = "sound-panel";
    panel.className = "hidden";
    function icon() {
      toggle.textContent = v.muted ? "🔇" : "🔊";
    }
    BUSES.forEach(function (b) {
      var row = document.createElement("label");
      row.className = "sound-row";
      var name = document.createElement("span");
      name.textContent = b[1];
      var range = document.createElement("input");
      range.type = "range";
      range.min = "0";
      range.max = "100";
      range.value = String(Math.round(100 * (typeof v[b[0]] === "number" ? v[b[0]] : b[2])));
      range.setAttribute("data-bus", b[0]);
      range.addEventListener("input", function () {
        v[b[0]] = Number(range.value) / 100;
        write(v);
        tell({ t: "volume", bus: b[0], v: v[b[0]] });
      });
      row.appendChild(name);
      row.appendChild(range);
      panel.appendChild(row);
    });
    var mute = document.createElement("button");
    mute.type = "button";
    mute.id = "sound-mute";
    function muteText() {
      mute.textContent = v.muted ? "UNMUTE" : "MUTE";
    }
    mute.addEventListener("click", function () {
      v.muted = !v.muted;
      write(v);
      tell({ t: "mute", on: !!v.muted });
      muteText();
      icon();
    });
    panel.appendChild(mute);
    toggle.addEventListener("click", function () {
      panel.classList.toggle("hidden");
    });
    // Keys typed in the panel are the panel's, not the champion's.
    panel.addEventListener("keydown", function (e) {
      e.stopPropagation();
    });
    icon();
    muteText();
    wrap.appendChild(panel);
    wrap.appendChild(toggle);
    document.body.appendChild(wrap);
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", build);
  else build();
})();
