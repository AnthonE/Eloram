#!/usr/bin/env python3
"""The game's sounds: a file for every take of every line of web/assets/audio/manifest.json that
isn't optional (the table is el_client_core::fx::sound). The physical body of a sound (thuds,
glass, metal, wood, cloth, footsteps, coins, cards) comes from Kenney's CC0 audio packs; the
elemental and magical layers (fire, water, wind, bells, glass partials, starlight, the warnings)
are synthesised here. Each sound is layered, pitched, filtered and enveloped, put in one shared
small room (effects; the interface stays dry), trimmed to its onset and levelled by bus.

    python3 -I art/tools/sounds.py [--cache dir] [--sheets dir] [--only prefix]

Writes web/assets/audio/<stem>.ogg (Opus in Ogg, 48 kHz, 64 kb/s mono or 96 kb/s stereo) and
<stem>.mp3 (LAME VBR -q:a 5), then decodes every file back and prints a report (duration, true
peak, RMS of the part above -40 dBFS, onset) and exits non-zero if a file breaks a rule: missing,
the wrong channels or rate, a peak over -1 dBFS, more than half again its stated length, an onset
past 5 ms, a level off its bus's target, or either format's whole set past 3 MiB. With --sheets,
a spectrogram contact sheet per family and one for the rest go in that directory (not shipped).

The packs are downloaded once into the cache (default art/cache/sounds/, not in git), checked
against their hashes and their licence files (CC0), and each extracted into its own directory;
nothing in them is run. Every random choice comes from a generator seeded by the take's name, so
a run makes the same files every time. Needs numpy, Pillow (for --sheets) and an ffmpeg with
libopus and libmp3lame.
"""
import argparse
import hashlib
import json
import math
import os
import re
import shutil
import subprocess
import sys
import urllib.request
import zipfile
import zlib
from concurrent.futures import ThreadPoolExecutor

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
AUDIO = os.path.join(ROOT, "web", "assets", "audio")
SR = 48000

# ---------------------------------------------------------------------------------------------
# Sources: Kenney's packs, pinned by URL and hash.

KENNEY = "https://kenney.nl/media/pages/assets"
PACKS = {
    "impact": ("impact-sounds", "87b4ddecda-1677589768",
               "029d734af1582474edf3a694d1b0cebc97c1c152f2f39fa34d4c2bafc5de77f8"),
    "rpg": ("rpg-audio", "8e99002d76-1677590336",
            "6dbeaf8544da958d8f2adcb4a4a4b76c1ade34a05f8ab9edccd327da7375f38b"),
    "scifi": ("sci-fi-sounds", "6b296f9ecf-1677589334",
              "119340f351a5098ad814f78719438c0da355a9ce8a4c8a3af6a8d48aa3d49e04"),
    "casino": ("casino-audio", "2472606a04-1721639069",
               "f36250766ac5bc378c13708ddf12a23a8e54a3251f8d482c7536e51b5dbafa18"),
    "interface": ("interface-sounds", "fa43c1dd4d-1677589452",
                  "f2193d072726d6758a5f7871b2dcc54dcce0d5c35c6f0a62f92549b327c81232"),
}


def _n(pat, a, b):
    return [pat.format(i) for i in range(a, b)]


# A name for each set of recordings: (pack, file stems under Audio/).
BANK = {
    "punch_h": ("impact", _n("impactPunch_heavy_{:03d}", 0, 5)),
    "punch_m": ("impact", _n("impactPunch_medium_{:03d}", 0, 5)),
    "soft_h": ("impact", _n("impactSoft_heavy_{:03d}", 0, 5)),
    "soft_m": ("impact", _n("impactSoft_medium_{:03d}", 0, 5)),
    "generic_l": ("impact", _n("impactGeneric_light_{:03d}", 0, 5)),
    "glass_h": ("impact", _n("impactGlass_heavy_{:03d}", 0, 5)),
    "glass_m": ("impact", _n("impactGlass_medium_{:03d}", 0, 5)),
    "glass_l": ("impact", _n("impactGlass_light_{:03d}", 0, 5)),
    "metal_h": ("impact", _n("impactMetal_heavy_{:03d}", 0, 5)),
    "metal_m": ("impact", _n("impactMetal_medium_{:03d}", 0, 5)),
    "metal_l": ("impact", _n("impactMetal_light_{:03d}", 0, 5)),
    "bell_h": ("impact", _n("impactBell_heavy_{:03d}", 0, 5)),
    "plate_h": ("impact", _n("impactPlate_heavy_{:03d}", 0, 5)),
    "plate_m": ("impact", _n("impactPlate_medium_{:03d}", 0, 5)),
    "tin_m": ("impact", _n("impactTin_medium_{:03d}", 0, 5)),
    "wood_h": ("impact", _n("impactWood_heavy_{:03d}", 0, 5)),
    "wood_m": ("impact", _n("impactWood_medium_{:03d}", 0, 5)),
    "wood_l": ("impact", _n("impactWood_light_{:03d}", 0, 5)),
    "plank_m": ("impact", _n("impactPlank_medium_{:03d}", 0, 5)),
    "mining": ("impact", _n("impactMining_{:03d}", 0, 5)),
    "step_snow": ("impact", _n("footstep_snow_{:03d}", 0, 5)),
    "step_concrete": ("impact", _n("footstep_concrete_{:03d}", 0, 5)),
    "step_grass": ("impact", _n("footstep_grass_{:03d}", 0, 5)),
    "step_wood": ("impact", _n("footstep_wood_{:03d}", 0, 5)),
    "creak": ("rpg", _n("creak{}", 1, 4)),
    "knife_draw": ("rpg", _n("drawKnife{}", 1, 4)),
    "knife_slice": ("rpg", ["knifeSlice", "knifeSlice2"]),
    "chop": ("rpg", ["chop"]),
    "cloth": ("rpg", _n("cloth{}", 1, 5)),
    "metal_click": ("rpg", ["metalClick", "metalLatch"]),
    "coins": ("rpg", ["handleCoins", "handleCoins2"]),
    "step_rpg": ("rpg", _n("footstep{:02d}", 0, 10)),
    "crunch": ("scifi", _n("explosionCrunch_{:03d}", 0, 5)),
    "lowboom": ("scifi", _n("lowFrequency_explosion_{:03d}", 0, 2)),
    "forcefield": ("scifi", _n("forceField_{:03d}", 0, 5)),
    "thruster": ("scifi", _n("thrusterFire_{:03d}", 0, 5)),
    "card_slide": ("casino", _n("card-slide-{}", 1, 9)),
    "card_place": ("casino", _n("card-place-{}", 1, 5)),
    "card_shuffle": ("casino", ["card-shuffle"]),
    "card_fan": ("casino", _n("card-fan-{}", 1, 3)),
    "chips": ("casino", _n("chips-collide-{}", 1, 5)),
    "chip_lay": ("casino", _n("chip-lay-{}", 1, 4)),
    "ui_click": ("interface", _n("click_{:03d}", 1, 6)),
    "ui_tick": ("interface", ["tick_001", "tick_002", "tick_004"]),
    "ui_switch": ("interface", _n("switch_{:03d}", 1, 8)),
}


class Sources:
    """The packs, fetched and checked once, and their recordings decoded on demand."""

    def __init__(self, cache):
        self.cache = cache
        self.dirs = {}
        self.decoded = {}

    def pack(self, key):
        if key in self.dirs:
            return self.dirs[key]
        slug, ver, sha = PACKS[key]
        os.makedirs(self.cache, exist_ok=True)
        z = os.path.join(self.cache, f"kenney_{slug}.zip")
        if not os.path.isfile(z):
            url = f"{KENNEY}/{slug}/{ver}/kenney_{slug}.zip"
            print("fetching", url, flush=True)
            data = urllib.request.urlopen(url, timeout=180).read()
            if hashlib.sha256(data).hexdigest() != sha:
                sys.exit(f"{url}: not the pack we pinned (sha256 differs)")
            with open(z + ".part", "wb") as f:
                f.write(data)
            os.replace(z + ".part", z)
        else:
            with open(z, "rb") as f:
                if hashlib.sha256(f.read()).hexdigest() != sha:
                    sys.exit(f"{z}: not the pack we pinned; delete it to fetch again")
        d = os.path.join(self.cache, slug)
        if not os.path.isdir(d):
            # A fresh directory of its own, renamed into place once whole.
            tmp = d + ".extracting"
            shutil.rmtree(tmp, ignore_errors=True)
            os.makedirs(tmp)
            with zipfile.ZipFile(z) as zf:
                zf.extractall(tmp)
            lic = open(os.path.join(tmp, "License.txt"), encoding="utf-8", errors="replace").read()
            if "CC0" not in lic or "publicdomain/zero" not in lic:
                sys.exit(f"{slug}: its licence file doesn't say CC0")
            os.replace(tmp, d)
        self.dirs[key] = d
        return d

    def get(self, name, i):
        """Recording `i` (wrapping) of set `name`: mono, 48 kHz, its onset at 0, peak 1."""
        pack, stems = BANK[name]
        stem = stems[i % len(stems)]
        key = (pack, stem)
        if key not in self.decoded:
            d = self.pack(pack)
            path = os.path.realpath(os.path.join(d, "Audio", stem + ".ogg"))
            if not path.startswith(os.path.realpath(d) + os.sep) or not os.path.isfile(path):
                sys.exit(f"{pack}: no {stem}.ogg")
            x = decode(path, 1)[:, 0]
            x = ffilt(x, lambda f: hp(f, 30, 2))
            x = x[max(0, first_above(x, 0.02) - T(0.0005)):]
            self.decoded[key] = x / max(1e-9, np.max(np.abs(x)))
        return self.decoded[key].copy()

    def count(self, name):
        return len(BANK[name][1])


SRC = None  # the Sources, set in main (forked workers share it)


def smp(name, i, rate=1.0):
    """A recording, at `rate` (pitch and speed together)."""
    x = SRC.get(name, i)
    return speed(x, rate) if rate != 1.0 else x


def decode(path, channels):
    out = subprocess.run(
        ["ffmpeg", "-v", "error", "-nostdin", "-i", path, "-ac", str(channels), "-ar", str(SR),
         "-f", "f32le", "-"],
        check=True, stdout=subprocess.PIPE).stdout
    return np.frombuffer(out, dtype="<f4").astype(np.float64).reshape(-1, channels)


# ---------------------------------------------------------------------------------------------
# The toolkit.


def T(s):
    return max(1, int(round(s * SR)))


def tt(n):
    return np.arange(n) / SR


def first_above(x, rel):
    a = np.abs(x) if x.ndim == 1 else np.max(np.abs(x), axis=1)
    k = np.nonzero(a >= rel * max(1e-12, a.max()))[0]
    return int(k[0]) if len(k) else 0


def endfade(x, s=0.01):
    """The last `s` seconds faded out, so nothing stops dead."""
    m = min(len(x), T(s))
    x = np.array(x, dtype=np.float64)
    x[len(x) - m:] *= np.linspace(1, 0, m)
    return x


def fit(x, n):
    """Cut (with a fade) or padded to `n` samples."""
    return endfade(x[:n]) if len(x) > n else np.concatenate([x, np.zeros(n - len(x))])


def rms(x):
    return math.sqrt(float(np.mean(x * x))) if len(x) else 0.0


def unit(x):
    return x / max(1e-12, np.max(np.abs(x)))


def db(g):
    return 10 ** (g / 20)


# Filter shapes, as magnitudes over frequency (they take arrays, so a centre can move).
def lp(f, fc, o=2):
    return 1 / np.sqrt(1 + (f / fc) ** (2 * o))


def hp(f, fc, o=2):
    return 1 / np.sqrt(1 + (fc / np.maximum(f, 1e-3)) ** (2 * o))


def bp(f, fc, q=1.0):
    r = np.maximum(f, 1e-3) / fc
    return 1 / np.sqrt(1 + q * q * (r - 1 / r) ** 2)


def peak(f, fc, q, g):
    """A bell boost of `g` (linear, over 1) at `fc`."""
    return 1 + (g - 1) * bp(f, fc, q) ** 2


def ffilt(x, shape, pad=0.25):
    """A static zero-phase filter, by its magnitude."""
    n = len(x)
    m = 1 << int(math.ceil(math.log2(n + T(pad) + 1)))
    f = np.fft.rfftfreq(m, 1 / SR)
    return np.fft.irfft(np.fft.rfft(x, m) * shape(f), m)[:n]


def tvf(x, shape, N=1024):
    """A filter that moves: `shape(t, f)` (t, the frame's time in seconds, as a column) applied
    frame by frame, short-time Fourier, overlap-added."""
    H = N // 4
    n = len(x)
    xp = np.concatenate([np.zeros(N), x, np.zeros(N + H)])
    nfr = 1 + (len(xp) - N) // H
    w = np.sqrt(0.5 - 0.5 * np.cos(2 * np.pi * np.arange(N) / N))
    idx = np.arange(N)[None, :] + H * np.arange(nfr)[:, None]
    F = np.fft.rfft(xp[idx] * w, axis=1)
    tc = ((H * np.arange(nfr) + N / 2 - N) / SR)[:, None]
    f = np.fft.rfftfreq(N, 1 / SR)[None, :]
    y = np.fft.irfft(F * shape(np.clip(tc, 0, n / SR), f), N, axis=1) * w
    blocks = np.zeros((nfr + 3, H))
    for j in range(4):
        blocks[j:j + nfr] += y[:, j * H:(j + 1) * H]
    return blocks.ravel()[N:N + n] / 2.0


def curve(n, pts, kind="lin"):
    """A control curve through (time in s, value) points; 'exp' interpolates in the log."""
    ts = np.array([p[0] for p in pts]) * SR
    vs = np.array([p[1] for p in pts], dtype=np.float64)
    i = np.arange(n)
    if kind == "exp":
        return np.exp(np.interp(i, ts, np.log(np.maximum(vs, 1e-9))))
    return np.interp(i, ts, vs)


def shaped(n, rng, color):
    """Noise of a spectral tilt: 0 white, 1 pink, 2 brown (lowest bins held under 25 Hz)."""
    m = 1 << int(math.ceil(math.log2(n + 1)))
    f = np.fft.rfftfreq(m, 1 / SR)
    spec = (rng.standard_normal(len(f)) + 1j * rng.standard_normal(len(f)))
    spec *= 1 / np.maximum(f, 25.0) ** (color / 2)
    spec[0] = 0
    x = np.fft.irfft(spec, m)[:n]
    return x / max(1e-12, rms(x))


def white(n, rng):
    return rng.standard_normal(n)


def pink(n, rng):
    return shaped(n, rng, 1)


def brown(n, rng):
    return shaped(n, rng, 2)


def phase(freq, n=None):
    f = np.full(n, float(freq)) if np.isscalar(freq) else np.asarray(freq, dtype=np.float64)
    return 2 * np.pi * np.cumsum(f) / SR


def sine(freq, n=None, ph=0.0):
    return np.sin(phase(freq, n) + ph)


def saw(freq, n=None, top=14000, rolloff=0.0):
    """A band-limited saw (additive), its harmonics stopping under `top`."""
    f = np.full(n, float(freq)) if np.isscalar(freq) else np.asarray(freq, dtype=np.float64)
    p = phase(f)
    out = np.zeros(len(f))
    fmax = f.max()
    for h in range(1, int(top / max(fmax, 1)) + 1):
        out += np.sin(h * p) / h ** (1 + rolloff)
    return out


def partials(n, f0, ratios, amps, taus, rng, attack=0.001, detune=0.0, beat=0.0):
    """Additive inharmonic partials, each ringing down in its own time: bells, glass, chimes.
    `beat` adds a slightly mistuned twin to each partial (the warble of a real bell)."""
    t = tt(n)
    out = np.zeros(n)
    for r, a, tau in zip(ratios, amps, taus):
        f = f0 * r * (1 + detune * rng.uniform(-1, 1))
        if f >= 15000:
            continue
        e = a * np.exp(-t / tau)
        out += e * np.sin(2 * np.pi * f * t + rng.uniform(0, 2 * np.pi))
        if beat:
            out += 0.5 * e * np.sin(2 * np.pi * f * (1 + beat * rng.uniform(0.5, 1.5)) * t
                                    + rng.uniform(0, 2 * np.pi))
    a = T(attack)
    out[:a] *= np.linspace(0, 1, a)
    return endfade(out, 0.015)


BELL = ([0.5, 1.0, 1.19, 1.5, 2.0, 2.51, 2.66, 3.01, 4.16, 5.43],
        [0.55, 0.9, 0.6, 0.35, 1.0, 0.4, 0.3, 0.25, 0.18, 0.1],
        [1.0, 0.7, 0.55, 0.45, 0.5, 0.3, 0.28, 0.22, 0.15, 0.1])
GLASS = ([1.0, 2.32, 4.25, 6.63, 9.38], [1.0, 0.55, 0.35, 0.2, 0.1], [1.0, 0.6, 0.35, 0.22, 0.14])
CHIME = ([1.0, 2.76, 5.40, 8.93], [1.0, 0.45, 0.25, 0.12], [1.0, 0.5, 0.28, 0.16])
BAR = ([1.0, 3.93, 9.61], [1.0, 0.35, 0.12], [1.0, 0.4, 0.15])


def bell(n, f0, rng, length=1.0, kind=BELL, bright=1.0, beat=0.0015, detune=0.004):
    r, a, tau = kind
    a = [x * bright ** k for k, x in enumerate(a)]
    return partials(n, f0, r, a, [x * length for x in tau], rng, detune=detune, beat=beat)


def fm(n, fc, ratio, index, rng=None):
    """Two-operator FM: `index` a scalar or a curve."""
    t = tt(n)
    idx = index if np.isscalar(index) else np.asarray(index)
    fcv = fc if np.isscalar(fc) else np.asarray(fc)
    pm = phase(fcv * ratio, n) if not np.isscalar(fc) else 2 * np.pi * fc * ratio * t
    pc = phase(fcv, n) if not np.isscalar(fc) else 2 * np.pi * fc * t
    return np.sin(pc + idx * np.sin(pm))


