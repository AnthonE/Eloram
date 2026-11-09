#!/usr/bin/env python3
"""The game's voices, as placeholders: a file for every take of the announcer's lines and every
champion's (web/assets/audio/manifest.json: `sounds` whose bus is "voice", and `voices`; their
words are el_client_core::fx::voice::says and el_client_core::fx::vo's scripts), spoken by Piper
(a neural text-to-speech, https://github.com/OHF-Voice/piper1-gpl) and processed into a voice of
each champion's own.

    python3 -I art/tools/voices.py [--models dir] [--only prefix,...] [--force] [--listen]

A placeholder stands in until a recording replaces it: drop `<stem>.ogg` and `<stem>.mp3` of the
same name into web/assets/audio/ (and give them their line in art/LICENSES.md). The tool keeps
the hash of every file it wrote in art/voices.lock.json, and never writes over a file that isn't
one of its own: a recording is never lost to a rerun. A placeholder whose words or casting
changed is made again; `--force` makes every placeholder again.

The casting (CAST, below) is Riot's way with a roster, cut down to what text-to-speech can do: a
speaker each, no two of the twenty close in pitch and manner (the writers' "roster comparisons to
flag vocal overlap"), each pitched and paced to its brief (vo.rs's `cast` notes), then processed as
Riot layers its processed voices (Spirit Blossom Thresh: one layer for intelligibility, a
muffled deeper copy, low end felt more than heard): a family's traits as quiet layers under a
voice that stays clear. The announcer is one voice of its own, steady and close, with a short
plate round it.

With --listen, a speech recogniser (faster-whisper's base.en) transcribes every file written and
the report gives the words it heard against the words meant: what stands in for a listener, since
no one has listened yet; a line whose letters heard are less than 60% like the ones meant is
listed, to listen to. Exits non-zero if a file breaks a rule: a peak over -1 dBFS, an onset past
50 ms, a level off its target.

Models are downloaded once into the cache (default art/cache/voices/, not in git), pinned by
hash: Piper's VCTK voice (109 speakers, CC BY 4.0, the Centre for Speech Technology Research,
University of Edinburgh) for the champions, and its Cori voice (LibriVox recordings, public domain)
for the announcer. Needs `pip install piper-tts numpy` (and `faster-whisper` for --listen), and
an ffmpeg with libopus and libmp3lame.
"""
import argparse
import difflib
import hashlib
import json
import math
import os
import re
import subprocess
import sys
import urllib.request
import zlib

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
AUDIO = os.path.join(ROOT, "web", "assets", "audio")
LOCK = os.path.join(ROOT, "art", "voices.lock.json")
SR = 48000
CEIL = -1.0
TARGET = -18.0  # active RMS (dBFS) of every line, the announcer's and the champions'
RMS_TOL = 2.5
# A line starts within this (ms, at 40 dB under its peak): speech may start on a soft consonant
# (a "w", a "b" voiced before it opens).
ONSET_MS = 50.0

HF = "https://huggingface.co/rhasspy/piper-voices/resolve/main"
MODELS = {
    "vctk": ("en/en_GB/vctk/medium/en_GB-vctk-medium",
             "4e9fc85ab9009385319fc6bae7f55577f8a2d7ee77fd9159a5500eb6531f41e6",
             "7f85e6391ed0f7f46e4abd19345929a16be931a0c9945086f96692dce2087fa8"),
    "cori": ("en/en_GB/cori/high/en_GB-cori-high",
             "470b4dd634c98f8a4850d7626ffc3dfc90774628eeef6605a6dd8f88f30a5903",
             "9e7fb5b5671612c22f3c81cbe46c1ae87b031a4632bcb509e499dad6f1e2adec"),
}