def adsr(n, a, d, s, r, curve_pow=1.0):
    """Attack, decay to sustain, sustain, release at the end (times in seconds)."""
    t = tt(n)
    dur = n / SR
    e = np.where(t < a, t / max(a, 1e-6), s + (1 - s) * np.exp(-(t - a) / max(d, 1e-6)))
    rel = np.clip((dur - t) / max(r, 1e-6), 0, 1)
    return (e * rel) ** curve_pow


def swell(n, rise, start=0.1, fall=0.05, power=2.0):
    """A shaped swell: from `start` up to 1 over `rise` seconds, then down over `fall`."""
    t = tt(n)
    up = start + (1 - start) * np.clip(t / rise, 0, 1) ** power
    down = np.clip((n / SR - t) / max(fall, 1e-6), 0, 1)
    return up * down


def decay(n, tau, delay=0.0):
    t = tt(n)
    return np.where(t < delay, 1.0, np.exp(-(t - delay) / tau))


def speed(x, r):
    """Played at `r` times the speed (pitch and time together), band-limited."""
    if abs(r - 1) < 1e-6:
        return x.copy()
    n = len(x)
    pad = 1 << int(math.ceil(math.log2(n + 64)))
    X = np.fft.rfft(x, pad)
    m = int(round(pad / r))
    k = m // 2 + 1
    Y = np.zeros(k, dtype=complex)
    c = min(k, len(X))
    Y[:c] = X[:c]
    y = np.fft.irfft(Y, m) * (m / pad)
    return y[:int(round(n / r))]


def varispeed(x, rate):
    """Played at a rate that moves (a curve, over the output), resampled linearly."""
    pos = np.cumsum(rate)
    pos = pos[pos < len(x) - 1]
    return np.interp(pos, np.arange(len(x)), x)


def delay_read(x, d):
    """x(t - d(t)), d in samples (a curve), interpolated."""
    i = np.arange(len(x)) - d
    return np.interp(i, np.arange(len(x)), x, left=0.0, right=0.0)


def comb(x, freq, g=0.85, K=10):
    """A resonator at `freq` (a curve or a scalar): the sum of echoes a period apart, decaying,
    which pitches noise (the warnings ride on this)."""
    n = len(x)
    f = np.full(n, float(freq)) if np.isscalar(freq) else np.asarray(freq)
    D = SR / np.maximum(f, 20)
    out = x.copy()
    for k in range(1, K + 1):
        out += g ** k * delay_read(x, k * D)
    return out * (1 - g)


def flange(x, rate, lo_ms, hi_ms, mix=0.7, ph=0.0):
    t = tt(len(x))
    d = (lo_ms + (hi_ms - lo_ms) * (0.5 - 0.5 * np.cos(2 * np.pi * rate * t + ph))) * SR / 1000
    return (x + mix * delay_read(x, d)) / (1 + mix)


def sat(x, drive):
    return np.tanh(x * drive) / math.tanh(drive)


def place(out, x, at, g=1.0):
    i = T(at) if at > 0 else 0
    if i >= len(out):
        return out
    m = min(len(x), len(out) - i)
    out[i:i + m] += g * (endfade(x[:m]) if m < len(x) else x[:m])
    return out


def tail(x, s):
    """Fade the last `s` seconds out."""
    m = min(len(x), T(s))
    x = x.copy()
    x[len(x) - m:] *= np.linspace(1, 0, m) ** 1.5
    return x


def thump(n, f0, f1, tp, ta, click=0.0, rng=None):
    """A drum-like body: a sine falling from f0 to f1 (time constant tp) ringing down over ta."""
    t = tt(n)
    f = f1 + (f0 - f1) * np.exp(-t / tp)
    x = np.sin(phase(f)) * np.exp(-t / ta)
    if click and rng is not None:
        c = ffilt(white(T(0.004), rng), lambda q: bp(q, 3000, 0.7)) * np.exp(-tt(T(0.004)) / 0.0008)
        x[:len(c)] += click * c / max(1e-9, np.abs(c).max())
    a = T(0.0015)
    x[:a] *= np.linspace(0, 1, a)
    return x


def impulses(n, rng, rate, amp=None, mode="pareto", a=2.2):
    """Random clicks: a Poisson train with `rate` per second (a curve or a scalar)."""
    r = np.full(n, float(rate)) if np.isscalar(rate) else np.asarray(rate)
    hit = rng.random(n) < r / SR
    k = np.nonzero(hit)[0]
    v = np.zeros(n)
    if mode == "pareto":
        mags = np.minimum(rng.pareto(a, len(k)) + 0.2, 6.0)
    else:
        mags = rng.uniform(0.3, 1.0, len(k))
    v[k] = mags * rng.choice([-1, 1], len(k))
    if amp is not None:
        v *= amp
    return v


def crackle(n, rng, rate, lo=900, hi=7000, amp=None):
    """Fire's crackle: pops of a few kinds (short noise bursts, a couple of milliseconds, each
    kind its own colour), at random, most small and a few loud."""
    out = np.zeros(n)
    for kind in range(4):
        v = impulses(n, rng, (rate if np.isscalar(rate) else np.asarray(rate)) / 4, amp)
        L = T(rng.uniform(0.0015, 0.005))
        ker = white(L, rng) * np.exp(-tt(L) / rng.uniform(0.0004, 0.0015))
        fc = math.exp(rng.uniform(math.log(lo * 1.5), math.log(hi * 0.7)))
        ker = ffilt(ker, lambda f: bp(f, fc, 0.8), pad=0.01)
        out += np.convolve(v, ker)[:n]
    return ffilt(out, lambda f: hp(f, lo, 2) * lp(f, hi, 2))


def bubbles(n, rng, rate, flo=300, fhi=1800, amp=None, rise=1.0):
    """Water: bubbles (each a sine whose pitch rises as it rings down, as a bubble's does)."""
    r = np.full(n, float(rate)) if np.isscalar(rate) else np.asarray(rate)
    k = np.nonzero(rng.random(n) < r / SR)[0]
    out = np.zeros(n)
    for i in k:
        f0 = math.exp(rng.uniform(math.log(flo), math.log(fhi)))
        tau = rng.uniform(0.004, 0.012) * (600 / f0) ** 0.5 * 1.5
        L = min(n - i, T(tau * 5))
        t = tt(L)
        f = f0 * (1 + rise * rng.uniform(0.1, 0.25) * t / tau)
        g = rng.uniform(0.2, 1.0) * (amp[i] if amp is not None else 1.0)
        e = np.exp(-t / tau)
        e[:T(0.0006)] *= np.linspace(0, 1, T(0.0006))
        out[i:i + L] += g * e * np.sin(phase(f))
    return endfade(out)


def grains(n, rng, rate, flo, fhi, dlo=0.02, dhi=0.08, amp=None, fcurve=None):
    """Starlight and sparkle: short sine grains at random pitches, each Hann-windowed. With
    `fcurve` (a multiplier over time) the pitches drift."""
    r = np.full(n, float(rate)) if np.isscalar(rate) else np.asarray(rate)
    k = np.nonzero(rng.random(n) < r / SR)[0]
    out = np.zeros(n)
    for i in k:
        L = min(n - i, T(rng.uniform(dlo, dhi)))
        if L < 8:
            continue
        f = math.exp(rng.uniform(math.log(flo), math.log(fhi)))
        if fcurve is not None:
            f *= fcurve[i]
        if f > 14500:
            continue
        w = np.sin(np.pi * np.arange(L) / L) ** 2
        g = rng.uniform(0.3, 1.0) * (amp[i] if amp is not None else 1.0)
        out[i:i + L] += g * w * np.sin(2 * np.pi * f * tt(L) + rng.uniform(0, 6.3))
    return endfade(out)


def ks(n, freq, rng, decay_s=0.6, bright=0.5):
    """A plucked string (Karplus-Strong), a period of the loop at a time."""
    P = max(2, int(round(SR / freq)))
    buf = np.zeros(n + 2 * P + 2)
    burst = white(P, rng)
    buf[1:P + 1] = burst - burst.mean()
    g = math.exp(-P / (decay_s * SR) * 6.9)
    for s in range(P + 1, n + 1, P):
        a = buf[s - P:s]
        b1 = buf[s - P - 1:s - 1]
        buf[s:s + P] = g * (bright * a + (1 - bright) * 0.5 * (a + b1))
    return buf[1:n + 1]


FORMANTS = {"a": [(730, 1.0, 8), (1090, 0.5, 9), (2440, 0.25, 12)],
            "o": [(570, 1.0, 8), (840, 0.45, 9), (2410, 0.15, 12)],
            "u": [(440, 1.0, 7), (1020, 0.3, 9), (2240, 0.1, 12)],
            "e": [(530, 1.0, 8), (1840, 0.45, 10), (2480, 0.25, 12)]}


def vowel(x, v):
    fs = FORMANTS[v]
    return ffilt(x, lambda f: sum(a * bp(f, fc, q) for fc, a, q in fs) + 0.02)


def chord_pad(n, freqs, rng, detune=0.006, voices=3, top=9000, rolloff=0.3):
    """Detuned saws, a few voices to a note: a warm bed (filter it after)."""
    out = np.zeros(n)
    for f in freqs:
        for v in range(voices):
            d = 1 + detune * (v - (voices - 1) / 2) + rng.uniform(-0.001, 0.001)
            out += saw(f * d, n, top=top, rolloff=rolloff) * (1 / voices)
    return out


def whoosh(n, rng, f0, f1, q=1.2, shape=None, color=1, mid=None):
    """Air moving past: pink noise through a band whose centre sweeps f0 -> (mid) -> f1."""
    x = shaped(n, rng, color)
    dur = n / SR
    if mid is None:
        fc = lambda t: f0 * (f1 / f0) ** (t / dur)
    else:
        fc = lambda t: np.where(t < dur / 2, f0 * (mid / f0) ** (2 * t / dur),
                                mid * (f1 / mid) ** (2 * t / dur - 1))
    y = tvf(x, lambda t, f: bp(f, fc(t), q))
    return y * (shape if shape is not None else 1.0)


def bump(n, at, width, power=2.0):
    """A smooth hump peaking at `at` seconds (a whoosh's pass)."""
    t = tt(n)
    return np.exp(-np.abs((t - at) / width) ** power)


def norm(x, to=1.0):
    return to * x / max(1e-12, np.max(np.abs(x)))


# ---------------------------------------------------------------------------------------------
# The shared room: a short synthetic space, the same for every effect.


def make_room(rng, n=0.55, rt=0.42, stereo=False):
    L = T(n)
    t = tt(L)
    chans = []
    for c in range(2 if stereo else 1):
        lo = ffilt(white(L, rng), lambda f: lp(f, 1500, 1)) * np.exp(-t * 6.9 / rt)
        hi = ffilt(white(L, rng), lambda f: hp(f, 1500, 1) * lp(f, 9000, 2)) * np.exp(-t * 6.9 / (rt * 0.55))
        ir = lo + 0.7 * hi
        ir[:T(0.004)] = 0  # pre-delay
        # A few early reflections off the stone.
        for d, g in ((0.009, 0.5), (0.014, -0.35), (0.021, 0.3), (0.033, 0.2)):
            ir[T(d + 0.002 * c)] += g * 4
        ir[:T(0.006)] *= np.linspace(0, 1, T(0.006))
        chans.append(ir / math.sqrt(np.sum(ir * ir)))
    return chans


ROOM = None


def room(x, wet):
    if wet <= 0:
        return x
    ir = ROOM[0]
    m = 1 << int(math.ceil(math.log2(len(x) + len(ir))))
    y = np.fft.irfft(np.fft.rfft(x, m) * np.fft.rfft(ir, m), m)[:len(x) + len(ir)]
    out = np.concatenate([x, np.zeros(len(ir))])
    return out + wet * y


# ---------------------------------------------------------------------------------------------
# The designs: a function a sound, each making one take (mono, or stereo for the wind) from its
# Take: its generator `v.rng`, its index `v.k`, `v.pick(set)` for its own recording.

DESIGNS = {}
FAMILIES = ["ember", "frost", "tide", "wind", "dusk", "growth", "stone", "bronze", "steel"]


def sound(name):
    def reg(fn):
        DESIGNS[name] = fn
        return fn
    return reg


def mf(m):
    return 440.0 * 2 ** ((m - 69) / 12)


def wob(n, rng, rate):
    """A smooth random wander in [-1, 1], about `rate` Hz."""
    x = ffilt(white(n, rng), lambda f: lp(f, rate, 3), pad=1.0)
    return x / max(1e-12, np.max(np.abs(x)))


def seg(x, start, n):
    return fit(x[T(start) if start > 0 else 0:], n)


def at(n, x, when, g=1.0):
    """`x` in a silence of `n` samples, starting at `when` seconds."""
    return place(np.zeros(n), x, when, g)


def bandcurve(n, fc):
    """A per-sample curve as a function of frame time, for tvf shapes."""
    ts = tt(n)
    return lambda tc: np.interp(tc, ts, fc)


def drone(n, fcurve, ratios, amps, rng, beat=0.002):
    """Sustained partials over a moving fundamental (glassy, bell-like or metallic drones)."""
    out = np.zeros(n)
    for r, a in zip(ratios, amps):
        fr = fcurve * r
        if fr.max() >= 14500:
            continue
        out += a * np.sin(phase(fr) + rng.uniform(0, 6.3))
        if beat:
            out += 0.6 * a * np.sin(phase(fr * (1 + beat * rng.uniform(0.5, 1.5))) + rng.uniform(0, 6.3))
    return out


def glints(n, rng, rate, flo, fhi, tau=(0.04, 0.15), amp=None, kind=GLASS, fcurve=None):
    """Glass and ice: little struck partials at random, each ringing down."""
    r = np.full(n, float(rate)) if np.isscalar(rate) else np.asarray(rate)
    idx = np.nonzero(rng.random(n) < r / SR)[0]
    ratios, amps, taus = kind
    out = np.zeros(n)
    for i in idx:
        f0 = math.exp(rng.uniform(math.log(flo), math.log(fhi)))
        if fcurve is not None:
            f0 *= fcurve[i]
        tau0 = rng.uniform(*tau)
        L = min(n - i, T(tau0 * 4))
        if L < 16:
            continue
        x = partials(L, f0, ratios, amps, [tau0 * q for q in taus], rng, attack=0.0004)
        out[i:i + L] += rng.uniform(0.25, 1.0) * x * (amp[i] if amp is not None else 1.0)
    return endfade(out)


def trem(n, rate, depth, ph=0.0):
    """Tremolo: `rate` a scalar or a curve."""
    p = phase(rate, n)
    return 1 - depth * (0.5 + 0.5 * np.sin(p + ph))


# --- ember: the forge. Roar (saturated brown noise with a flicker), crackle, hiss.

def roar(n, rng, lo=110, hi=1400, drive=2.0, flick=7.0, depth=0.45):
    x = 0.8 * brown(n, rng) + 0.45 * pink(n, rng)
    x = ffilt(x, lambda f: hp(f, lo, 2) * lp(f, hi, 2))
    x = unit(x) * (1 + depth * wob(n, rng, flick))
    return unit(sat(x, drive))


def hiss(n, rng, lo=3000, hi=9000):
    return unit(ffilt(white(n, rng), lambda f: hp(f, lo, 2) * lp(f, hi, 2)))


@sound("ember.cast")
def _(v):
    r, n = v.rng, T(0.36)
    dur = n / SR
    e = swell(n, 0.30, start=0.2, fall=0.05, power=1.6)
    w = unit(whoosh(n, r, 300 * v.j(0.1), 2600 * v.j(0.1), q=0.9, color=1.5))
    f = roar(n, r, 120, 1800, drive=2.2)
    f = unit(tvf(f, lambda tc, fr: lp(fr, 500 * 8 ** (tc / dur), 2)))
    c = unit(crackle(n, r, curve(n, [(0, 60), (dur, 420)]), 1200, 8000))
    h = hiss(n, r) * swell(n, 0.3, 0.05, 0.05, 2.5)
    return e * (0.75 * w + 0.8 * f) + 0.35 * c * np.sqrt(e) + 0.12 * h


@sound("ember.launch")
def _(v):
    r, n = v.rng, T(0.42)
    e = adsr(n, 0.008, 0.12, 0.0, 0.05)
    w = unit(whoosh(n, r, 2400 * v.j(0.1), 450, q=1.1, color=1.2)) * np.maximum(e, 0.8 * bump(n, 0.06, 0.12, 1.5))
    f = roar(n, r, 90, 1600, drive=2.8) * decay(n, 0.11)
    th = smp("thruster", v.pick("thruster"), 1.25 * v.j(0.05))
    th = unit(ffilt(seg(th, 0.3 + 0.4 * v.k, n), lambda q: hp(q, 300, 2))) * decay(n, 0.1)
    pop = thump(n, 180, 70, 0.02, 0.05, click=0.3, rng=r)
    c = unit(crackle(n, r, curve(n, [(0, 300), (0.3, 30)]), 1500, 9000)) * decay(n, 0.15)
    return 0.8 * w + 0.7 * f + 0.4 * th + 0.45 * pop + 0.3 * c


@sound("ember.impact")
def _(v):
    r, n = v.rng, T(0.5)
    body = fit(smp("punch_h", v.pick("punch_h"), 0.85 * v.j(0.04)), n)
    flare = tvf(pink(n, r), lambda tc, f: bp(f, 2600 * (800 / 2600) ** np.minimum(tc / 0.3, 1), 0.8))
    flare = sat(unit(flare) * adsr(n, 0.003, 0.14, 0, 0.05), 1.5)
    tl = roar(n, r, 100, 1200, 2.0) * decay(n, 0.2, delay=0.02)
    c = unit(crackle(n, r, curve(n, [(0, 600), (0.45, 20)]), 1200, 8000))
    sub = thump(n, 120, 55, 0.03, 0.09)
    return 0.9 * body + 0.7 * flare + 0.35 * tl + 0.4 * c + 0.5 * sub


@sound("ember.blast")
def _(v):
    r, n = v.rng, T(0.95)
    whump = thump(n, 120, 36, 0.05, 0.28, click=0.2, rng=r)
    boom = unit(ffilt(fit(smp("lowboom", v.k), n), lambda f: lp(f, 500, 2)))
    cr = unit(fit(smp("crunch", v.pick("crunch"), 0.85), n))
    rr = roar(n, r, 70, 2200, drive=3.0) * adsr(n, 0.012, 0.3, 0.0, 0.1)
    sparks = crackle(n, r, curve(n, [(0, 50), (0.08, 700), (0.9, 40)]), 2200, 11000)
    sparks = unit(sparks) * curve(n, [(0, 0.2), (0.06, 1), (0.95, 0.3)])
    sizzle = hiss(n, r, 4000, 10000) * decay(n, 0.35)
    return 1.0 * whump + 0.7 * boom + 0.5 * cr + 0.85 * rr + 0.4 * sparks + 0.08 * sizzle


@sound("ember.windup")
def _(v):
    r, n = v.rng, T(0.8)
    dur = n / SR
    e = swell(n, 0.76, start=0.14, fall=0.03, power=1.4)
    fc = curve(n, [(0, 90), (dur, 300)], "exp")
    pitched = ffilt(comb(pink(n, r), fc, g=0.9, K=14), lambda f: hp(f, 80, 2) * lp(f, 3000, 2))
    tone = sat(ffilt(saw(fc * 2, n), lambda f: lp(f, 2200, 2)), 2.0)
    rr = roar(n, r, 100, 2400, 2.2)
    rr = tvf(rr, lambda tc, f: lp(f, 400 * 10 ** (tc / dur), 2))
    edge = whoosh(n, r, 1500, 5500, q=3.5, color=0.5)
    c = crackle(n, r, curve(n, [(0, 40), (dur, 600)]), 1500, 9000)
    return e * (0.75 * unit(pitched) + 0.25 * unit(tone) + 0.7 * unit(rr) + 0.3 * unit(edge)) \
        + 0.3 * unit(c) * e


@sound("ember.dash")
def _(v):
    r, n = v.rng, T(0.36)
    w = whoosh(n, r, 500, 900, q=1.3, mid=2400) * bump(n, 0.09, 0.07)
    t0 = 0.11 * v.j(0.1)
    pe = curve(n, [(0, 0), (t0, 0), (t0 + 0.012, 1), (n / SR, 0.0)]) ** 2
    puff = roar(n, r, 120, 1500, 2.5) * pe
    c = unit(crackle(n, r, 500, 1200, 8000)) * pe
    return 0.9 * unit(w) + 0.7 * puff + 0.35 * c


@sound("ember.buff")
def _(v):
    r, n = v.rng, T(0.6)
    env = adsr(n, 0.05, 0.18, 0.15, 0.2)
    flare = unit(ffilt(pink(n, r), lambda f: bp(f, 700, 0.6))) * env
    rr = roar(n, r, 120, 900, 1.5) * env
    c = unit(crackle(n, r, curve(n, [(0, 350), (0.6, 25)]), 1000, 6000)) * decay(n, 0.3)
    glow = ffilt(chord_pad(n, [mf(43), mf(50)], r, voices=2), lambda f: lp(f, 600, 2))
    glow = unit(glow) * swell(n, 0.12, 0.2, 0.3, 1.0)
    return 0.6 * flare + 0.5 * rr + 0.45 * c + 0.18 * glow


@sound("ember.ult")
def _(v):
    r, n = v.rng, T(1.4)
    dur = n / SR
    e = swell(n, 1.15, start=0.15, fall=0.22, power=1.5)
    rr = roar(n, r, 60, 2500, 2.6)
    rr = tvf(rr, lambda tc, f: lp(f, 300 * 12 ** np.minimum(tc / 1.15, 1), 2))
    sub = sine(curve(n, [(0, 42), (dur, 58)], "exp"))
    f0 = curve(n, [(0, mf(55)), (dur, mf(55) * 1.02)])
    ring = drone(n, f0, [1, 2, 3.01, 4.2, 5.4], [1, 0.5, 0.35, 0.2, 0.1], r, beat=0.003)
    ring += 0.6 * drone(n, f0 * 1.5, [1, 2, 3.01], [1, 0.4, 0.2], r, beat=0.003)
    ring = unit(ring) * swell(n, 0.5, start=0.35, fall=0.3, power=1.2)
    c = unit(crackle(n, r, curve(n, [(0, 80), (dur, 500)]), 1200, 8000))
    return e * 0.85 * unit(rr) + 0.6 * ring + 0.3 * sub * e + 0.3 * c * e


# --- frost: glass and ice. Struck glass partials, ice ticks, breath; recorded glass and snow.

@sound("frost.cast")
def _(v):
    r, n = v.rng, T(0.36)
    dur = n / SR
    e = swell(n, 0.3, start=0.2, fall=0.04, power=1.5)
    breath = tvf(pink(n, r), lambda tc, f: bp(f, 2000 * 3 ** (tc / dur), 0.9))
    tk = glints(n, r, curve(n, [(0, 25), (dur, 150)]), 2200, 5000, (0.03, 0.09),
                fcurve=curve(n, [(0, 1), (dur, 1.6)], "exp"))
    sh = drone(n, curve(n, [(0, 1400), (dur, 2100)], "exp"), [1, 2.32], [1, 0.3], r, beat=0.004)
    return e * (0.6 * unit(breath) + 0.22 * unit(sh)) + 0.55 * unit(tk)


@sound("frost.launch")
def _(v):
    r, n = v.rng, T(0.36)
    dur = n / SR
    t = tt(n)
    f = curve(n, [(0, 3300 * v.j(0.05)), (dur, 2300)], "exp") * (1 + 0.012 * np.sin(2 * np.pi * 11 * t))
    we = adsr(n, 0.01, 0.1, 0.3, 0.08)
    wh = sine(f) + 0.3 * sine(f * 2.32)
    fb = bandcurve(n, f)
    airy = tvf(white(n, r), lambda tc, fr: bp(fr, fb(tc), 10))
    w = whoosh(n, r, 3000, 700, q=1.0) * decay(n, 0.1)
    glint = fit(smp("glass_l", v.pick("glass_l"), 1.6), n)
    return 0.4 * wh * we + 0.4 * unit(airy) * we + 0.7 * unit(w) + 0.35 * glint


@sound("frost.impact")
def _(v):
    r, n = v.rng, T(0.5)
    crack = fit(smp("glass_h", v.pick("glass_h"), v.j(0.05)), n) * decay(n, 0.15, delay=0.04)
    crunch = fit(smp("step_snow", v.pick("step_snow"), 0.8), n)
    spray = glints(n, r, curve(n, [(0, 900), (0.05, 500), (0.35, 0)]), 2200, 7500, (0.04, 0.18))
    sub = thump(n, 150, 70, 0.02, 0.07)
    return 0.9 * crack + 0.5 * crunch + 0.5 * unit(spray) + 0.45 * sub


@sound("frost.blast")
def _(v):
    r, n = v.rng, T(0.95)
    crunch = fit(smp("step_snow", v.pick("step_snow"), 0.55), n)
    c2 = unit(ffilt(fit(smp("crunch", v.pick("crunch"), 0.9), n), lambda f: lp(f, 2500, 2)))
    s1 = fit(smp("glass_h", v.pick("glass_h"), 0.8), n) * decay(n, 0.3, delay=0.05)
    s2 = at(n, smp("glass_h", v.pick("glass_h", 2), 0.72), 0.03) * decay(n, 0.3, delay=0.08)
    shards = glints(n, r, curve(n, [(0, 500), (0.1, 250), (0.8, 5)]), 1500, 6500, (0.12, 0.35))
    sub = thump(n, 100, 38, 0.04, 0.22, click=0.2, rng=r)
    return 0.7 * crunch + 0.45 * c2 + 0.6 * s1 + 0.45 * s2 + 0.55 * unit(shards) + 0.8 * sub


@sound("frost.windup")
def _(v):
    r, n = v.rng, T(0.8)
    dur = n / SR
    e = swell(n, 0.76, start=0.14, fall=0.03, power=1.3)
    fc = curve(n, [(0, 420), (dur, 980)], "exp")
    dr = drone(n, fc, [1, 2.32, 4.25, 1.004], [1, 0.45, 0.2, 0.7], r, beat=0.004)
    dr *= trem(n, curve(n, [(0, 6), (dur, 15)]), 0.35)
    air = ffilt(comb(white(n, r), fc * 2, g=0.85, K=10), lambda f: hp(f, 1000, 2) * lp(f, 9000, 2))
    ticks = crackle(n, r, curve(n, [(0, 30), (dur, 500)]), 2500, 11000)
    return e * (0.7 * unit(dr) + 0.35 * unit(air)) + 0.3 * unit(ticks) * np.sqrt(e)


@sound("frost.dash")
def _(v):
    r, n = v.rng, T(0.36)
    sw = whoosh(n, r, 1200, 1500, q=1.5, color=0.8, mid=4200) * bump(n, 0.08, 0.065)
    ch = at(n, bell(T(0.26), 1900 * v.j(0.06), r, length=0.25, kind=GLASS, bright=0.9), 0.1)
    return 0.85 * unit(sw) + 0.45 * unit(ch)


@sound("frost.buff")
def _(v):
    r, n = v.rng, T(0.6)
    t = tt(n)
    b1 = bell(n, 1320 * v.j(0.02), r, length=0.45, kind=GLASS) * np.minimum(1, t / 0.015 + 0.15)
    b2 = at(n, bell(n, 1980 * v.j(0.02), r, length=0.35, kind=GLASS), 0.06)
    breath = unit(ffilt(pink(n, r), lambda f: hp(f, 4000, 2) * lp(f, 11000, 2))) * adsr(n, 0.08, 0.2, 0, 0.1)
    tk = glints(n, r, 25, 4000, 8000, (0.03, 0.1))
    return trem(n, 7, 0.25) * (0.7 * unit(b1) + 0.45 * unit(b2)) + 0.2 * breath + 0.25 * unit(tk)


@sound("frost.ult")
def _(v):
    r, n = v.rng, T(1.4)
    dur = n / SR
    fc = curve(n, [(0, 380), (0.7, 950), (dur, 620)], "exp")
    fb = bandcurve(n, fc)
    howl = tvf(pink(n, r), lambda tc, f: bp(f, fb(tc), 5))
    howl = unit(howl) * (0.7 + 0.3 * wob(n, r, 3)) + 0.25 * sine(fc * (1 + 0.01 * wob(n, r, 5)))
    ge = swell(n, 0.7, start=0.3, fall=0.3, power=1.2)
    cracks = np.zeros(n)
    for i, (when, rate) in enumerate(((0.0, 0.9), (0.42, 0.75), (0.8, 0.85))):
        place(cracks, smp("glass_h", v.pick("glass_h", i)), when, 1.0 - 0.15 * i)
        place(cracks, smp("step_snow", v.pick("step_snow", i), 0.6), when, 0.5)
    dr = drone(n, curve(n, [(0, 130), (dur, 124)]), [1, 2.32, 4.25], [1, 0.5, 0.25], r, beat=0.004)
    dr = unit(dr) * swell(n, 0.6, start=0.25, fall=0.35, power=1.2)
    return ge * 0.7 * unit(howl) + 0.6 * unit(cracks) + 0.5 * dr


# --- tide: water. Splashes, bubbles (rising-pitch sines), washes and rushes, all synthesised.

def splash(n, rng, size=1.0, lo=300, hi=7000):
    t = tt(n)
    ae = np.minimum(1, t / 0.002)
    burst = unit(ffilt(pink(n, rng), lambda f: bp(f, 1400, 0.5) * hp(f, lo, 2))) * np.exp(-t / (0.05 * size)) * ae
    grain = 0.5 + 0.5 * np.abs(wob(n, rng, 70))
    spray = unit(ffilt(white(n, rng), lambda f: hp(f, 2500, 2) * lp(f, hi, 2))) * np.exp(-t / (0.14 * size)) * grain * ae
    bb = bubbles(n, rng, curve(n, [(0, 900 * size), (0.35 * size, 0)]), 250 / size ** 0.3, 1600)
    return 1.0 * burst + 0.45 * spray + 0.6 * unit(bb)


@sound("tide.cast")
def _(v):
    r, n = v.rng, T(0.36)
    dur = n / SR
    e = swell(n, 0.3, start=0.2, fall=0.04, power=1.4)
    ws = tvf(pink(n, r), lambda tc, f: bp(f, 250 * 6 ** (tc / dur), 0.9)) * (1 + 0.35 * wob(n, r, 6))
    bb = bubbles(n, r, curve(n, [(0, 50), (dur, 500)]), 300, 1400)
    low = ffilt(brown(n, r), lambda f: lp(f, 300, 2))
    return e * (0.8 * unit(ws) + 0.35 * unit(low)) + 0.45 * unit(bb) * np.sqrt(e)


@sound("tide.launch")
def _(v):
    r, n = v.rng, T(0.42)
    sp = splash(n, r, 0.6)
    rush = whoosh(n, r, 2000 * v.j(0.1), 500, q=0.8, color=1.0) * adsr(n, 0.015, 0.15, 0, 0.05)
    bb = bubbles(n, r, curve(n, [(0, 400), (0.35, 0)]), 400, 1600)
    return 0.6 * unit(sp) + 0.8 * unit(rush) + 0.35 * unit(bb)


@sound("tide.impact")
def _(v):
    r, n = v.rng, T(0.5)
    body = fit(smp("soft_h", v.pick("soft_h"), 0.9), n)
    sp = splash(n, r, 1.2)
    sub = thump(n, 110, 50, 0.025, 0.08)
    return 0.7 * body + 0.9 * unit(sp) + 0.5 * sub


@sound("tide.blast")
def _(v):
    r, n = v.rng, T(0.95)
    t = tt(n)
    sub = thump(n, 85, 34, 0.05, 0.3)
    boom = unit(ffilt(fit(smp("lowboom", v.k), n), lambda f: lp(f, 300, 2)))
    sp = splash(n, r, 2.0)
    crash = sat(unit(ffilt(pink(n, r), lambda f: bp(f, 600, 0.5))) * adsr(n, 0.01, 0.3, 0, 0.1), 1.8)
    fizz = bubbles(n, r, curve(n, [(0, 3000), (0.9, 100)]), 2000, 5000)
    foam = unit(ffilt(white(n, r), lambda f: hp(f, 3000, 2) * lp(f, 9000, 2))) * np.exp(-t / 0.5) \
        * (0.5 + 0.5 * np.abs(wob(n, r, 50)))
    return 0.9 * sub + 0.5 * boom + 0.8 * unit(sp) + 0.6 * crash + 0.3 * unit(fizz) + 0.18 * foam


@sound("tide.windup")
def _(v):
    r, n = v.rng, T(0.8)
    dur = n / SR
    e = swell(n, 0.76, start=0.14, fall=0.03, power=1.3)
    rush = tvf(brown(n, r) + pink(n, r), lambda tc, f: lp(f, 200 * 11 ** (tc / dur), 2))
    fc = curve(n, [(0, 60), (dur, 180)], "exp")
    pitched = ffilt(comb(pink(n, r), fc, g=0.9, K=14), lambda f: hp(f, 50, 2) * lp(f, 2500, 2))
    tone = ffilt(saw(fc * 2, n), lambda f: lp(f, 1200, 2))
    bb = bubbles(n, r, curve(n, [(0, 50), (dur, 900)]), 250, 1200)
    return e * (0.75 * unit(rush) + 0.6 * unit(pitched) + 0.15 * unit(tone)) + 0.35 * unit(bb) * e


@sound("tide.dash")
def _(v):
    r, n = v.rng, T(0.36)
    rush = whoosh(n, r, 700, 900, q=1.0, mid=2200) * bump(n, 0.08, 0.07)
    sp = at(n, splash(T(0.3), r, 0.5), 0.1 * v.j(0.1))
    return 0.85 * unit(rush) + 0.6 * unit(sp)


@sound("tide.buff")
def _(v):
    r, n = v.rng, T(0.6)
    wash = unit(ffilt(pink(n, r), lambda f: lp(f, 1500, 2) * hp(f, 120, 2))) * (1 + 0.3 * wob(n, r, 3))
    e = adsr(n, 0.12, 0.2, 0.2, 0.2)
    bb = bubbles(n, r, curve(n, [(0, 80), (0.15, 160), (0.6, 30)]), 500, 1500)
    return 0.7 * wash * e + 0.5 * unit(bb)


@sound("tide.ult")
def _(v):
    r, n = v.rng, T(1.4)
    dur = n / SR
    e = swell(n, 1.1, start=0.15, fall=0.2, power=1.4)
    whirl = phase(curve(n, [(0, 2.5), (dur, 8.0)]), n)
    centre = 700 * (1 + 0.55 * np.sin(whirl)) * curve(n, [(0, 1), (dur, 2.0)], "exp")
    fb = bandcurve(n, centre)
    w = tvf(pink(n, r), lambda tc, f: bp(f, fb(tc), 1.5))
    w = flange(unit(w), 0.7, 1.0, 6.0, 0.6)
    roar_ = unit(sat(unit(ffilt(brown(n, r), lambda f: lp(f, 250, 2))), 1.6))
    fc = curve(n, [(0, 55), (dur, 110)], "exp")
    pitched = ffilt(comb(pink(n, r), fc, g=0.88, K=12), lambda f: lp(f, 2000, 2))
    bb = bubbles(n, r, curve(n, [(0, 100), (dur, 1500)]), 300, 1600)
    return e * (0.8 * unit(w) + 0.6 * roar_ + 0.4 * unit(pitched)) + 0.35 * unit(bb) * e


# --- wind: air. Bands of noise that move, narrow ones that howl, whistles; recorded cloth.

def gust(n, rng, fc, q=1.5, color=1.0):
    fb = bandcurve(n, fc if not np.isscalar(fc) else np.full(n, float(fc)))
    return tvf(shaped(n, rng, color), lambda tc, f: bp(f, fb(tc), q))


def whistle(n, rng, fc, wobble=0.01, rate=5):
    f = fc * (1 + wobble * wob(n, rng, rate))
    return sine(f) * (0.6 + 0.4 * wob(n, rng, 8))


@sound("wind.cast")
def _(v):
    r, n = v.rng, T(0.36)
    dur = n / SR
    e = swell(n, 0.3, start=0.2, fall=0.04, power=1.5)
    g = gust(n, r, curve(n, [(0, 400), (dur, 3000)], "exp"), 1.3)
    wh = whistle(n, r, curve(n, [(0, 1800), (dur, 2600)], "exp"), 0.01)
    return e * (0.9 * unit(g) + 0.15 * wh)