# ---------------------------------------------------------------------------------------------
# The casting: a voice, its pitch (semitones), its pace (Piper's length scale: over 1 is slower),
# and its processing, champion by champion. Each speaker's median pitch was measured over the same
# sentence (male voices 81 to 149 Hz, female 158 to 248 Hz) and the twenty spread across them.
#
# The processing, quietest first: "close" (a dry small room), "room" (a hall), "hall" (a long
# bright room), "warm" (saturation and a low shelf), "grit" (an edge in the top), "chorus" (two
# slow moving copies: water), "breath" (air shaped by the voice: wind), "shimmer" (an octave up,
# very quiet: frost and starlight), "under" (an octave down, muffled: night, the deep), "bronze" (a
# metal comb), "machine" (ring modulation and a crushed copy: the siege engine), "whisper"
# (a breathy double, the voice itself a little hushed).

CAST = {
    "Vessa": ("vctk", "p231", 0.0, 1.0, ["close", "grit"]),
    "Orrin": ("vctk", "p316", 0.0, 1.08, ["warm", "hall"]),
    "Brakka": ("vctk", "p237", -2.5, 1.12, ["warm", "under", "close"]),
    "Kesh": ("vctk", "p334", -0.5, 1.02, ["whisper", "close"]),
    "Liora": ("vctk", "p225", 0.0, 1.04, ["bronze", "room"]),
    "Talmar": ("vctk", "p326", -1.0, 1.0, ["chorus", "room"]),
    "Seren": ("vctk", "p276", 0.5, 1.12, ["shimmer", "hall"]),
    "Hollis": ("vctk", "p245", -2.0, 1.14, ["bronze", "under", "room"]),
    "Vire": ("vctk", "p343", -0.5, 0.95, ["grit", "close"]),
    "Oda": ("vctk", "p262", -1.0, 1.12, ["bronze", "close"]),
    "Marret": ("vctk", "p345", 0.0, 1.0, ["close", "grit"]),
    "Nimra": ("vctk", "p248", 0.0, 1.0, ["shimmer", "room"]),
    "Gorum": ("vctk", "p254", -2.0, 1.15, ["under", "warm", "close"]),
    "Sable": ("vctk", "p294", 0.0, 0.97, ["under", "whisper", "room"]),
    "Pell": ("vctk", "p298", 0.0, 0.95, ["bronze", "room"]),
    "Ulla": ("vctk", "p249", -0.5, 1.1, ["warm", "close"]),
    "Torek": ("vctk", "p311", -1.0, 1.06, ["machine", "close"]),
    "Sefa": ("vctk", "p310", 0.5, 0.94, ["breath", "room"]),
    "Brun": ("vctk", "p256", -1.0, 1.0, ["warm", "grit", "room"]),
    "Mira": ("vctk", "p307", 0.5, 0.96, ["warm", "room"]),
}
ANNOUNCER = ("cori", None, 0.0, 1.06, ["presence", "plate"])
NOISE = (0.55, 0.7)  # Piper's noise scales: a little steadier than its defaults

# ---------------------------------------------------------------------------------------------
# Models.


def model(cache, key):
    path, sha, sha_cfg = MODELS[key]
    os.makedirs(cache, exist_ok=True)
    base = os.path.join(cache, os.path.basename(path))
    for ext, want in ((".onnx", sha), (".onnx.json", sha_cfg)):
        f = base + ext
        if not os.path.isfile(f):
            url = f"{HF}/{path}{ext}"
            print("fetching", url, flush=True)
            data = urllib.request.urlopen(url, timeout=600).read()
            if hashlib.sha256(data).hexdigest() != want:
                sys.exit(f"{url}: not the model we pinned (sha256 differs)")
            with open(f + ".part", "wb") as o:
                o.write(data)
            os.replace(f + ".part", f)
        else:
            h = hashlib.sha256(open(f, "rb").read()).hexdigest()
            if h != want:
                sys.exit(f"{f}: not the model we pinned; delete it to fetch again")
    return base + ".onnx"