@sound("wind.launch")
def _(v):
    r, n = v.rng, T(0.36)
    dur = n / SR
    g = gust(n, r, curve(n, [(0, 4500 * v.j(0.1)), (dur, 1200)], "exp"), 2.2)
    wh = whistle(n, r, curve(n, [(0, 2800 * v.j(0.05)), (dur, 1500)], "exp"), 0.008)
    snap = ffilt(white(n, r), lambda f: hp(f, 3000, 2)) * np.exp(-tt(n) / 0.003)
    return 0.9 * unit(g) * adsr(n, 0.004, 0.09, 0, 0.05) + 0.35 * wh * adsr(n, 0.006, 0.1, 0, 0.05) + 0.3 * unit(snap)


@sound("wind.impact")
def _(v):
    r, n = v.rng, T(0.45)
    dur = n / SR
    sw = gust(n, r, curve(n, [(0, 3500), (dur, 1500)], "exp"), 1.8) * bump(n, 0.03, 0.035)
    hit = 0.07 * v.j(0.1)
    th = at(n, smp("soft_m", v.pick("soft_m")), hit) + at(n, smp("punch_m", v.pick("punch_m"), 0.9), hit, 0.6)
    low = at(n, thump(T(0.2), 120, 55, 0.02, 0.05), hit)
    return 0.8 * unit(sw) + 0.8 * unit(th) + 0.4 * low


@sound("wind.blast")
def _(v):
    r, n = v.rng, T(0.95)
    t = tt(n)
    deep = tvf(brown(n, r), lambda tc, f: lp(f, 200 + 700 * np.sin(np.pi * np.minimum(tc / 0.9, 1)), 2))
    sw = gust(n, r, 900 * (1 + 0.5 * np.sin(2 * np.pi * 3.5 * t)), 3)
    sw = flange(unit(sw), 1.2, 0.5, 4, 0.6)
    env = adsr(n, 0.03, 0.35, 0, 0.1)
    puff = fit(smp("soft_h", v.pick("soft_h"), 0.6), n)
    return env * (0.9 * unit(deep) + 0.6 * unit(sw)) + 0.4 * puff


@sound("wind.windup")
def _(v):
    r, n = v.rng, T(0.8)
    dur = n / SR
    e = swell(n, 0.76, start=0.14, fall=0.03, power=1.3)
    fc = curve(n, [(0, 280), (dur, 900)], "exp") * (1 + 0.012 * np.sin(2 * np.pi * 5.5 * tt(n)))
    hw = whistle(n, r, fc, 0.006) + 0.5 * whistle(n, r, fc * 1.505, 0.006)
    g = gust(n, r, fc, 8)
    air = gust(n, r, fc * 3, 1.2)
    return e * (0.55 * unit(hw) + 0.7 * unit(g) + 0.25 * unit(air))


@sound("wind.dash")
def _(v):
    r, n = v.rng, T(0.36)
    t = tt(n)
    cl = fit(smp("cloth", v.pick("cloth"), 1.1), n)
    w = whoosh(n, r, 600, 900, q=1.2, mid=2600) * bump(n, 0.09, 0.07)
    w = w * (1 - 0.5 * (0.5 + 0.5 * np.sin(2 * np.pi * 28 * t)))
    return 0.6 * cl + 0.85 * unit(w)


@sound("wind.buff")
def _(v):
    r, n = v.rng, T(0.6)
    t = tt(n)
    dur = n / SR
    g = gust(n, r, curve(n, [(0, 300), (dur, 1600)], "exp"), 1.0)
    e = swell(n, 0.35, start=0.25, fall=0.22, power=1.3)
    fl = 1 - 0.35 * (0.5 + 0.5 * np.sin(2 * np.pi * 16 * t))
    cl = at(n, smp("cloth", v.pick("cloth"), 1.2), 0.05)
    return 0.85 * unit(g) * e * fl + 0.25 * cl


@sound("wind.ult")
def _(v):
    r, n = v.rng, T(1.4)
    dur = n / SR
    e = swell(n, 1.1, start=0.15, fall=0.22, power=1.4)
    deep = tvf(brown(n, r), lambda tc, f: lp(f, 200 * 5 ** (tc / dur), 2))
    spin = phase(curve(n, [(0, 1.5), (dur, 6.0)]), n)
    base = curve(n, [(0, 500), (dur, 1300)], "exp")
    sp = gust(n, r, base * (1 + 0.5 * np.sin(spin)), 2.5)
    wh = whistle(n, r, curve(n, [(0, 700), (dur, 1500)], "exp") * (1 + 0.01 * np.sin(2 * np.pi * 6 * tt(n))), 0.005)
    return e * (0.8 * unit(sat(unit(deep), 1.5)) + 0.7 * unit(sp) + 0.28 * wh)


# --- dusk: night and starlight. Sine grains (sparkle), flangers (the phasey dark), low hums.

def hum(n, rng, f=55, cutoff=500):
    return unit(ffilt(chord_pad(n, [f, f * 1.5], rng, detune=0.008, voices=3),
                      lambda q: lp(q, cutoff, 2) * hp(q, 40, 2)))


def stars(n, rng, rate, flo=2500, fhi=9000, fcurve=None, amp=None, d=(0.015, 0.06)):
    return grains(n, rng, rate, flo, fhi, d[0], d[1], amp, fcurve)


@sound("dusk.cast")
def _(v):
    r, n = v.rng, T(0.36)
    dur = n / SR
    e = swell(n, 0.3, start=0.2, fall=0.04, power=1.4)
    s = stars(n, r, curve(n, [(0, 80), (dur, 400)]), 1500, 6000, fcurve=curve(n, [(0, 1), (dur, 1.5)], "exp"))
    s = flange(s, 3, 0.5, 3)
    h = hum(n, r, 55 * v.j(0.02))
    w = whoosh(n, r, 500, 2500, q=1.2, color=1.0)
    return 0.6 * unit(s) * np.sqrt(e) + 0.55 * h * e + 0.35 * unit(w) * e


@sound("dusk.launch")
def _(v):
    r, n = v.rng, T(0.42)
    w = whoosh(n, r, 2200 * v.j(0.1), 600, q=1.2) * adsr(n, 0.01, 0.14, 0, 0.05)
    w = flange(unit(w), 5, 0.3, 3, 0.8)
    s = stars(n, r, curve(n, [(0, 400), (0.25, 0)]), 3000, 9000)
    th = thump(n, 200, 90, 0.02, 0.06)
    return 0.9 * unit(w) + 0.45 * unit(s) + 0.4 * th


@sound("dusk.impact")
def _(v):
    r, n = v.rng, T(0.5)
    dur = n / SR
    th = thump(n, 160, 75, 0.02, 0.12)
    t = tt(n)
    hol = ffilt(pink(n, r), lambda f: 1.0 * bp(f, 380, 5) + 0.6 * bp(f, 950, 6)) * np.exp(-t / 0.06)
    body = fit(smp("soft_m", v.pick("soft_m"), 0.8), n)
    sh = stars(n, r, curve(n, [(0, 300), (0.45, 0)]), 600, 2400, fcurve=curve(n, [(0, 1), (dur, 0.7)], "exp"), d=(0.03, 0.09))
    sh = flange(sh, 2.5, 0.5, 4)
    return 0.8 * th + 0.6 * unit(hol) + 0.4 * body + 0.5 * unit(sh)


@sound("dusk.blast")
def _(v):
    r, n = v.rng, T(0.95)
    t = tt(n)
    th = thump(n, 95, 32, 0.05, 0.3, click=0.2, rng=r)
    boom = unit(ffilt(fit(smp("lowboom", v.k), n), lambda f: lp(f, 250, 2)))
    hol = unit(ffilt(pink(n, r), lambda f: bp(f, 320, 4))) * np.exp(-t / 0.08)
    sw = stars(n, r, curve(n, [(0, 800), (0.9, 50)]), 500, 4000, d=(0.03, 0.1))
    sw = flange(sw, 2, 0.5, 5, 0.8)
    pad = unit(ffilt(chord_pad(n, [mf(45), mf(48), mf(52)], r), lambda f: lp(f, 900, 2))) * adsr(n, 0.05, 0.4, 0, 0.15)
    return 0.9 * th + 0.6 * boom + 0.4 * hol + 0.55 * unit(sw) + 0.25 * pad


@sound("dusk.windup")
def _(v):
    r, n = v.rng, T(0.8)
    dur = n / SR
    e = swell(n, 0.76, start=0.14, fall=0.03, power=1.3)
    f = curve(n, [(0, 550), (dur, 2100)], "exp")
    wh = sine(f) + sine(f * 2 ** (9 / 1200)) + sine(f * 2 ** (-11 / 1200)) + 0.4 * sine(f / 2)
    wh = unit(wh) * trem(n, curve(n, [(0, 5), (dur, 15)]), 0.3)
    s = stars(n, r, curve(n, [(0, 40), (dur, 500)]), 2500, 9000)
    cb = ffilt(comb(white(n, r), f, g=0.8, K=8), lambda q: hp(q, 400, 2) * lp(q, 9000, 2))
    return e * (0.6 * wh + 0.35 * unit(s) + 0.3 * unit(cb))


@sound("dusk.dash")
def _(v):
    r, n = v.rng, T(0.36)
    w = whoosh(n, r, 600, 700, q=1.0, color=0.8, mid=2000) * bump(n, 0.09, 0.07)
    w = flange(unit(w), 4, 0.3, 2.5, 0.7)
    pt = 0.2 * v.j(0.08)
    pop = at(n, thump(T(0.1), 700, 220, 0.006, 0.03), pt)
    s = at(n, stars(T(0.15), r, 200, 3000, 8000), pt)
    return 0.85 * unit(w) + 0.5 * pop + 0.25 * unit(s)


@sound("dusk.buff")
def _(v):
    r, n = v.rng, T(0.6)
    s = flange(stars(n, r, 120, 800, 3000, d=(0.04, 0.12)), 1.5, 0.5, 4, 0.8)
    pad = unit(ffilt(chord_pad(n, [mf(57), mf(60), mf(64)], r), lambda f: lp(f, 1500, 2)))
    e = curve(n, [(0, 0.35), (0.1, 1), (0.35, 0.7), (0.6, 0)])
    return 0.6 * unit(s) * np.sqrt(e) + 0.3 * pad * e + 0.3 * hum(n, r, 55) * e


@sound("dusk.ult")
def _(v):
    r, n = v.rng, T(1.4)
    t = tt(n)
    hit = 0.72
    pre = np.clip(t / hit, 0, 1)
    fall = curve(n, [(0, 4500), (hit, 350)], "exp")
    fb = bandcurve(n, fall)
    roar_ = tvf(pink(n, r), lambda tc, f: bp(f, fb(tc), 1.6)) * (0.2 + 0.8 * pre ** 1.5) * (t < hit + 0.02)
    whine_f = curve(n, [(0, 2400), (hit, 260)], "exp")
    whine = (sine(whine_f) + 0.5 * sine(whine_f * 1.5)) * (0.25 + 0.75 * pre) * (t < hit + 0.02)
    boom = at(n, thump(n, 90, 30, 0.05, 0.35, click=0.3, rng=r), hit)
    lb = at(n, unit(ffilt(smp("lowboom", 0), lambda f: lp(f, 300, 2))), hit)
    cr = at(n, unit(ffilt(smp("crunch", v.pick("crunch"), 0.8), lambda f: lp(f, 1500, 2))), hit)
    sh = stars(n, r, np.where(t > hit, 500 * np.exp(-(t - hit) / 0.3), 0), 800, 6000, d=(0.03, 0.1))
    sh = flange(sh, 1.5, 0.5, 5, 0.8)
    pad = unit(ffilt(chord_pad(n, [mf(45), mf(52), mf(57)], r), lambda f: lp(f, 900, 2)))
    pad *= np.where(t > hit, np.exp(-(t - hit) / 0.35), 0)
    return 0.7 * unit(roar_) + 0.3 * whine + 1.0 * boom + 0.6 * lb + 0.45 * cr + 0.5 * unit(sh) + 0.25 * pad


# --- growth: wood, leaves, chimes. Recorded creaks, wood and grass; synthesised rustle and
# tubular chimes on a pentatonic scale.

PENTA = [mf(m) for m in (72, 74, 76, 79, 81, 84, 86, 88, 91, 93)]


def rustle(n, rng, rate, lo=1500, hi=8000):
    out = np.zeros(n)
    for _ in range(3):
        vv = impulses(n, rng, (rate if np.isscalar(rate) else np.asarray(rate)) / 3, mode="uniform")
        L = T(rng.uniform(0.006, 0.02))
        ker = white(L, rng) * np.hanning(L)
        fc = math.exp(rng.uniform(math.log(2500), math.log(6000)))
        ker = ffilt(ker, lambda f: bp(f, fc, 0.9), pad=0.02)
        out += np.convolve(vv, ker)[:n]
    return ffilt(out, lambda f: hp(f, lo, 2) * lp(f, hi, 2))


def chimes(n, rng, notes, times, length=0.6, bright=0.8, gains=None):
    out = np.zeros(n)
    for i, (f, when) in enumerate(zip(notes, times)):
        L = n - T(when)
        if L <= 0:
            continue
        g = 1.0 if gains is None else gains[i]
        place(out, bell(L, f, rng, length=length, kind=CHIME, bright=bright, beat=0.001), when, g)
    return out


def knock(n, rng, f0, tau=0.08):
    return partials(n, f0, BAR[0], BAR[1], [tau, tau * 0.4, tau * 0.15], rng)


@sound("growth.cast")
def _(v):
    r, n = v.rng, T(0.36)
    dur = n / SR
    cr = smp("creak", v.pick("creak"), v.j(0.05))
    cr = fit(cr, n) * swell(n, 0.05, 0.4, 0.06, 1.0)
    rs = rustle(n, r, curve(n, [(0, 200), (dur, 900)]))
    kn = knock(n, r, 420 * v.j(0.05), 0.05)
    return 0.8 * unit(cr) + 0.45 * unit(rs) * swell(n, 0.3, 0.3, 0.05) + 0.3 * unit(kn)


@sound("growth.launch")
def _(v):
    r, n = v.rng, T(0.36)
    t = tt(n)
    w = whoosh(n, r, 1600 * v.j(0.1), 600, q=1.0) * adsr(n, 0.01, 0.12, 0, 0.05)
    rs = rustle(n, r, 900) * (1 - 0.6 * (0.5 + 0.5 * np.sin(2 * np.pi * 22 * t))) * decay(n, 0.12)
    gr = fit(smp("step_grass", v.pick("step_grass"), 1.3), n) * decay(n, 0.15)
    return 0.8 * unit(w) + 0.5 * unit(rs) + 0.35 * unit(gr)


@sound("growth.impact")
def _(v):
    r, n = v.rng, T(0.45)
    wd = fit(smp("wood_h", v.pick("wood_h"), v.j(0.05)), n)
    ch = fit(smp("chop", 0, 1.1 * v.j(0.05)), n)
    kn = knock(n, r, 300 * v.j(0.05), 0.06)
    rs = rustle(n, r, curve(n, [(0, 600), (0.4, 0)]))
    pm = unit(ffilt(fit(smp("punch_m", v.pick("punch_m"), 0.9), n), lambda f: lp(f, 1500, 2)))
    return 0.8 * wd + 0.5 * ch + 0.4 * unit(kn) + 0.4 * unit(rs) + 0.4 * pm


@sound("growth.blast")
def _(v):
    r, n = v.rng, T(0.95)
    soft = fit(smp("soft_h", v.pick("soft_h"), 0.8), n)
    th = thump(n, 110, 50, 0.03, 0.12)
    gr = fit(smp("step_grass", v.pick("step_grass"), 0.9), n)
    rs = rustle(n, r, curve(n, [(0, 1200), (0.6, 0)]))
    base = v.k
    notes = [PENTA[base + i] for i in range(5)]
    ch = chimes(n, r, notes, [0.05, 0.13, 0.21, 0.29, 0.37], length=0.5, gains=[0.7, 0.75, 0.8, 0.9, 1.0])
    return 0.7 * soft + 0.6 * th + 0.4 * unit(gr) + 0.35 * unit(rs) + 0.45 * unit(ch)


@sound("growth.windup")
def _(v):
    r, n = v.rng, T(0.8)
    dur = n / SR
    e = swell(n, 0.76, start=0.14, fall=0.03, power=1.3)
    cr = smp("creak", 1)
    cr = np.concatenate([cr, cr[::-1], cr])
    cr = fit(varispeed(cr, curve(n, [(0, 0.55), (dur, 1.35)], "exp")), n)
    fc = curve(n, [(0, 65), (dur, 150)], "exp") * (1 + 0.01 * wob(n, r, 6))
    groan = vowel(sat(saw(fc, n), 1.5), "o")
    cb = ffilt(comb(pink(n, r), fc, g=0.88, K=12), lambda f: lp(f, 2500, 2))
    return 0.55 * unit(cr) * np.sqrt(e) + e * (0.6 * unit(groan) + 0.4 * unit(cb))


@sound("growth.dash")
def _(v):
    r, n = v.rng, T(0.36)
    b = bump(n, 0.09, 0.07)
    rs = rustle(n, r, 1500) * b
    w = whoosh(n, r, 800, 900, q=1.1, mid=2400) * b
    gr = fit(smp("step_grass", v.pick("step_grass"), 1.4), n)
    return 0.6 * unit(rs) + 0.7 * unit(w) + 0.35 * unit(gr)


@sound("growth.buff")
def _(v):
    r, n = v.rng, T(0.6)
    t = tt(n)
    notes = [mf(88), mf(91), mf(96)] if v.k == 0 else [mf(86), mf(91), mf(93)]
    ch = chimes(n, r, notes, [0, 0.06, 0.13], length=0.45, bright=0.7) * np.minimum(1, t / 0.004)
    br = unit(ffilt(pink(n, r), lambda f: hp(f, 2000, 2) * lp(f, 9000, 2))) * adsr(n, 0.1, 0.25, 0, 0.1)
    return 0.7 * unit(ch) + 0.25 * br


@sound("growth.ult")
def _(v):
    r, n = v.rng, T(1.4)
    dur = n / SR
    times = [0.0] + sorted(r.uniform(0.05, 1.15, 15) ** 0.8)
    notes = [PENTA[int(r.integers(0, 8))] for _ in times]
    gains = [0.6 + 0.4 * (w / 1.2) for w in times]
    ch = chimes(n, r, notes, times, length=0.45, bright=0.7, gains=gains)
    pad = sum(sine(mf(m) * (1 + 0.002 * r.standard_normal()), n) for m in (60, 64, 67, 74))
    pad = unit(ffilt(pad + 0.3 * saw(mf(48), n), lambda f: lp(f, 2500, 2))) * swell(n, 1.0, 0.1, 0.25, 1.5)
    rs = rustle(n, r, curve(n, [(0, 300), (dur, 1500)])) * swell(n, 1.1, 0.25, 0.25, 1.2)
    return 0.55 * unit(ch) + 0.35 * pad + 0.35 * unit(rs)


# --- stone: rock and rumble. Recorded picks on rock, punches, concrete; synthesised rumble,
# grinding and rubble.

def rumble(n, rng, cutoff=140, drive=1.5):
    x = unit(ffilt(brown(n, rng), lambda f: hp(f, 35, 2) * lp(f, cutoff, 2))) * (1 + 0.4 * wob(n, rng, 5))
    return unit(sat(x, drive))


def grind(n, rng, rate=120):
    vv = impulses(n, rng, rate, mode="uniform")
    L = T(0.012)
    ker = ffilt(white(L, rng), lambda f: bp(f, 600, 0.7), pad=0.02) * np.hanning(L)
    g = np.convolve(vv, ker)[:n]
    b = ffilt(white(n, rng), lambda f: bp(f, 700, 0.8)) * (0.5 + 0.5 * np.abs(wob(n, rng, 30)))
    return unit(g) + 0.5 * unit(b)


def rubble(n, rng, rate, lo=300, hi=4000):
    out = np.zeros(n)
    for _ in range(4):
        vv = impulses(n, rng, (rate if np.isscalar(rate) else np.asarray(rate)) / 4)
        L = T(rng.uniform(0.004, 0.015))
        ker = white(L, rng) * np.exp(-tt(L) / (L / SR / 4))
        fc = math.exp(rng.uniform(math.log(lo * 2), math.log(hi / 2)))
        out += np.convolve(vv, ffilt(ker, lambda f: bp(f, fc, 1.0), pad=0.02))[:n]
    return ffilt(out, lambda f: hp(f, lo, 2) * lp(f, hi, 2))


@sound("stone.cast")
def _(v):
    r, n = v.rng, T(0.36)
    e = swell(n, 0.3, start=0.3, fall=0.05, power=1.3)
    return e * (0.9 * rumble(n, r, 160) + 0.45 * unit(grind(n, r, 150)))


@sound("stone.launch")
def _(v):
    r, n = v.rng, T(0.42)
    ph = fit(smp("punch_h", v.pick("punch_h"), 0.75), n)
    mn = unit(ffilt(fit(smp("mining", v.pick("mining"), 0.7), n), lambda f: lp(f, 3000, 2)))
    w = whoosh(n, r, 900, 250, q=0.9, color=1.5) * adsr(n, 0.02, 0.15, 0, 0.05)
    th = thump(n, 90, 45, 0.03, 0.1)
    return 0.8 * ph + 0.45 * mn + 0.7 * unit(w) + 0.5 * th


@sound("stone.impact")
def _(v):
    r, n = v.rng, T(0.5)
    mn = fit(smp("mining", v.pick("mining"), 0.85 * v.j(0.03)), n)
    ph = fit(smp("punch_h", v.pick("punch_h"), 0.8), n)
    cr = unit(ffilt(fit(smp("crunch", v.pick("crunch"), 1.0), n), lambda f: lp(f, 2500, 2)))
    rb = rubble(n, r, curve(n, [(0, 300), (0.4, 0)]))
    return 0.8 * mn + 0.8 * ph + 0.4 * cr + 0.3 * unit(rb)


@sound("stone.blast")
def _(v):
    r, n = v.rng, T(0.95)
    boom = unit(ffilt(fit(smp("lowboom", v.k), n), lambda f: lp(f, 400, 2)))
    th = thump(n, 80, 30, 0.05, 0.35, click=0.3, rng=r)
    cr = fit(smp("crunch", v.pick("crunch"), 0.8), n)
    mn = fit(smp("mining", v.pick("mining"), 0.6), n)
    rb = rubble(n, r, curve(n, [(0, 250), (0.85, 8)]))
    return 0.6 * boom + 0.9 * th + 0.5 * unit(cr) + 0.5 * mn + 0.4 * unit(rb)


@sound("stone.windup")
def _(v):
    r, n = v.rng, T(0.8)
    t = tt(n)
    dur = n / SR
    f = curve(n, [(0, 2400), (dur, 650)], "exp")
    fb = bandcurve(n, f)
    wh = sine(f) + 0.25 * sine(2 * f)
    band = tvf(white(n, r), lambda tc, q: bp(q, fb(tc), 15))
    a = curve(n, [(0, 0.3), (dur, 1.0)]) ** 1.3
    ru = rumble(n, r, 200, 2.0) * np.clip((t - 0.3) / (dur - 0.3), 0, 1) ** 2
    return a * (0.6 * wh + 0.3 * unit(band)) + 0.7 * ru


@sound("stone.dash")
def _(v):
    r, n = v.rng, T(0.36)
    out = np.zeros(n)
    for i, when in enumerate((0.0, 0.12 * v.j(0.05), 0.24 * v.j(0.05))):
        place(out, smp("step_concrete", v.pick("step_concrete", i), 0.6), when, 0.8)
        place(out, thump(T(0.15), 95, 45, 0.02, 0.05), when, 0.6)
    gr = grind(n, r, 120) * swell(n, 0.2, 0.3, 0.1)
    return unit(out) + 0.3 * unit(ffilt(gr, lambda f: lp(f, 2000, 2)))


@sound("stone.buff")
def _(v):
    r, n = v.rng, T(0.6)
    gr = unit(ffilt(grind(n, r, 160), lambda f: lp(f, 2000, 2))) * decay(n, 0.25)
    ck = crackle(n, r, curve(n, [(0, 300), (0.6, 10)]), 300, 2500)
    ru = rumble(n, r, 120) * decay(n, 0.3)
    th = thump(n, 90, 50, 0.03, 0.08)
    return 0.6 * gr + 0.4 * unit(ck) + 0.5 * ru + 0.35 * th


@sound("stone.ult")
def _(v):
    r, n = v.rng, T(1.4)
    out = np.zeros(n)
    place(out, smp("mining", 0, 0.8), 0, 0.8)
    place(out, unit(smp("crunch", 0, 0.75)), 0, 0.6)
    place(out, thump(n, 70, 28, 0.06, 0.4, click=0.3, rng=r), 0, 1.0)
    place(out, unit(ffilt(smp("lowboom", 1), lambda f: lp(f, 300, 2))), 0, 0.6)
    place(out, smp("mining", 2, 0.7), 0.55, 0.6)
    place(out, unit(smp("crunch", 3, 0.7)), 0.55, 0.45)
    place(out, thump(T(0.6), 75, 35, 0.04, 0.2), 0.55, 0.6)
    ru = rumble(n, r, 200, 2.5) * swell(n, 0.5, 0.4, 0.4, 1.0)
    rb = rubble(n, r, curve(n, [(0, 60), (0.3, 250), (1.3, 30)]))
    return unit(out) + 0.6 * ru + 0.35 * unit(rb)


# --- bronze: bells and warm light. Recorded bell and metal strikes; synthesised bells (a
# church bell's partials), warm saw pads and a sung chord (formants).

@sound("bronze.cast")
def _(v):
    r, n = v.rng, T(0.36)
    base = 72 + (2 if v.k else 0)
    notes = [mf(base), mf(base + 4), mf(base + 7)]
    out = np.zeros(n)
    for i, f in enumerate(notes):
        place(out, bell(n, f, r, length=0.3, bright=0.7), 0.07 * i, 0.7 + 0.15 * i)
    pad = unit(ffilt(chord_pad(n, [mf(base - 12), mf(base - 8), mf(base - 5)], r), lambda f: lp(f, 1200, 2)))
    return 0.75 * unit(out) + 0.3 * pad * swell(n, 0.3, 0.2, 0.05)


@sound("bronze.launch")
def _(v):
    r, n = v.rng, T(0.42)
    dur = n / SR
    w = whoosh(n, r, 2600 * v.j(0.1), 700, q=1.0) * adsr(n, 0.008, 0.14, 0, 0.05)
    f0 = curve(n, [(0, 1400 * v.j(0.03)), (dur, 1340)])
    ring = drone(n, f0, [1, 2, 2.51, 3.01], [1, 0.6, 0.35, 0.25], r, beat=0.002) * decay(n, 0.15)
    sh = grains(n, r, curve(n, [(0, 200), (0.3, 0)]), 3000, 8000, 0.02, 0.05)
    return 0.8 * unit(w) + 0.45 * unit(ring) + 0.25 * unit(sh)


@sound("bronze.impact")
def _(v):
    r, n = v.rng, T(0.6)
    b = fit(smp("bell_h", v.pick("bell_h")), n)
    m = fit(smp("metal_h", v.pick("metal_h"), 0.8), n)
    syn = bell(n, 520 * v.j(0.04), r, length=0.55, bright=0.9)
    pm = fit(smp("punch_m", v.pick("punch_m")), n)
    return 0.7 * b + 0.5 * m + 0.6 * unit(syn) + 0.4 * pm


@sound("bronze.blast")
def _(v):
    r, n = v.rng, T(1.0)
    great = ffilt(bell(n, 123 * v.j(0.03), r, length=1.1, bright=1.1), lambda f: hp(f, 85, 2))
    m = fit(smp("metal_h", v.pick("metal_h"), 0.55), n)
    b = fit(smp("bell_h", v.pick("bell_h"), 0.5), n)
    th = thump(n, 100, 60, 0.03, 0.2)
    return 0.9 * unit(great) + 0.5 * m + 0.5 * b + 0.45 * th


@sound("bronze.windup")
def _(v):
    r, n = v.rng, T(0.8)
    dur = n / SR
    e = swell(n, 0.76, start=0.14, fall=0.03, power=1.3)
    glide = curve(n, [(0, 1.0), (dur, 4 / 3)], "exp")
    ch = sum(drone(n, mf(m) * glide, [1, 2, 3.01], [1, 0.4, 0.15], r, beat=0.003) for m in (69, 73, 76))
    ch = unit(ch) * trem(n, curve(n, [(0, 6), (dur, 16)]), 0.4)
    sh = grains(n, r, curve(n, [(0, 40), (dur, 400)]), 2500, 8000, 0.02, 0.06,
                fcurve=glide)
    return e * (0.7 * ch + 0.3 * unit(sh))


@sound("bronze.dash")
def _(v):
    r, n = v.rng, T(0.36)
    w = whoosh(n, r, 900, 1000, q=1.4, mid=2600) * bump(n, 0.08, 0.065)
    ring = at(n, bell(T(0.3), 1760 * v.j(0.03), r, length=0.2, bright=0.7), 0.02)
    hoof = np.zeros(n)
    for i, when in enumerate((0.05, 0.13 * v.j(0.05))):
        place(hoof, smp("wood_m", v.pick("wood_m", i), 0.6), when, 0.8 - 0.2 * i)
        place(hoof, thump(T(0.12), 120, 60, 0.015, 0.04), when, 0.5)
    return 0.75 * unit(w) + 0.35 * unit(ring) + 0.6 * unit(hoof)


@sound("bronze.buff")
def _(v):
    r, n = v.rng, T(0.6)
    t = tt(n)
    f = mf(81 + (2 if v.k else 0))
    b = partials(n, f, [1, 2, 3, 4.2], [1, 0.35, 0.2, 0.1], [1.0, 0.6, 0.4, 0.25], r, attack=0.006)
    b += 0.4 * partials(n, 2 * f * 1.002, [1, 2], [1, 0.3], [0.7, 0.4], r, attack=0.01)
    return unit(b) * trem(n, 5, 0.15) * np.minimum(1, 0.5 + t / 0.02)


@sound("bronze.ult")
def _(v):
    r, n = v.rng, T(1.4)
    dur = n / SR
    t = tt(n)
    deep = ffilt(bell(n, 98, r, length=1.4, bright=1.0), lambda f: hp(f, 80, 2))
    st = fit(smp("metal_h", 0, 0.5), n)
    rise = curve(n, [(0, 1.0), (dur, 1.03)]) * (1 + 0.006 * np.sin(2 * np.pi * 5 * t))
    choir = sum(saw(mf(m) * rise * (1 + 0.004 * d), n, top=8000, rolloff=0.2)
                for m in (55, 59, 62, 67) for d in (-1, 0, 1))
    choir = unit(vowel(choir, "a")) * swell(n, 1.0, 0.08, 0.3, 1.5)
    sh = grains(n, r, curve(n, [(0, 30), (dur, 300)]), 3000, 9000, 0.02, 0.06)
    return 0.8 * unit(deep) + 0.4 * st + 0.55 * choir + 0.2 * unit(sh)


# --- steel: blades, bows and guns. Recorded knives, metal, cloth and steps; synthesised strings
# (Karplus-Strong), cracks, whizzes and rings.

def blade_ring(n, rng, f0, length=0.3):
    return partials(n, f0, [1, 2.71, 5.1, 8.3], [1, 0.5, 0.25, 0.1],
                    [length, length * 0.6, length * 0.35, length * 0.2], rng, beat=0.002)


def crack(n, rng, tau=0.0015):
    x = ffilt(white(n, rng), lambda f: hp(f, 1000, 2) * lp(f, 10000, 2)) * np.exp(-tt(n) / tau)
    return unit(x)


@sound("steel.cast")
def _(v):
    r, n = v.rng, T(0.32)
    kd = fit(smp("knife_draw", v.pick("knife_draw"), v.j(0.04)), n)
    ck = at(n, smp("metal_click", 0, 1.1), 0.2 * v.j(0.08))
    ring = at(n, blade_ring(T(0.15), r, 3200 * v.j(0.05), 0.1), 0.2)
    return 0.8 * kd + 0.5 * ck + 0.2 * unit(ring)


@sound("steel.launch")
def _(v):
    r, n = v.rng, T(0.36)
    dur = n / SR
    f = curve(n, [(0, 3600 * v.j(0.05)), (dur, 1700)], "exp")
    fb = bandcurve(n, f)
    whiz = tvf(white(n, r), lambda tc, q: bp(q, fb(tc), 6)) * (bump(n, 0.05, 0.06) * 0.5 + decay(n, 0.12) * 0.5)
    twang = ks(n, 98 * v.j(0.03), r, 0.35, 0.3)
    snap = crack(n, r, 0.002)
    shot = crack(n, r, 0.004) + 0.8 * thump(n, 160, 60, 0.02, 0.04)
    mix = [(1.0, 0.2), (0.3, 1.0), (0.7, 0.6)][v.k % 3]
    return 0.6 * unit(whiz) + mix[0] * (0.6 * unit(twang) + 0.4 * snap) + mix[1] * 0.7 * unit(shot)


@sound("steel.impact")
def _(v):
    r, n = v.rng, T(0.45)
    ks_ = fit(smp("knife_slice", v.pick("knife_slice"), 1.15 * v.j(0.04)), n)
    pm = fit(smp("punch_m", v.pick("punch_m")), n)
    ring = blade_ring(n, r, 2400 * v.j(0.05), 0.25)
    return 0.7 * ks_ + 0.7 * pm + 0.3 * unit(ring)


@sound("steel.blast")
def _(v):
    r, n = v.rng, T(0.95)
    times = np.concatenate([[0.0], np.sort(r.uniform(0.04, 0.6, 6))])
    out = np.zeros(n)
    for i, when in enumerate(times):
        g = 1.0 - 0.06 * i
        place(out, crack(T(0.05), r, r.uniform(0.001, 0.003)), when, 0.6 * g)
        place(out, thump(T(0.2), r.uniform(110, 150), 50, 0.02, 0.06), when, 0.7 * g)
        if i % 3 == 1:
            place(out, smp("metal_l", v.pick("metal_l", i), 1.2), when + 0.01, 0.25)
    dust = unit(ffilt(pink(n, r), lambda f: lp(f, 1500, 2))) * curve(n, [(0, 0.2), (0.4, 0.6), (0.95, 0)])
    return unit(out) + 0.12 * dust


@sound("steel.windup")
def _(v):
    r, n = v.rng, T(0.8)
    dur = n / SR
    e = swell(n, 0.76, start=0.14, fall=0.03, power=1.3)
    fc = curve(n, [(0, 320), (dur, 1500)], "exp")
    whine = fm(n, fc, 1.41, 1.5) + 0.4 * fm(n, fc * 2, 1.41, 0.8)
    whine = unit(whine) * trem(n, curve(n, [(0, 8), (dur, 20)]), 0.25)
    cb = ffilt(comb(white(n, r), fc, g=0.85, K=10), lambda q: hp(q, 300, 2) * lp(q, 9000, 2))
    return e * (0.6 * sat(whine, 1.3) + 0.35 * unit(cb))


@sound("steel.dash")
def _(v):
    r, n = v.rng, T(0.36)
    cl = fit(smp("cloth", v.pick("cloth"), 1.2), n)
    w = whoosh(n, r, 700, 800, q=1.1, mid=2000) * bump(n, 0.09, 0.07)
    st = at(n, smp("step_rpg", v.pick("step_rpg")), 0.2 * v.j(0.08))
    return 0.55 * cl + 0.75 * unit(w) + 0.6 * st


@sound("steel.buff")
def _(v):
    r, n = v.rng, T(0.6)
    t = tt(n)
    ring = blade_ring(n, r, 1180 * v.j(0.03), 0.5)
    strike = fit(smp("metal_l", v.pick("metal_l")), n)
    f = curve(n, [(0, 118), (0.6, 104)]) * (1 + 0.01 * wob(n, r, 7))
    grunt = vowel(sat(saw(f, n), 2.0), "a") * curve(n, [(0, 0.2), (0.1, 1), (0.25, 0.7), (0.6, 0)])
    return 0.6 * unit(ring) + 0.5 * strike + 0.45 * unit(grunt)