class Voices:
    def __init__(self, cache):
        from piper import PiperVoice
        self.cache = cache
        self.loaded = {}
        self.PiperVoice = PiperVoice

    def say(self, key, speaker, text, length, semis):
        """`text` spoken, mono at 48 kHz, `semis` up (the pace kept: it's spoken slower first)."""
        from piper import SynthesisConfig
        if key not in self.loaded:
            m = model(self.cache, key)
            cfg = json.load(open(m + ".json"))
            self.loaded[key] = (self.PiperVoice.load(m), cfg)
        v, cfg = self.loaded[key]
        ids = cfg.get("speaker_id_map") or {}
        r = 2 ** (semis / 12)
        syn = SynthesisConfig(speaker_id=ids.get(speaker) if speaker else None, length_scale=length * r,
                              noise_scale=NOISE[0], noise_w_scale=NOISE[1])
        x = np.concatenate([c.audio_float_array for c in v.synthesize(text, syn)]).astype(np.float64)
        sr = cfg["audio"]["sample_rate"]
        # Pitched by playing it faster (or slower) than it was made, then brought to 48 kHz.
        return resample(x, sr * r, SR)


def resample(x, sr_in, sr_out):
    data = np.ascontiguousarray(x, dtype="<f4").tobytes()
    out = subprocess.run(
        ["ffmpeg", "-v", "error", "-nostdin", "-f", "f32le", "-ar", str(int(round(sr_in))),
         "-ac", "1", "-i", "-", "-af", f"aresample={sr_out}:filter_size=64", "-f", "f32le", "-"],
        input=data, check=True, stdout=subprocess.PIPE).stdout
    return np.frombuffer(out, dtype="<f4").astype(np.float64)


# ---------------------------------------------------------------------------------------------
# The processing.


def T(s):
    return max(1, int(round(s * SR)))


def db(v):
    return 10 ** (v / 20)


def eq(x, shape):
    """`x` through a zero-phase filter whose gain at f Hz is shape(f)."""
    n = len(x) + T(0.2)
    m = 1 << int(math.ceil(math.log2(n)))
    X = np.fft.rfft(x, m)
    f = np.fft.rfftfreq(m, 1 / SR)
    return np.fft.irfft(X * shape(f), m)[:len(x)]


def lp(f, fc, o=2):
    return 1 / np.sqrt(1 + (f / fc) ** (2 * o))


def hp(f, fc, o=2):
    return 1 / np.sqrt(1 + (fc / np.maximum(f, 1e-6)) ** (2 * o))


def shelf(f, fc, gain_db):
    """Low shelf: `gain_db` under fc, none over."""
    g = db(gain_db)
    return 1 + (g - 1) / (1 + (f / fc) ** 2)


def bell(f, fc, q, gain_db):
    return 1 + (db(gain_db) - 1) / (1 + ((f - fc) / (fc / q)) ** 2)