@sound("steel.ult")
def _(v):
    r, n = v.rng, T(1.4)
    out = np.zeros(n)
    place(out, unit(smp("knife_draw", 0, 0.8)), 0, 0.7)
    hit = 0.52
    ring = blade_ring(n, r, 1050, 1.0)
    ring *= np.minimum(1, 0.35 + tt(n) / 0.5)
    place(out, smp("punch_h", 0, 0.85), hit, 1.0)
    place(out, smp("metal_h", 1, 0.75), hit, 0.7)
    place(out, thump(T(0.8), 80, 35, 0.04, 0.25), hit, 0.9)
    place(out, unit(ffilt(smp("crunch", 1, 0.9), lambda f: lp(f, 2000, 2))), hit, 0.4)
    return unit(out) + 0.35 * unit(ring)


# --- the rest of the match: autos, minions, turrets, structures, statuses, spells.

@sound("auto.swing")
def _(v):
    r, n = v.rng, T(0.2)
    w = whoosh(n, r, 700 * v.j(0.1), 900, q=1.5, mid=1900 * v.j(0.1)) * bump(n, 0.06, 0.045)
    return unit(w)


@sound("auto.hit")
def _(v):
    r, n = v.rng, T(0.25)
    pm = fit(smp("punch_m", v.pick("punch_m"), v.j(0.04)), n)
    ml = fit(smp("metal_l", v.pick("metal_l"), 1.1), n)
    th = thump(n, 130, 60, 0.015, 0.05)
    return 0.9 * pm + 0.22 * ml + 0.4 * th


@sound("minion.swing")
def _(v):
    r, n = v.rng, T(0.15)
    return unit(whoosh(n, r, 1000, 1400, q=1.6, mid=2600 * v.j(0.1)) * bump(n, 0.045, 0.035))


@sound("minion.hit")
def _(v):
    r, n = v.rng, T(0.15)
    g = fit(smp("generic_l", v.pick("generic_l"), 1.1), n)
    s = fit(smp("soft_m", v.pick("soft_m"), 1.3), n)
    return 0.7 * g + 0.5 * s


@sound("minion.shot")
def _(v):
    r, n = v.rng, T(0.2)
    f = curve(n, [(0, 1400 * v.j(0.06)), (0.12, 350), (0.2, 300)], "exp")
    z = fm(n, f, 2.01, 1.2) * adsr(n, 0.003, 0.06, 0, 0.03)
    nz = ffilt(white(n, r), lambda q: bp(q, 2500, 1.0)) * decay(n, 0.03)
    return ffilt(0.8 * z + 0.25 * unit(nz), lambda q: lp(q, 5000, 2))


@sound("minion.siege")
def _(v):
    r, n = v.rng, T(0.4)
    th = thump(n, 90, 45, 0.03, 0.12)
    lb = unit(ffilt(fit(smp("lowboom", v.k), n), lambda f: lp(f, 500, 2)))
    cr = unit(ffilt(fit(smp("crunch", v.pick("crunch"), 0.9), n), lambda f: lp(f, 1200, 2)))
    return 0.9 * th + 0.5 * lb * decay(n, 0.15) + 0.4 * cr


@sound("minion.die")
def _(v):
    r, n = v.rng, T(0.3)
    rb = rubble(n, r, curve(n, [(0, 400), (0.2, 0)]), 400, 5000)
    puff = unit(ffilt(pink(n, r), lambda f: lp(f, 1200, 2) * hp(f, 150, 2))) * adsr(n, 0.01, 0.08, 0, 0.05)
    s = fit(smp("soft_m", v.pick("soft_m"), 1.1), n)
    return 0.5 * unit(rb) + 0.6 * puff + 0.6 * s


@sound("turret.shot")
def _(v):
    r, n = v.rng, T(0.5)
    t = tt(n)
    f = curve(n, [(0, 400), (0.06, 2400 * v.j(0.04)), (0.5, 2000)], "exp")
    zap = fm(n, f, 1.5, curve(n, [(0, 3), (0.1, 1), (0.5, 0.5)])) * adsr(n, 0.004, 0.08, 0.15, 0.1)
    w = whoosh(n, r, 3000, 800, q=1.0) * (bump(n, 0.08, 0.05) * 0.4 + decay(n, 0.15, delay=0.06) * (t > 0.04))
    cry = glints(n, r, curve(n, [(0, 300), (0.25, 0)]), 2500, 6000, (0.04, 0.12))
    return 0.55 * zap + 0.8 * unit(w) + 0.3 * unit(cry)


@sound("turret.hit")
def _(v):
    r, n = v.rng, T(0.5)
    ph = fit(smp("punch_h", v.pick("punch_h"), 0.9), n)
    th = thump(n, 100, 45, 0.03, 0.12)
    el = crackle(n, r, curve(n, [(0, 2500), (0.3, 50)]), 2000, 9000) * decay(n, 0.12)
    gl = fit(smp("glass_l", v.pick("glass_l"), 0.9), n)
    return 0.8 * ph + 0.6 * th + 0.4 * unit(el) + 0.3 * gl


@sound("turret.warn")
def _(v):
    r, n = v.rng, T(0.6)
    dur = n / SR
    f = curve(n, [(0, 150), (dur, 240)], "exp")
    tone = ffilt(saw(f, n) + 0.6 * sine(f * 2), lambda q: lp(q, 1500, 2))
    pulse = 0.55 + 0.45 * (np.sin(phase(curve(n, [(0, 10), (dur, 16)]), n)) > -0.3)
    pulse = movavg(pulse, T(0.004))
    ping = at(n, bell(T(0.4), 1760, r, length=0.25, kind=GLASS), 0)
    return unit(tone) * pulse * adsr(n, 0.005, 1.0, 1.0, 0.06) + 0.3 * unit(ping)


@sound("turret.fall")
def _(v):
    r, n = v.rng, T(1.8)
    t = tt(n)
    out = np.zeros(n)
    place(out, smp("mining", 1, 0.8), 0, 0.9)
    place(out, unit(smp("crunch", 2, 0.8)), 0, 0.7)
    place(out, smp("glass_h", 3, 0.7), 0, 0.4)
    place(out, thump(n, 75, 30, 0.05, 0.35, click=0.3, rng=r), 0, 1.0)
    for when, i in ((0.45, 0), (0.8, 3), (1.15, 4)):
        place(out, smp("punch_h", i, 0.6), when, 0.6)
        place(out, smp("mining", i, 0.65), when, 0.35)
    ru = rumble(n, r, 220, 2.2) * curve(n, [(0, 0.5), (0.25, 1.0), (1.2, 0.6), (1.8, 0)])
    roar_ = unit(ffilt(pink(n, r), lambda f: bp(f, 500, 0.6))) * curve(n, [(0, 0.3), (0.3, 1), (1.8, 0)]) ** 1.5
    rb = rubble(n, r, curve(n, [(0, 50), (0.2, 300), (1.5, 20)]))
    dying = sine(curve(n, [(0, 900), (1.2, 300)], "exp")) * np.exp(-t / 0.35) * 0.5
    return unit(out) + 0.55 * ru + 0.4 * roar_ + 0.35 * unit(rb) + 0.15 * dying


@sound("inhibitor.fall")
def _(v):
    r, n = v.rng, T(1.8)
    t = tt(n)
    out = np.zeros(n)
    for i, (when, rate) in enumerate(((0, 0.85), (0.02, 0.7), (0.05, 1.0))):
        place(out, smp("glass_h", i, rate), when, 0.7)
    place(out, thump(n, 85, 32, 0.05, 0.4, click=0.2, rng=r), 0, 1.0)
    place(out, unit(ffilt(smp("lowboom", 0), lambda f: lp(f, 300, 2))), 0, 0.6)
    sh = glints(n, r, curve(n, [(0, 1200), (0.2, 500), (1.6, 10)]), 1200, 6000, (0.15, 0.45))
    ring = drone(n, curve(n, [(0, 660), (1.8, 560)], "exp"), [1, 2.32, 4.25], [1, 0.5, 0.25], r, beat=0.004)
    ring = unit(ring) * np.exp(-t / 0.6)
    fall = stars(n, r, curve(n, [(0, 0), (0.15, 300), (1.6, 0)]), 1500, 6000,
                 fcurve=curve(n, [(0, 1), (1.8, 0.5)], "exp"), d=(0.03, 0.08))
    return unit(out) + 0.5 * unit(sh) + 0.3 * ring + 0.25 * unit(fall)


@sound("nexus.fall")
def _(v):
    r, n = v.rng, T(3.0)
    t = tt(n)
    out = np.zeros(n)
    place(out, sat(thump(n, 70, 28, 0.08, 0.6, click=0.4, rng=r), 1.6), 0, 1.0)
    place(out, unit(ffilt(smp("lowboom", 0), lambda f: lp(f, 400, 2))), 0, 0.8)
    place(out, unit(ffilt(smp("lowboom", 1, 0.9), lambda f: lp(f, 300, 2))), 0.9, 0.6)
    place(out, unit(smp("crunch", 0, 0.7)), 0, 0.6)
    for i, (when, rate) in enumerate(((0, 0.8), (0.01, 0.6), (0.03, 1.0), (0.05, 0.7), (0.9, 0.65), (0.93, 0.85))):
        place(out, smp("glass_h", i, rate), when, 0.6)
    sh = glints(n, r, curve(n, [(0, 1500), (0.3, 600), (0.9, 900), (1.2, 300), (2.8, 5)]), 900, 6500, (0.15, 0.5))
    drone_ = drone(n, curve(n, [(0, 220), (3.0, 104)], "exp"), [1, 2.32, 4.25, 1.5], [1, 0.5, 0.25, 0.5], r, beat=0.005)
    drone_ = unit(drone_) * curve(n, [(0, 0.6), (0.4, 1), (3.0, 0)]) ** 1.2
    ru = rumble(n, r, 180, 2.5) * curve(n, [(0, 0.6), (0.3, 1), (1.5, 0.8), (3.0, 0)])
    roar_ = tvf(pink(n, r), lambda tc, f: bp(f, 1200 * (250 / 1200) ** np.minimum(tc / 2.6, 1), 0.7))
    roar_ = unit(sat(unit(roar_), 1.5)) * curve(n, [(0, 0.4), (0.4, 1), (3.0, 0)]) ** 1.3
    rb = rubble(n, r, curve(n, [(0, 100), (0.3, 500), (2.6, 30)]))
    return unit(out) + 0.45 * unit(sh) + 0.3 * drone_ + 0.6 * ru + 0.45 * roar_ + 0.3 * unit(rb)


@sound("status.stun")
def _(v):
    r, n = v.rng, T(0.6)
    t = tt(n)
    f = 2200 * v.j(0.03) * (1 + 0.03 * np.sin(2 * np.pi * 7 * t))
    ring = fm(n, f, 1.4, 0.8) + 0.5 * fm(n, f * 1.33, 1.4, 0.6)
    ring = unit(ring) * trem(n, 7, 0.5, 1.0) * decay(n, 0.3) * np.minimum(1, t / 0.005)
    tin = fit(smp("tin_m", v.pick("tin_m")), n)
    return 0.6 * ring + 0.6 * tin


@sound("status.root")
def _(v):
    r, n = v.rng, T(0.5)
    cr = fit(smp("creak", v.pick("creak"), 1.2), n) * decay(n, 0.25)
    zipf = curve(n, [(0, 800), (0.12, 3000)], "exp")
    zp = gust(n, r, zipf, 3) * adsr(n, 0.005, 0.08, 0, 0.05)
    kn = fit(smp("wood_l", v.pick("wood_l")), n)
    return 0.7 * unit(cr) + 0.5 * unit(zp) + 0.5 * kn


@sound("status.silence")
def _(v):
    r, n = v.rng, T(0.5)
    hush = unit(ffilt(pink(n, r), lambda f: lp(f, 1200, 2) * hp(f, 200, 2))) * curve(n, [(0, 0.3), (0.12, 1), (0.3, 0.2), (0.5, 0)])
    pt = 0.24
    pop = at(n, thump(T(0.08), 500, 150, 0.005, 0.015), pt)
    click = at(n, unit(ffilt(white(T(0.01), r), lambda f: lp(f, 2500, 2))) * np.exp(-tt(T(0.01)) / 0.002), pt)
    return 0.7 * hush + 0.6 * pop + 0.2 * click


@sound("status.slow")
def _(v):
    r, n = v.rng, T(0.4)
    dur = n / SR
    f = curve(n, [(0, 600), (dur, 180)], "exp")
    tone = ffilt(saw(f, n), lambda q: lp(q, 1500, 2))
    sm = gust(n, r, f * 3, 1.5)
    e = adsr(n, 0.01, 1.0, 1.0, 0.1) * curve(n, [(0, 1), (dur, 0.4)])
    return e * (0.6 * unit(tone) + 0.5 * unit(sm))


@sound("status.airborne")
def _(v):
    r, n = v.rng, T(0.6)
    w = whoosh(n, r, 400, 2500, q=1.2) * swell(n, 0.35, 0.2, 0.25, 1.5)
    land = 0.42 * v.j(0.05)
    th = at(n, smp("soft_h", v.pick("soft_h")), land) + at(n, thump(T(0.2), 110, 50, 0.02, 0.06), land, 0.6)
    return 0.8 * unit(w) + 0.8 * th


@sound("status.shove")
def _(v):
    r, n = v.rng, T(0.4)
    ph = unit(ffilt(fit(smp("punch_h", v.pick("punch_h"), 0.8), n), lambda f: lp(f, 2500, 2)))
    sh = fit(smp("soft_h", v.pick("soft_h")), n)
    sc = at(n, unit(ffilt(grind(T(0.32), r, 200), lambda f: hp(f, 400, 2))) * curve(T(0.32), [(0, 1), (0.32, 0)]), 0.04)
    return 0.8 * ph + 0.5 * sh + 0.4 * sc


@sound("status.reveal")
def _(v):
    r, n = v.rng, T(0.4)
    p = partials(n, mf(96), [1, 2.76, 5.4], [1, 0.3, 0.1], [0.12, 0.06, 0.03], r, attack=0.001)
    g = glints(n, r, 40, 4000, 8000, (0.02, 0.06))
    return unit(p) + 0.2 * unit(g)


@sound("status.immune")
def _(v):
    r, n = v.rng, T(0.6)
    dur = n / SR
    out = np.zeros(n)
    for i, m in enumerate((79, 84, 88, 91)):
        place(out, bell(n, mf(m), r, length=0.3, kind=CHIME, bright=0.8), 0.05 * i, 0.6 + 0.12 * i)
    sh = drone(n, curve(n, [(0, mf(84)), (dur, mf(91))], "exp"), [1, 2], [1, 0.3], r, beat=0.004)
    w = whoosh(n, r, 600, 4000, q=1.2) * swell(n, 0.3, 0.2, 0.25)
    return 0.7 * unit(out) + 0.2 * unit(sh) * swell(n, 0.3, 0.3, 0.25) + 0.35 * unit(w)


@sound("shield.up")
def _(v):
    r, n = v.rng, T(0.5)
    ff = unit(ffilt(fit(smp("forcefield", v.pick("forcefield")), n), lambda f: lp(f, 3000, 2)))
    hm = unit(ffilt(chord_pad(n, [110 * v.j(0.02), 165], r), lambda f: lp(f, 900, 2)))
    e = swell(n, 0.15, 0.3, 0.3, 1.2)
    return (0.5 * ff + 0.6 * hm * trem(n, 9, 0.2)) * e


@sound("heal")
def _(v):
    r, n = v.rng, T(0.5)
    t = tt(n)
    base = (79, 84, 88) if v.k == 0 else (81, 86, 89)
    ch = chimes(n, r, [mf(m) for m in base], [0, 0.07, 0.14], length=0.3, bright=0.6) * np.minimum(1, t / 0.006)
    br = unit(ffilt(pink(n, r), lambda f: hp(f, 2500, 2) * lp(f, 9000, 2))) * adsr(n, 0.08, 0.15, 0, 0.1)
    return 0.7 * unit(ch) + 0.2 * br


@sound("empowered")
def _(v):
    r, n = v.rng, T(0.4)
    dur = n / SR
    pad = chord_pad(n, [mf(45), mf(52)], r, voices=3)
    pad = tvf(pad, lambda tc, f: lp(f, 300 * 7 ** np.minimum(tc / 0.25, 1), 2))
    th = thump(n, 120, 60, 0.03, 0.08)
    return 0.8 * unit(pad) * swell(n, 0.2, 0.3, 0.18, 1.3) + 0.35 * th


@sound("hasted")
def _(v):
    r, n = v.rng, T(0.35)
    w = whoosh(n, r, 600, 3500, q=1.2) * swell(n, 0.2, 0.25, 0.15, 1.3)
    ch = at(n, bell(T(0.25), mf(93), r, length=0.15, kind=CHIME, bright=0.6), 0.15)
    return 0.85 * unit(w) + 0.2 * unit(ch)


@sound("blink")
def _(v):
    r, n = v.rng, T(0.3)
    f = curve(n, [(0, 300), (0.08, 3000 * v.j(0.05)), (0.3, 3000)], "exp")
    zip_ = fm(n, f, 1.5, 1.0) * curve(n, [(0, 0.4), (0.07, 1), (0.085, 0), (0.3, 0)])
    pop = at(n, thump(T(0.08), 900, 300, 0.006, 0.015, click=0.4, rng=r), 0.085)
    sp = at(n, stars(T(0.2), r, 300, 3000, 9000), 0.09)
    nz = gust(n, r, f, 2.5) * curve(n, [(0, 0.3), (0.08, 1), (0.09, 0), (0.3, 0)])
    return 0.45 * zip_ + 0.4 * unit(nz) + 0.7 * pop + 0.25 * unit(sp)


@sound("tag.throw")
def _(v):
    r, n = v.rng, T(0.3)
    return unit(whoosh(n, r, 600, 900, q=1.0, mid=1800, color=1.3) * bump(n, 0.1, 0.07))


@sound("tag.hit")
def _(v):
    r, n = v.rng, T(0.3)
    sn = fit(smp("step_snow", v.pick("step_snow"), 1.1), n)
    sm = fit(smp("soft_m", v.pick("soft_m")), n)
    return 0.8 * sn + 0.6 * sm


@sound("tag.dash")
def _(v):
    r, n = v.rng, T(0.4)
    return unit(whoosh(n, r, 500, 800, q=1.0, mid=2200) * bump(n, 0.12, 0.09))


@sound("death.champion")
def _(v):
    r, n = v.rng, T(1.0)
    t = tt(n)
    out = np.zeros(n)
    place(out, smp("soft_h", v.pick("soft_h"), 0.8), 0, 1.0)
    place(out, unit(ffilt(smp("punch_h", v.pick("punch_h"), 0.7), lambda f: lp(f, 2000, 2))), 0, 0.7)
    place(out, smp("soft_m", v.pick("soft_m"), 0.85), 0.13 * v.j(0.1), 0.45)
    pad = unit(ffilt(chord_pad(n, [55, 65.4, 82.4], r), lambda f: lp(f, 600, 2) * hp(f, 40, 2)))
    pad *= curve(n, [(0, 0.1), (0.25, 0.8), (0.6, 0.6), (1.0, 0)])
    return unit(out) + 0.4 * pad


@sound("death.own")
def _(v):
    r, n = v.rng, T(1.6)
    out = np.zeros(n)
    place(out, unit(ffilt(smp("punch_h", 1, 0.75), lambda f: lp(f, 800, 2))), 0, 1.0)
    place(out, unit(ffilt(smp("soft_h", 2, 0.7), lambda f: lp(f, 900, 2))), 0, 0.7)
    place(out, thump(n, 60, 32, 0.05, 0.3), 0, 0.9)
    dr = chord_pad(n, [55, 55 * 1.19, 82.4], r, detune=0.01)
    dr = unit(ffilt(dr, lambda f: lp(f, 320, 2) * hp(f, 40, 2))) * curve(n, [(0, 0.2), (0.3, 0.8), (1.0, 0.6), (1.6, 0)])
    return unit(out) + 0.55 * dr


@sound("respawn")
def _(v):
    r, n = v.rng, T(0.8)
    w = whoosh(n, r, 300, 3000, q=1.0) * swell(n, 0.45, 0.2, 0.3, 1.5)
    ch = at(n, bell(T(0.5), mf(76), r, length=0.35, kind=CHIME, bright=0.7), 0.42)
    ch2 = at(n, bell(T(0.4), mf(83), r, length=0.3, kind=CHIME, bright=0.6), 0.47)
    sh = grains(n, r, curve(n, [(0, 20), (0.45, 200), (0.8, 0)]), 3000, 8000, 0.02, 0.06)
    return 0.7 * unit(w) + 0.5 * unit(ch) + 0.3 * unit(ch2) + 0.2 * unit(sh)


@sound("level.up")
def _(v):
    r, n = v.rng, T(0.8)
    notes = (72, 76, 79, 84, 88)
    out = np.zeros(n)
    for i, m in enumerate(notes):
        L = n - T(0.065 * i)
        x = fm(L, mf(m), 2.0, curve(L, [(0, 2.0), (0.3, 0.3)])) * np.exp(-tt(L) / 0.25)
        x += 0.3 * sine(mf(m) * 2, L) * np.exp(-tt(L) / 0.15)
        x[:T(0.002)] *= np.linspace(0, 1, T(0.002))
        place(out, x, 0.065 * i, 0.6 + 0.1 * i)
    sh = grains(n, r, curve(n, [(0, 50), (0.3, 300), (0.8, 0)]), 4000, 9000, 0.02, 0.05)
    return unit(out) + 0.15 * unit(sh)


@sound("relic.take")
def _(v):
    r, n = v.rng, T(0.7)
    bb = bubbles(n, r, curve(n, [(0, 600), (0.4, 300), (0.7, 0)]), 800, 2500, rise=2.0)
    ch = chimes(n, r, [mf(84), mf(91)], [0, 0.1], length=0.35, bright=0.6)
    br = unit(ffilt(pink(n, r), lambda f: hp(f, 2000, 2) * lp(f, 9000, 2))) * adsr(n, 0.05, 0.2, 0, 0.1)
    return 0.6 * unit(bb) + 0.55 * unit(ch) + 0.2 * br


@sound("relic.beam")
def _(v):
    r, n = v.rng, T(1.0)
    dur = n / SR
    f = curve(n, [(0, 110), (dur, 220)], "exp")
    hm = sum(saw(f * d, n, top=6000) for d in (0.995, 1.0, 1.006))
    hm = tvf(hm, lambda tc, q: lp(q, 400 * 6 ** (tc / dur), 2))
    hm = unit(hm) * trem(n, curve(n, [(0, 4), (dur, 16)]), 0.4)
    wh = sine(f * 8) * curve(n, [(0, 0), (dur, 1)]) ** 2
    return swell(n, 0.9, 0.2, 0.05, 1.3) * (0.8 * hm + 0.2 * wh)


@sound("augment.proc")
def _(v):
    r, n = v.rng, T(0.25)
    p = partials(n, 2600 * v.j(0.05), GLASS[0], GLASS[1], [0.05, 0.03, 0.02, 0.015, 0.01], r, attack=0.0005)
    ck = fit(smp("ui_tick", v.pick("ui_tick")), n)
    sp = grains(n, r, 60, 5000, 9000, 0.015, 0.04)
    return 0.8 * unit(p) + 0.3 * ck + 0.2 * unit(sp)


# --- the interface: dry, short, clean.

def tone(n, f, kind="bell", tau=0.3, attack=0.002, rise=0.04):
    t = tt(n)
    if kind == "bell":
        x = fm(n, f, 3.5, 1.2 * np.exp(-t / (tau * 0.4))) * np.exp(-t / tau)
        x += 0.25 * np.sin(2 * np.pi * f * 2 * t) * np.exp(-t / (tau * 0.5))
    elif kind == "pluck":
        x = sum(np.sin(2 * np.pi * f * h * t) / h * np.exp(-t * h / tau) for h in (1, 2, 3, 4))
    elif kind == "square":
        x = sum(np.sin(2 * np.pi * f * h * t) / h for h in (1, 3, 5, 7, 9) if f * h < 9000)
        x = x * np.exp(-t / tau)
    elif kind == "brass":
        a = np.minimum(1, 0.15 + t / rise)
        x = saw(f * (1 + 0.004 * np.sin(2 * np.pi * 5.5 * t)), n, top=7000)
        x = tvf(x, lambda tc, q: lp(q, 300 + 2200 * np.minimum(tc / 0.06, 1) * np.exp(-tc / 0.6), 2)) * a
    x = np.asarray(x, dtype=np.float64)
    a = T(attack)
    x[:a] *= np.linspace(0, 1, a)
    return x


def notes_seq(n, seq, kind="bell", tau=0.3, gains=None):
    out = np.zeros(n)
    for i, (f, when) in enumerate(seq):
        L = n - T(when)
        if L > 0:
            place(out, tone(L, f, kind, tau), when, 1.0 if gains is None else gains[i])
    return out


@sound("ui.click")
def _(v):
    n = T(0.07)
    c = fit(smp("ui_click", 1), n)
    return ffilt(c, lambda f: lp(f, 7000, 2) * hp(f, 200, 2))


def coin(n, rng, f0, tau=0.12):
    return partials(n, f0, [1, 1.48, 2.32, 2.95, 3.6], [1, 0.7, 0.5, 0.35, 0.2],
                    [tau, tau * 0.8, tau * 0.6, tau * 0.5, tau * 0.4], rng, attack=0.0005, beat=0.003)


@sound("ui.buy")
def _(v):
    r, n = v.rng, T(0.33)
    c1 = coin(n, r, 2900, 0.12)
    c2 = at(n, coin(n, r, 3550, 0.09), 0.045)
    ch = fit(smp("chips", 0, 1.2), n)
    return unit(c1) + 0.6 * unit(c2) + 0.35 * ch


@sound("ui.sell")
def _(v):
    r, n = v.rng, T(0.42)
    out = np.zeros(n)
    for i, (when, f) in enumerate(((0.0, 2500), (0.12, 2300))):
        place(out, coin(T(0.3), r, f, 0.1), when, 1.0 - 0.2 * i)
        place(out, smp("chip_lay", i, 1.1), when, 0.5)
    return out


@sound("ui.level")
def _(v):
    n = T(0.22)
    return notes_seq(n, [(mf(84), 0), (mf(91), 0.05)], "pluck", 0.08, [0.7, 1.0])


@sound("ui.card")
def _(v):
    n = T(0.58)
    out = np.zeros(n)
    for i, when in enumerate((0.0, 0.13, 0.26)):
        place(out, fit(smp("card_slide", i + 1, 1.15), T(0.2)) * decay(T(0.2), 0.07), when, 1.0 - 0.08 * i)
        place(out, smp("card_place", i, 1.1), when + 0.03, 0.35)
    return out


@sound("ui.augment")
def _(v):
    r, n = v.rng, T(0.68)
    t = tt(n)
    ch = notes_seq(n, [(mf(88), 0), (mf(95), 0.04), (mf(100), 0.08)], "bell", 0.3, [1, 0.8, 0.5])
    g = glints(n, r, curve(n, [(0, 200), (0.5, 0)]), 4000, 8000, (0.03, 0.08))
    sw = unit(ffilt(pink(n, r), lambda f: hp(f, 3000, 2) * lp(f, 9000, 2))) * bump(n, 0.08, 0.06)
    return unit(ch) * trem(n, 9, 0.2) + 0.25 * unit(g) + 0.12 * sw


@sound("ui.reroll")
def _(v):
    n = T(0.42)
    sh = smp("card_shuffle", 0)
    x = sh[T(0.4):]
    x = fit(x[first_above(x[:T(0.3)], 0.2):], n)
    return unit(x) * curve(n, [(0, 1), (0.3, 0.8), (0.42, 0)])


@sound("ui.error")
def _(v):
    n = T(0.26)
    out = np.zeros(n)
    for when in (0.0, 0.13):
        L = T(0.095)
        b = tone(L, 110, "square", 1.0) + 0.5 * tone(L, 116.5, "square", 1.0)
        b *= np.minimum(1, np.linspace(0, 1, L) * 0 + 1) * np.minimum(1, (L - np.arange(L)) / T(0.01))
        place(out, ffilt(b, lambda f: lp(f, 1600, 2)), when)
    return out


@sound("ui.found")
def _(v):
    n = T(0.85)
    seq = [(mf(79), 0), (mf(83), 0.08), (mf(86), 0.16), (mf(91), 0.24)]
    x = notes_seq(n, seq, "bell", 0.35, [0.6, 0.7, 0.85, 1.0])
    sh = grains(n, v.rng, curve(n, [(0, 0), (0.25, 200), (0.85, 0)]), 4000, 9000, 0.02, 0.05)
    return unit(x) + 0.12 * unit(sh)


@sound("ui.ready")
def _(v):
    n = T(0.5)
    x = tone(n, mf(84), "bell", 0.22) + 0.8 * tone(n, mf(91), "bell", 0.22)
    body = thump(n, 220, 140, 0.01, 0.04)
    return unit(x) + 0.35 * body


@sound("ui.tick")
def _(v):
    r, n = v.rng, T(0.05)
    x = fit(smp("ui_tick", 0), n)
    k = knock(n, r, 1800, 0.012)
    return unit(x) + 0.4 * unit(k)


@sound("ui.vote")
def _(v):
    n = T(0.66)
    out = np.zeros(n)
    for when, m in ((0.0, 67), (0.28, 72)):
        L = T(0.3)
        x = tone(L, mf(m), "brass") + 0.5 * tone(L, mf(m - 12), "brass")
        x *= np.minimum(1, (L - np.arange(L)) / T(0.06))
        place(out, x, when)
    return out


@sound("ping.look")
def _(v):
    n = T(0.5)
    return tone(n, mf(88), "bell", 0.18) + 0.3 * tone(n, mf(100), "bell", 0.1)


@sound("ping.danger")
def _(v):
    n = T(0.45)
    out = np.zeros(n)
    for when, m in ((0.0, 88), (0.12, 81), (0.24, 88)):
        L = T(0.11)
        x = tone(L, mf(m), "square", 0.3) * np.minimum(1, (L - np.arange(L)) / T(0.008))
        place(out, x, when, 1.0 if when < 0.2 else 0.7)
    return ffilt(out, lambda f: lp(f, 6000, 2))


@sound("ping.coming")
def _(v):
    n = T(0.42)
    seq = [(mf(m), 0.045 * i) for i, m in enumerate((72, 74, 76, 79, 84))]
    return notes_seq(n, seq, "pluck", 0.12, [0.6, 0.65, 0.7, 0.8, 1.0])


@sound("ping.gone")
def _(v):
    n = T(0.55)
    out = notes_seq(n, [(mf(81), 0)], "bell", 0.15)
    L = n - T(0.17)
    t = tt(L)
    f = mf(76) * curve(L, [(0, 1.0), (0.15, 1.0), (0.3, 1.06)])
    x = (np.sin(phase(f)) + 0.3 * np.sin(phase(2 * f))) * np.exp(-t / 0.2) * np.minimum(1, t / 0.003)
    return place(out, x, 0.17)


@sound("ping.attack")
def _(v):
    n = T(0.6)
    x = sum(g * tone(n, mf(m), "brass", rise=0.02) for m, g in ((45, 1.0), (52, 0.7), (57, 0.4)))
    return sat(unit(x), 1.3) * curve(n, [(0, 1), (0.4, 0.8), (0.6, 0)])


@sound("ping.back")
def _(v):
    n = T(0.6)
    out = np.zeros(n)
    for i, (when, m) in enumerate(((0.0, 76), (0.12, 72), (0.24, 69))):
        L = T(0.2 if i < 2 else 0.33)
        x = tone(L, mf(m), "square", 0.12 if i < 2 else 0.2)
        place(out, x, when)
    return ffilt(out, lambda f: lp(f, 5000, 2))


# --- the ambience: loops made periodic from the start (circular noise, periodic modulations,
# circular filtering and reverb), so the end runs into the start with no seam at all.

def cnoise(n, rng):
    return rng.standard_normal(n)


def cfilt(x, shape):
    f = np.fft.rfftfreq(len(x), 1 / SR)
    return np.fft.irfft(np.fft.rfft(x) * shape(f), len(x))