def env(x, s=0.02):
    """The voice's envelope: its RMS over `s`."""
    w = T(s)
    c = np.concatenate([[0.0], np.cumsum(x * x)])
    i = np.arange(len(x))
    lo = np.maximum(i - w // 2, 0)
    hi = np.minimum(i + w // 2, len(x))
    return np.sqrt((c[hi] - c[lo]) / np.maximum(hi - lo, 1))


def shift(x, ratio, win=0.045):
    """Pitched by `ratio` and the length kept: grains read faster (or slower), laid back in place."""
    N = T(win)
    hop = N // 4
    w = np.hanning(N)
    out = np.zeros(len(x) + N)
    norm = np.zeros(len(x) + N)
    xp = np.concatenate([x, np.zeros(int(N * ratio) + 2)])
    for s in range(0, len(x), hop):
        idx = s + np.arange(N) * ratio
        g = np.interp(idx, np.arange(len(xp)), xp)
        out[s:s + N] += g * w
        norm[s:s + N] += w
    return (out / np.maximum(norm, 1e-3))[:len(x)]


def reverb(x, rt, wet, bright=6000.0, pre=0.012, seed=1):
    """A room: decaying noise, darker as it goes, after a pre-delay."""
    rng = np.random.default_rng(seed)
    n = T(rt * 1.2)
    t = np.arange(n) / SR
    ir = rng.standard_normal(n) * np.exp(-6.9 * t / rt)
    ir = eq(ir, lambda f: lp(f, bright, 1) * hp(f, 180, 1))
    ir[:T(pre)] = 0
    ir /= np.sqrt(np.sum(ir ** 2)) + 1e-12
    m = 1 << int(math.ceil(math.log2(len(x) + n)))
    y = np.fft.irfft(np.fft.rfft(x, m) * np.fft.rfft(ir, m), m)[:len(x) + n]
    out = np.concatenate([x, np.zeros(n)])
    return out + wet * y * (np.sqrt(np.mean(x ** 2)) / (np.sqrt(np.mean(y ** 2)) + 1e-12))


def fx(x, name, rng):
    if name == "close":
        return reverb(x, 0.25, 0.06, 7000, 0.004)
    if name == "room":
        return reverb(x, 0.6, 0.11, 6000, 0.01)
    if name == "hall":
        # Long enough to be a hall, short and late enough to leave the words clear.
        return reverb(x, 0.95, 0.09, 5000, 0.03)
    if name == "plate":
        return reverb(x, 0.9, 0.08, 9000, 0.008)
    if name == "presence":
        y = eq(x, lambda f: hp(f, 90) * bell(f, 3200, 1.2, 2.5) * shelf(f, 220, 1.5))
        return compress(y, -24, 2.5)
    if name == "warm":
        y = np.tanh(2.0 * x / (np.max(np.abs(x)) + 1e-9)) * np.max(np.abs(x))
        return eq(0.5 * x + 0.5 * y, lambda f: shelf(f, 250, 2.5) * lp(f, 11000, 1))
    if name == "grit":
        top = eq(x, lambda f: hp(f, 2500, 2))
        return x + 0.25 * np.tanh(6 * top / (np.max(np.abs(top)) + 1e-9)) * np.max(np.abs(top))
    if name == "chorus":
        n = np.arange(len(x))
        out = x.copy()
        for k, (rate, depth, base) in enumerate(((0.6, 0.004, 0.018), (0.45, 0.005, 0.026))):
            d = (base + depth * np.sin(2 * np.pi * rate * n / SR + k)) * SR
            out += 0.3 * np.interp(n - d, n, x, left=0)
        return out
    if name == "breath":
        air = eq(rng.standard_normal(len(x)), lambda f: hp(f, 1800, 2) * lp(f, 8000, 2))
        e = env(x, 0.03)
        return x + 0.09 * air * e / (np.max(e) + 1e-9) * np.max(np.abs(x))
    if name == "shimmer":
        up = eq(shift(x, 2.0), lambda f: hp(f, 2500, 2))
        return x + 0.07 * up * (np.max(np.abs(x)) / (np.max(np.abs(up)) + 1e-9))
    if name == "under":
        down = eq(shift(x, 0.5, 0.06), lambda f: lp(f, 900, 2))
        return x + 0.16 * down * (np.max(np.abs(x)) / (np.max(np.abs(down)) + 1e-9))
    if name == "bronze":
        y = x.copy()
        for k, g in enumerate((0.3, 0.16, 0.08), start=1):
            d = T(k / 410.0)
            y[d:] += g * x[:-d]
        return 0.7 * x + 0.3 * y
    if name == "machine":
        t = np.arange(len(x)) / SR
        ring = x * np.sin(2 * np.pi * 58 * t)
        hold = 3
        crushed = np.repeat(x[::hold], hold)[:len(x)]
        crushed = np.round(crushed * 24) / 24
        return 0.68 * x + 0.22 * ring + 0.14 * eq(crushed, lambda f: lp(f, 7000, 2))
    if name == "whisper":
        noise = eq(rng.standard_normal(len(x)), lambda f: hp(f, 1500, 2) * lp(f, 9000, 2))
        e = env(eq(x, lambda f: hp(f, 300, 2)), 0.015)
        hush = noise * e / (np.max(e) + 1e-9) * np.max(np.abs(x))
        return eq(x, lambda f: shelf(f, 300, -3)) + 0.12 * hush
    raise SystemExit(f"no processing called {name}")


def compress(x, thresh_db, ratio, att=0.005, rel=0.08):
    e = env(x, 0.01)
    lvl = 20 * np.log10(np.maximum(e, 1e-9))
    over = np.maximum(lvl - thresh_db, 0)
    g = db(-over * (1 - 1 / ratio))
    # Smoothed: fast down, slower up.
    a, r = math.exp(-1 / (att * SR)), math.exp(-1 / (rel * SR))
    out = np.empty_like(g)
    cur = 1.0
    gs = g.tolist()
    for i, v in enumerate(gs):
        k = a if v < cur else r
        cur = k * cur + (1 - k) * v
        out[i] = cur
    return x * out


def active_rms_db(x):
    B = T(0.01)
    m = len(x) // B
    if m == 0:
        return -120.0
    p = np.mean(x[:m * B].reshape(m, B) ** 2, axis=1)
    act = p > db(-40) ** 2
    return 10 * math.log10(float(np.mean(p[act]))) if act.any() else -120.0


def true_peak_db(x):
    n = len(x)
    m = 1 << int(math.ceil(math.log2(n + 256)))
    X = np.fft.rfft(x, m)
    Y = np.zeros(m * 2 + 1, dtype=complex)
    Y[:len(X)] = X
    y = np.fft.irfft(Y, m * 4)[:n * 4] * 4
    return 20 * math.log10(max(float(np.max(np.abs(y))), 1e-12))


def trim(x):
    """To its onset (40 dB under its peak) and its tail (50 dB under, then 60 ms), faded in and out."""
    a = np.abs(x)
    pk = a.max()
    s = max(0, int(np.argmax(a > pk * db(-40))) - T(0.0005))
    e = env(x[s:], 0.01)
    alive = np.nonzero(e > pk * db(-50))[0]
    end = s + (alive[-1] + T(0.06) if len(alive) else len(x) - s)
    x = x[s:min(end, len(x))].copy()
    fi, fo = T(0.002), min(T(0.05), len(x) // 3)
    x[:fi] *= np.linspace(0, 1, fi)
    x[len(x) - fo:] *= np.linspace(1, 0, fo) ** 1.5
    return x


def master(x):
    """Trimmed to its onset and tail, brought to the target, its peaks under the ceiling."""
    x = eq(x, lambda f: hp(f, 70, 2) * lp(f, 15000, 2))
    # Trimmed before it's compressed (which lifts the quiet before the words) and again after, at
    # the level the onset is measured at.
    x = trim(x)
    x = compress(x, -26, 2.0)
    x = x * db(TARGET - active_rms_db(x))
    x = trim(x)
    tp = true_peak_db(x)
    if tp > CEIL - 0.8:
        # The peaks taken down alone: a soft clip, then the level again.
        c = db(CEIL - 0.8)
        x = np.tanh(x / c) * c
        x = x * db(TARGET - active_rms_db(x))
        tp = true_peak_db(x)
        if tp > CEIL - 0.5:
            x = x * db(CEIL - 0.5 - tp)
    return x


# ---------------------------------------------------------------------------------------------
# Writing, and the check.


def encode(y, path, fmt):
    # Speech: Opus at 28 kb/s, 48 kHz; the MP3 (for a browser without Opus) at 24 kHz, which
    # speech loses nothing to and halves the file.
    codec = (["-c:a", "libopus", "-b:a", "28k", "-vbr", "on", "-compression_level", "10",
              "-application", "voip", "-ar", str(SR)] if fmt == "ogg"
             else ["-c:a", "libmp3lame", "-q:a", "7", "-id3v2_version", "0", "-ar", str(SR // 2)])
    data = np.ascontiguousarray(y, dtype="<f4").tobytes()
    tmp = path + ".part"
    subprocess.run(
        ["ffmpeg", "-v", "error", "-nostdin", "-y", "-f", "f32le", "-ar", str(SR), "-ac", "1", "-i", "-",
         *codec, "-map_metadata", "-1", "-fflags", "+bitexact", "-flags:a", "+bitexact",
         "-f", "ogg" if fmt == "ogg" else "mp3", tmp], input=data, check=True)
    os.replace(tmp, path)


def decode(path):
    out = subprocess.run(["ffmpeg", "-v", "error", "-nostdin", "-i", path, "-ac", "1", "-ar", str(SR),
                          "-f", "f32le", "-"], check=True, stdout=subprocess.PIPE).stdout
    return np.frombuffer(out, dtype="<f4").astype(np.float64)


def sha(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


def measure(path):
    x = decode(path)
    a = np.abs(x)
    return dict(file=os.path.basename(path), dur=len(x) / SR, peak=true_peak_db(x), rms=active_rms_db(x),
                onset=1000 * int(np.argmax(a > a.max() * db(-40))) / SR, size=os.path.getsize(path),
                sha=sha(path))


def write(y, stem):
    rows = []
    for fmt in ("ogg", "mp3"):
        path = os.path.join(AUDIO, f"{stem}.{fmt}")
        g = 0.0
        for _ in range(5):
            encode(y * db(g), path, fmt)
            x = decode(path)
            tp = true_peak_db(x)
            if tp <= CEIL - 0.05:
                break
            g -= tp - CEIL + 0.15
        rows.append(measure(path))
    return rows


def letters(t):
    return re.sub(r"[^a-z]", "", t.lower())


def heard(said, meant):
    """How much of what was meant was heard: the letters of each, spaces and stops dropped ("white
    out" is "whiteout"), matched (difflib's ratio, 1 for the same)."""
    a, b = letters(meant), letters(said)
    if not a:
        return 1.0
    return difflib.SequenceMatcher(None, a, b, autojunk=False).ratio()


def save(lock):
    with open(LOCK + ".part", "w") as f:
        json.dump(dict(sorted(lock.items())), f, indent=1)
        f.write("\n")
    os.replace(LOCK + ".part", LOCK)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--models", default=os.path.join(ROOT, "art", "cache", "voices"))
    ap.add_argument("--only", default=None, help="make only the lines whose names start so (comma-separated)")
    ap.add_argument("--force", action="store_true", help="make every placeholder again")
    ap.add_argument("--listen", action="store_true", help="transcribe each file and score its words")
    a = ap.parse_args()
    man = json.load(open(os.path.join(AUDIO, "manifest.json"), encoding="utf-8"))
    lines = []
    for d in man["sounds"]:
        if d["bus"] == "voice" and d.get("says"):
            lines.append((d["name"], d["files"][0], d["says"], ANNOUNCER))
    for d in man["voices"]:
        cast = CAST[d["champion"]]
        for stem, text in zip(d["files"], d["says"]):
            lines.append((d["name"], stem, text, cast))
    only = None if a.only is None else tuple(a.only.split(","))
    lock = json.load(open(LOCK)) if os.path.isfile(LOCK) else {}
    voices = Voices(a.models)
    listener = None
    if a.listen:
        from faster_whisper import WhisperModel
        listener = WhisperModel("base.en", device="cpu", compute_type="int8",
                                download_root=os.path.join(a.models, "whisper"))
    bad, listen, rows, kept, made = [], [], [], 0, 0
    for name, stem, text, cast in lines:
        if only is not None and not name.startswith(only):
            continue
        key, speaker, semis, length, chain = cast
        recipe = json.dumps([text, key, speaker, semis, length, chain, NOISE], sort_keys=True)
        paths = [os.path.join(AUDIO, f"{stem}.{f}") for f in ("ogg", "mp3")]
        mine = stem in lock
        there = [os.path.isfile(p) for p in paths]
        if any(there) and (not mine or any(sha(p) not in (lock[stem]["ogg"], lock[stem]["mp3"])
                                           for p, t in zip(paths, there) if t)):
            # Not a file this tool wrote: a recording. Never written over.
            kept += 1
            continue
        if mine and all(there) and lock[stem]["recipe"] == recipe and not a.force:
            if listener is None:
                continue
            # Already made: only heard again.
            out = [measure(p) for p in paths]
        else:
            rng = np.random.default_rng(zlib.crc32(stem.encode()))
            x = voices.say(key, speaker, text, length, semis)
            for f in chain:
                x = fx(x, f, rng)
            y = master(x)
            out = write(y, stem)
            lock[stem] = {"recipe": recipe, "ogg": out[0]["sha"], "mp3": out[1]["sha"]}
            # Kept as each is written: a run cut short leaves no placeholder taken for a recording.
            save(lock)
            made += 1
        score = None
        if listener is not None:
            # Heard as a browser would play it: the Opus file, decoded (at the recogniser's 16 kHz).
            pcm = subprocess.run(["ffmpeg", "-v", "error", "-nostdin", "-i", os.path.join(AUDIO, f"{stem}.ogg"),
                                  "-ac", "1", "-ar", "16000", "-f", "f32le", "-"],
                                 check=True, stdout=subprocess.PIPE).stdout
            segs, _ = listener.transcribe(np.frombuffer(pcm, dtype="<f4"), language="en", beam_size=5)
            said = " ".join(s.text for s in segs).strip()
            score = heard(said, text)
        for m in out:
            why = []
            if m["peak"] > CEIL:
                why.append("peak")
            if m["onset"] > ONSET_MS:
                why.append("late onset")
            if abs(m["rms"] - TARGET) > RMS_TOL:
                why.append("level")
            if score is not None and score < 0.6 and m["file"].endswith(".ogg"):
                # The recogniser's word is advice (it mishears short barks a person wouldn't): a
                # line to listen to, not a broken file.
                listen.append(f"{m['file']}: {text!r} heard as {said!r}")
            if why:
                bad.append(f"{m['file']}: " + ", ".join(why))
            ogg = m["file"].endswith(".ogg")
            rows.append((m, score if ogg else None, text + (f"  ({said})" if ogg and score is not None and score < 0.999 else "")))
    save(lock)
    print(f"{'file':38} {'dur s':>6} {'peak':>6} {'rms':>6} {'onset':>6} {'heard':>6}  says (heard as)")
    for m, score, text in rows:
        h = "" if score is None else f"{100 * score:5.0f}%"
        print(f"{m['file']:38} {m['dur']:6.2f} {m['peak']:6.1f} {m['rms']:6.1f} {m['onset']:6.1f} {h:>6}  {text}")
    total = {f: sum(os.path.getsize(os.path.join(AUDIO, f"{s}.{f}")) for _, s, _, _ in lines
                    if os.path.isfile(os.path.join(AUDIO, f"{s}.{f}"))) for f in ("ogg", "mp3")}
    for f, b in total.items():
        print(f"{f}: {b / 2 ** 20:.2f} MiB of voices")
    scored = [s for _, s, _ in rows if s is not None]
    if scored:
        print(f"heard: {100 * sum(scored) / len(scored):.0f}% alike, on average; "
              f"{sum(1 for s in scored if s >= 0.999)} of {len(scored)} lines word for word, "
              f"{sum(1 for s in scored if s >= 0.8)} at 80% or more")
    print(f"{made} made, {kept} recordings kept as they are")
    if listen:
        print(f"\n{len(listen)} lines the recogniser didn't follow (listen to them):", *listen, sep="\n  ")
    if bad:
        print(f"\n{len(bad)} problems:", *bad, sep="\n  ")
        sys.exit(1)


if __name__ == "__main__":
    main()