def cwander(n, rng, fmax, lo=None):
    """A smooth periodic wander in [-1, 1]: random phases on the bins under `fmax`."""
    k = np.arange(n // 2 + 1)
    f = k * SR / n
    spec = (rng.standard_normal(len(k)) + 1j * rng.standard_normal(len(k))) * (f <= fmax) * (f > 0)
    if lo is not None:
        spec *= f >= lo
    x = np.fft.irfft(spec, n)
    return x / max(1e-12, np.max(np.abs(x)))


def cconv(x, ir):
    n = len(x)
    h = np.zeros(n)
    h[:len(ir)] = ir
    return np.fft.irfft(np.fft.rfft(x) * np.fft.rfft(h), n)


@sound("amb.wind")
def _(v):
    r = v.rng
    n = T(24.0)
    # Bands from low rumble to the high hiss of blown snow, each swelling on its own.
    bands = [(90, 0.7, 0.8), (180, 0.8, 0.9), (330, 1.0, 1.0), (560, 1.2, 0.8),
             (900, 1.5, 0.55), (1500, 1.5, 0.35), (2600, 1.2, 0.18), (5000, 0.9, 0.08)]
    gusts = [0.5 + 0.5 * cwander(n, r, 0.25) for _ in range(2)]  # the two sides' gusts
    lr = []
    for side in range(2):
        out = np.zeros(n)
        for i, (fc, q, g) in enumerate(bands):
            b = cfilt(cnoise(n, r), lambda f: bp(f, fc, q))
            b /= rms(b)
            local = 0.5 + 0.5 * cwander(n, r, 0.18 + 0.03 * i)
            shared = gusts[side] if i >= 2 else 0.6
            out += g * b * (0.25 + 0.75 * local * shared) ** 1.6
        # Whistles round the stone, coming and going.
        for fw in (640, 910, 1270):
            fwv = fw * (1 + 0.02 * side)
            w = cfilt(cnoise(n, r), lambda f: bp(f, fwv, 40))
            w /= rms(w)
            amp = np.clip(cwander(n, r, 0.12), 0, 1) ** 2
            out += 0.22 * w * amp * gusts[side]
        lr.append(out)
    # Some of each side in the other, for a wide but whole field.
    L, R = lr
    L, R = L + 0.35 * R, R + 0.35 * L
    irs = make_room(np.random.default_rng(11), n=1.2, rt=1.1, stereo=True)
    L = L + 0.25 * cconv(L, irs[0])
    R = R + 0.25 * cconv(R, irs[1])
    return np.stack([L, R], axis=1)


@sound("amb.fire")
def _(v):
    r = v.rng
    n = T(8.0)
    flick = 0.6 + 0.4 * cwander(n, r, 6.0)
    breathe = 0.7 + 0.3 * cwander(n, r, 0.5)
    rr = cfilt(cnoise(n, r), lambda f: hp(f, 60, 2) * lp(f, 500, 2) / np.maximum(f, 30) ** 0.5)
    rr = rr / rms(rr) * flick * breathe
    hs = cfilt(cnoise(n, r), lambda f: hp(f, 2500, 2) * lp(f, 8000, 2))
    hs = hs / rms(hs) * (0.5 + 0.5 * cwander(n, r, 2.0)) ** 2
    # Crackle, circular: pops of a few kinds, convolved round the loop; and a few big snaps.
    pops = np.zeros(n)
    for kind in range(5):
        vv = impulses(n, r, 9 * (0.6 + 0.4 * breathe))
        L = T(r.uniform(0.0015, 0.006))
        ker = white(L, r) * np.exp(-tt(L) / r.uniform(0.0004, 0.002))
        fc = math.exp(r.uniform(math.log(1200), math.log(5000)))
        ker = ffilt(ker, lambda f: bp(f, fc, 0.8), pad=0.01)
        pops += cconv(vv, ker)
    snaps = np.zeros(n)
    for i in r.choice(n, 7, replace=False):
        L = T(0.03)
        s = white(L, r) * np.exp(-tt(L) / 0.006)
        s = ffilt(s, lambda f: bp(f, 1800, 0.6), pad=0.02)
        snaps += np.roll(np.concatenate([s, np.zeros(n - L)]), i) * r.uniform(1.0, 2.0)
    crack = cfilt(pops + snaps, lambda f: hp(f, 700, 2) * lp(f, 9000, 2))
    crack /= max(1e-9, rms(crack))
    crack = 5 * np.tanh(crack / 5)
    out = 1.0 * rr + 0.12 * hs + 0.5 * crack
    ir = make_room(np.random.default_rng(12), n=0.5, rt=0.4)[0]
    return out + 0.15 * cconv(out, ir)


# ---------------------------------------------------------------------------------------------
# Mastering: the band kept, the onset at the start, the tail cut, faded, levelled by bus.

TARGET = {"effects": -18.0, "ui": -20.0, "ambience": -26.0}
CEIL = -1.0  # the rule, measured on the decoded files
MASTER_CEIL = -1.7  # what the master aims under (the encoders overshoot a little)
RMS_TOL = 2.5  # dB either side of the bus's target
MAX_LIMIT = 9.0  # dB of peak taken off at most; past it, the level gives instead
WET = {"effects": 0.17, "ui": 0.0, "ambience": 0.0}

# The interface's lines don't say how long they are: these are their limits.
UI_LEN = {"ui.click": 0.08, "ui.buy": 0.35, "ui.sell": 0.45, "ui.level": 0.25, "ui.card": 0.6,
          "ui.augment": 0.7, "ui.reroll": 0.45, "ui.error": 0.3, "ui.found": 0.9,
          "ui.ready": 0.55, "ui.tick": 0.06, "ui.vote": 0.7, "ping.look": 0.55,
          "ping.danger": 0.5, "ping.coming": 0.45, "ping.gone": 0.6, "ping.attack": 0.65,
          "ping.back": 0.65}


def stated_len(d):
    m = re.search(r"([0-9.]+) s\b", d["what"])
    return float(m.group(1)) if m else UI_LEN[d["name"]]


def chans(x):
    return x if x.ndim == 2 else x[:, None]


def oversample(x, k=4):
    x = chans(x)
    n = len(x)
    m = 1 << int(math.ceil(math.log2(n + 256)))
    X = np.fft.rfft(x, m, axis=0)
    Y = np.zeros((m * k // 2 + 1, x.shape[1]), dtype=complex)
    Y[:X.shape[0]] = X
    return np.fft.irfft(Y, m * k, axis=0)[:n * k] * k


def true_peak_db(x):
    p = float(np.max(np.abs(oversample(x))))
    return 20 * math.log10(max(p, 1e-12))


def active_rms_db(x):
    """RMS over the 10 ms blocks louder than -40 dBFS."""
    x = chans(x)
    B = T(0.01)
    m = len(x) // B
    if m == 0:
        return -120.0
    p = np.mean(x[:m * B].reshape(m, B, -1) ** 2, axis=(1, 2))
    act = p > db(-40) ** 2
    if not act.any():
        return -120.0
    return 10 * math.log10(float(np.mean(p[act])))


def onset_ms(x):
    """When the sound starts: its first sample within 30 dB of its peak."""
    return 1000.0 * first_above(chans(x)[:, 0] if x.ndim == 1 else x, db(-30)) / SR


def movmin(x, w):
    """min(x[i : i + w]) for each i (van Herk / Gil-Werman)."""
    n = len(x)
    m = -(-(n + w) // w) * w
    xp = np.full(m, np.inf)
    xp[:n] = x
    b = xp.reshape(-1, w)
    pre = np.minimum.accumulate(b, axis=1).ravel()
    suf = np.minimum.accumulate(b[:, ::-1], axis=1)[:, ::-1].ravel()
    i = np.arange(n)
    j = np.minimum(i + w - 1, m - 1)
    return np.minimum(suf[i], pre[j])


def movavg(x, w):
    c = np.concatenate([[0.0], np.cumsum(x)])
    i = np.arange(len(x))
    lo = np.maximum(i - w + 1, 0)
    return (c[i + 1] - c[lo]) / (i + 1 - lo)


def limit(x, ceil_db, look=0.003, hold=0.03):
    """A look-ahead peak limiter: a gain that is never over what the peaks need, ramped over
    `look` and held for `hold`."""
    x2 = chans(x)
    a = np.max(np.abs(x2), axis=1)
    c = db(ceil_db)
    need = np.minimum(1.0, c / np.maximum(a, 1e-12))
    if need.min() >= 1.0:
        return x
    L, Hd = T(look), T(hold)
    # The minimum over [i - hold, i + look], then averaged over [i - look, i]: never over need.
    g = movmin(np.concatenate([np.ones(Hd), need]), Hd + L + 1)[:len(need)]
    g = movavg(g, L)
    y = x2 * g[:, None]
    return y if x.ndim == 2 else y[:, 0]


def master(x, d, ceil_db=MASTER_CEIL):
    bus = d["bus"]
    looped = d["loop"]
    target = TARGET[bus]
    x = np.asarray(x, dtype=np.float64)
    band = lambda f: hp(f, 35, 2) * lp(f, 14000, 2)
    if looped:
        # Circular all through, so the seam stays a seam.
        x2 = chans(x)
        X = np.fft.rfft(x2, axis=0)
        f = np.fft.rfftfreq(len(x2), 1 / SR)
        x = np.fft.irfft(X * band(f)[:, None], len(x2), axis=0)
        if x.shape[1] == 1:
            x = x[:, 0]
        x = x * db(target - active_rms_db(x))
        if true_peak_db(x) > ceil_db:
            # Limited round the loop: the limiter sees the end before the start and the start
            # after the end, so its gain is continuous across the seam too.
            w = T(0.1)
            ext = np.concatenate([x[-w:], x, x[:w]])
            x = limit(ext, ceil_db - 0.3)[w:w + len(x)]
            tp = true_peak_db(x)
            if tp > ceil_db:
                x = x * db(ceil_db - tp)
        return x
    x = ffilt(x, band) if x.ndim == 1 else np.stack([ffilt(c, band) for c in x.T], axis=1)
    mono = np.max(np.abs(chans(x)), axis=1)
    pk = mono.max()
    s = max(0, first_above(mono, db(-40)) - T(0.0005))
    x = x[s:]
    mono = mono[s:]
    # The tail: where a 10 ms envelope falls 55 dB under the peak, and never past 1.4 times the
    # stated length.
    env = movavg(mono, T(0.01))
    alive = np.nonzero(env > pk * db(-55))[0]
    end = min(len(x), (alive[-1] + T(0.01)) if len(alive) else len(x))
    end = min(end, T(stated_len(d) * 1.4))
    x = x[:end].copy()
    fo = min(T(0.04 if stated_len(d) > 0.2 else 0.012), len(x) // 3)
    fi = T(0.002)
    ramp_in = np.linspace(0, 1, fi)
    ramp_out = np.linspace(1, 0, fo) ** 1.5
    if x.ndim == 1:
        x[:fi] *= ramp_in
        x[len(x) - fo:] *= ramp_out
    else:
        x[:fi] *= ramp_in[:, None]
        x[len(x) - fo:] *= ramp_out[:, None]
    # The level: the target RMS over the active part, the peaks limited, but never by more
    # than MAX_LIMIT.
    tp0 = true_peak_db(x)
    g = target - active_rms_db(x)
    for _ in range(8):
        g = min(g, ceil_db + MAX_LIMIT - tp0)
        y = limit(x * db(g), ceil_db - 0.3)
        r = active_rms_db(y)
        if abs(r - target) < 0.1 or g >= ceil_db + MAX_LIMIT - tp0 - 1e-6:
            break
        g += target - r
    y = limit(x * db(g), ceil_db - 0.3)
    tp = true_peak_db(y)
    if tp > ceil_db:
        y = y * db(ceil_db - tp)
    return y


# ---------------------------------------------------------------------------------------------
# Encoding, and the check on what was written.


def ffmpeg_encode(y, path, stereo, fmt):
    c = 2 if stereo else 1
    codec = (["-c:a", "libopus", "-b:a", "96k" if stereo else "64k", "-vbr", "on",
              "-compression_level", "10", "-application", "audio"] if fmt == "ogg"
             else ["-c:a", "libmp3lame", "-q:a", "5", "-id3v2_version", "0"])
    data = np.ascontiguousarray(chans(y), dtype="<f4").tobytes()
    tmp = path + ".part"
    subprocess.run(
        ["ffmpeg", "-v", "error", "-nostdin", "-y", "-f", "f32le", "-ar", str(SR), "-ac", str(c),
         "-i", "-", *codec, "-ar", str(SR), "-map_metadata", "-1", "-fflags", "+bitexact",
         "-flags:a", "+bitexact", "-f", "ogg" if fmt == "ogg" else "mp3", tmp],
        input=data, check=True)
    os.replace(tmp, path)


def probe(path):
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "stream=codec_name,channels,sample_rate",
         "-of", "json", path], check=True, stdout=subprocess.PIPE).stdout
    s = json.loads(out)["streams"][0]
    return s["codec_name"], int(s["channels"]), int(s["sample_rate"])


def measure(path, stereo):
    x = decode(path, 2 if stereo else 1)
    x = x if stereo else x[:, 0]
    return x, {
        "dur": len(chans(x)) / SR,
        "peak": true_peak_db(x),
        "rms": active_rms_db(x),
        "onset": onset_ms(x),
    }


def write_take(y, stem, d):
    """Both formats, each re-encoded quieter until its decoded true peak is under CEIL."""
    stereo = d["stereo"]
    rows = []
    decoded = None
    for fmt in ("ogg", "mp3"):
        path = os.path.join(AUDIO, f"{stem}.{fmt}")
        g = 0.0
        for _ in range(5):
            ffmpeg_encode(y * db(g), path, stereo, fmt)
            x, m = measure(path, stereo)
            if m["peak"] <= CEIL - 0.05:
                break
            g -= m["peak"] - CEIL + 0.15
        m.update(file=f"{stem}.{fmt}", size=os.path.getsize(path), probe=probe(path))
        rows.append(m)
        if fmt == "ogg":
            decoded = x
    return rows, decoded


# ---------------------------------------------------------------------------------------------
# Spectrograms (for looking at, not shipped).

SPEC_ROWS = 180
SPEC_HOP = T(0.0025)


def spectrogram(x):
    x = chans(x).mean(axis=1)
    N = 2048
    xp = np.concatenate([x, np.zeros(N)])
    nfr = max(1, (len(xp) - N) // SPEC_HOP + 1)
    idx = np.arange(N)[None, :] + SPEC_HOP * np.arange(nfr)[:, None]
    w = np.hanning(N)
    S = np.abs(np.fft.rfft(xp[idx] * w, axis=1)) / (N / 4)
    f = np.fft.rfftfreq(N, 1 / SR)
    rows = np.geomspace(30, 20000, SPEC_ROWS)
    S = np.stack([np.interp(rows, f, s) for s in S], axis=1)
    L = 20 * np.log10(np.maximum(S, 1e-9))
    img = np.clip((L - L.max() + 65) / 65, 0, 1)
    return (img[::-1] * 255).astype(np.uint8)


def colour(img):
    stops = np.array([[0, 0, 4], [40, 11, 84], [101, 21, 110], [159, 42, 99], [212, 72, 66],
                      [245, 125, 21], [250, 193, 39], [252, 255, 164]], dtype=np.float64)
    pos = np.linspace(0, 255, len(stops))
    v = img.astype(np.float64)
    return np.stack([np.interp(v, pos, stops[:, c]) for c in range(3)], axis=-1).astype(np.uint8)


def sheet(title, rows, path):
    """rows: [(label, [(stem, spectrogram)])]"""
    from PIL import Image, ImageDraw
    pad, lab = 8, 14
    maxw = 1400
    tiles = []
    for label, takes in rows:
        line = []
        for stem, img in takes:
            if img.shape[1] > maxw:  # the loops: squeezed to fit
                step = img.shape[1] / maxw
                img = img[:, (np.arange(maxw) * step).astype(int)]
            line.append((stem, img))
        tiles.append(line)
    W = max(sum(i.shape[1] + pad for _, i in line) for line in tiles) + pad
    H = pad + 22 + sum(SPEC_ROWS + lab + pad for _ in tiles)
    im = Image.new("RGB", (max(W, 400), H), (18, 18, 24))
    dr = ImageDraw.Draw(im)
    dr.text((pad, pad), f"{title}  (30 Hz to 20 kHz, log; 2.5 ms a pixel; 65 dB under each take's loudest bin)",
            fill=(230, 230, 230))
    y = pad + 22
    for line in tiles:
        x = pad
        for stem, img in line:
            dr.text((x, y), stem, fill=(200, 200, 210))
            im.paste(Image.fromarray(colour(img)), (x, y + lab))
            x += img.shape[1] + pad
        y += SPEC_ROWS + lab + pad
    im.save(path, optimize=True)


# ---------------------------------------------------------------------------------------------
# The run.


class Take:
    def __init__(self, d, k):
        self.d = d
        self.name = d["name"]
        self.k = k
        self.takes = d["takes"]
        self.stem = d["files"][k]
        self.L = stated_len(d)
        self.rng = np.random.default_rng(zlib.crc32(self.stem.encode()))
        self.wet = WET[d["bus"]]

    def j(self, amount):
        """A factor near 1, this take's."""
        return 1 + self.rng.uniform(-amount, amount)

    def pick(self, name, offset=0):
        """A recording of set `name` that each take of a sound has its own of."""
        return (self.k + offset + zlib.crc32(self.name.encode())) % SRC.count(name)


# Each take a few percent off the first, in pitch and so in time too.
CENTS = [0, -40, 45, -20, 25]


def render(d, k):
    v = Take(d, k)
    fn = DESIGNS.get(d["name"])
    if fn is None:
        raise SystemExit(f"no design for {d['name']}")
    x = fn(v)
    if not d["loop"]:
        x = endfade(x, 0.012)
    if d["takes"] > 1 and not d["loop"]:
        c = CENTS[k % len(CENTS)] + v.rng.uniform(-8, 8)
        x = speed(x, 2 ** (c / 1200))
    if not d["loop"] and v.wet > 0:
        x = room(x, v.wet)
    return master(x, d)


def job(args):
    d, k = args
    y = render(d, k)
    rows, decoded = write_take(y, d["files"][k], d)
    return d["name"], k, rows, spectrogram(decoded)


def main():
    global SRC, ROOM
    ap = argparse.ArgumentParser()
    ap.add_argument("--cache", default=os.path.join(ROOT, "art", "cache", "sounds"))
    ap.add_argument("--sheets", default=None)
    ap.add_argument("--only", default=None, help="make only the sounds whose names start so")
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 1)
    a = ap.parse_args()
    man = json.load(open(os.path.join(AUDIO, "manifest.json"), encoding="utf-8"))
    sounds = [d for d in man["sounds"] if not d["optional"]]
    SRC = Sources(a.cache)
    for p in PACKS:
        SRC.pack(p)
    ROOM = make_room(np.random.default_rng(7))
    todo = [(d, k) for d in sounds for k in range(d["takes"])
            if a.only is None or d["name"].startswith(a.only)]
    import multiprocessing as mp
    with mp.get_context("fork").Pool(a.jobs) as pool:
        done = pool.map(job, todo, chunksize=1)
    by = {(name, k): (rows, img) for name, k, rows, img in done}

    # The report.
    bad = []
    total = {"ogg": 0, "mp3": 0}
    print(f"{'file':34} {'dur s':>6} {'max s':>6} {'peak':>6} {'rms':>6} {'aim':>5} {'onset':>6}")
    for d in sounds:
        lim = stated_len(d) * 1.5
        aim = TARGET[d["bus"]]
        for k, stem in enumerate(d["files"]):
            for fmt in ("ogg", "mp3"):
                path = os.path.join(AUDIO, f"{stem}.{fmt}")
                if not os.path.isfile(path):
                    bad.append(f"{stem}.{fmt}: missing")
                    continue
                total[fmt] += os.path.getsize(path)
            if (d["name"], k) not in by:
                continue
            for m in by[(d["name"], k)][0]:
                why = []
                codec, ch, rate = m["probe"]
                want = "opus" if m["file"].endswith(".ogg") else "mp3"
                if codec != want or ch != (2 if d["stereo"] else 1) or rate != SR:
                    why.append(f"{codec} {ch} ch {rate} Hz")
                if m["peak"] > CEIL:
                    why.append("peak")
                if not d["loop"] and m["dur"] > lim:
                    why.append("too long")
                if not d["loop"] and m["onset"] > 5.0:
                    why.append("late onset")
                if abs(m["rms"] - aim) > RMS_TOL:
                    why.append("level")
                flag = ("  <- " + ", ".join(why)) if why else ""
                print(f"{m['file']:34} {m['dur']:6.2f} {lim:6.2f} {m['peak']:6.1f} "
                      f"{m['rms']:6.1f} {aim:5.0f} {m['onset']:6.1f}{flag}")
                if why:
                    bad.append(f"{m['file']}: " + ", ".join(why))
    for fmt, b in total.items():
        print(f"{fmt}: {b / 2 ** 20:.2f} MiB")
        if b > 3 * 2 ** 20:
            bad.append(f"the {fmt} set is {b / 2 ** 20:.2f} MiB, over 3")
    if a.sheets:
        os.makedirs(a.sheets, exist_ok=True)
        groups = {}
        for d in sounds:
            fam = d["name"].split(".")[0]
            g = fam if fam in FAMILIES else "shared"
            line = [(stem, by[(d["name"], k)][1]) for k, stem in enumerate(d["files"])
                    if (d["name"], k) in by]
            if line:
                groups.setdefault(g, []).append((d["name"], line))
        for g, rows in groups.items():
            p = os.path.join(a.sheets, f"{g}.png")
            sheet(g, rows, p)
            print("sheet", p)
    if bad:
        print(f"\n{len(bad)} problems:", *bad, sep="\n  ")
        sys.exit(1)
    print("all", sum(d["takes"] for d in sounds), "takes good")


if __name__ == "__main__":
    main()
