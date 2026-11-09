// Clips from another rig onto a champion's (docs/CHAMPIONS.md, "The second clips"): Quaternius's
// Universal Animation Library (CC0, `art/tools/ual.py`), whose two volumes share one skeleton under
// two sets of bone names, put on Meshy's 24-bone rigs. Both stand facing +Z, their left on +X; the
// library's rest is a T-pose and Meshy's an A-pose, so each bone's turn is taken from the library's
// rest after turning that rest onto the champion's: the shortest turn that lays each bone of one
// along the same bone of the other. Then a bone of the champion points where the library's does.
//
// Also: forward kinematics over any glTF (a node's place and turn in the world at a time of a
// clip), the moment a clip strikes (its weapon's tip, or its hand, fastest), and a sheet of stick
// figures a clip, to look at clips without the browser.
import sharp from 'sharp';

// Quaternions as [x, y, z, w].
const qmul = (a, b) => [
  a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
  a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
  a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
  a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
];
const qinv = (q) => [-q[0], -q[1], -q[2], q[3]];
const qnorm = (q) => {
  const l = Math.hypot(...q) || 1;
  return q.map((v) => v / l);
};
const qrot = (q, v) => {
  const p = qmul(qmul(q, [v[0], v[1], v[2], 0]), qinv(q));
  return [p[0], p[1], p[2]];
};
const sub = (a, b) => a.map((v, i) => v - b[i]);
const norm = (v) => {
  const l = Math.hypot(...v) || 1;
  return v.map((x) => x / l);
};
/** The shortest turn taking unit vector `a` onto unit vector `b`. */
function arc(a, b) {
  const d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  if (d < -0.9999) return [1, 0, 0, 0];
  const c = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
  return qnorm([c[0], c[1], c[2], 1 + d]);
}
function slerp(a, b, t) {
  let d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
  if (d < 0) {
    b = b.map((v) => -v);
    d = -d;
  }
  if (d > 0.9995) return qnorm(a.map((v, i) => v + (b[i] - v) * t));
  const th = Math.acos(d);
  const s = Math.sin(th);
  return a.map((v, i) => v * (Math.sin((1 - t) * th) / s) + b[i] * (Math.sin(t * th) / s));
}

/** A sampler's value at `t`: linear, slerp for a rotation, a step held. A normalised integer
 *  output (a shipped clip's rotations) is read back to floats. */
function sample(sampler, t, rotation) {
  const times = sampler.getInput().getArray();
  const out = sampler.getOutput();
  const n = out.getElementSize();
  const arr = out.getArray();
  const k = out.getNormalized() ? (arr instanceof Int16Array ? 32767 : arr instanceof Int8Array ? 127 : 1) : 1;
  const at = (i) => Array.from(arr.subarray(i * n, i * n + n), (v) => Math.max(-1, v / k));
  const last = times.length - 1;
  if (t <= times[0]) return at(0);
  if (t >= times[last]) return at(last);
  let i = 0;
  while (times[i + 1] < t) i++;
  if (sampler.getInterpolation() === 'STEP') return at(i);
  const u = (t - times[i]) / (times[i + 1] - times[i]);
  return rotation ? slerp(at(i), at(i + 1), u) : at(i).map((v, j) => v + (at(i + 1)[j] - v) * u);
}

/** Meshy's bones, parents first, and each one's next along it (whose direction at rest is the
 *  bone's). A hand and a toe have none and take their parent's turn onto the other rest; the head's
 *  is none at all. */
export const NEXT = {
  Hips: 'Spine02',
  Spine02: 'Spine01',
  Spine01: 'Spine',
  Spine: 'neck',
  neck: 'Head',
  Head: null,
  LeftShoulder: 'LeftArm',
  LeftArm: 'LeftForeArm',
  LeftForeArm: 'LeftHand',
  LeftHand: null,
  RightShoulder: 'RightArm',
  RightArm: 'RightForeArm',
  RightForeArm: 'RightHand',
  RightHand: null,
  LeftUpLeg: 'LeftLeg',
  LeftLeg: 'LeftFoot',
  LeftFoot: 'LeftToeBase',
  LeftToeBase: null,
  RightUpLeg: 'RightLeg',
  RightLeg: 'RightFoot',
  RightFoot: 'RightToeBase',
  RightToeBase: null,
};

const side = (names) => {
  const out = {};
  for (const [m, s] of Object.entries(names)) {
    if (m.startsWith('Left')) {
      out[m] = s('L', 'l');
      out[`Right${m.slice(4)}`] = s('R', 'r');
    } else out[m] = s;
  }
  return out;
};

/** Each library's bone for each of Meshy's. */
export const MAPS = {
  ual1: side({
    Hips: 'DEF-hips',
    Spine02: 'DEF-spine.001',
    Spine01: 'DEF-spine.002',
    Spine: 'DEF-spine.003',
    neck: 'DEF-neck',
    Head: 'DEF-head',
    LeftShoulder: (s) => `DEF-shoulder.${s}`,
    LeftArm: (s) => `DEF-upper_arm.${s}`,
    LeftForeArm: (s) => `DEF-forearm.${s}`,
    LeftHand: (s) => `DEF-hand.${s}`,
    LeftUpLeg: (s) => `DEF-thigh.${s}`,
    LeftLeg: (s) => `DEF-shin.${s}`,
    LeftFoot: (s) => `DEF-foot.${s}`,
    LeftToeBase: (s) => `DEF-toe.${s}`,
  }),
  ual2: side({
    Hips: 'pelvis',
    Spine02: 'spine_01',
    Spine01: 'spine_02',
    Spine: 'spine_03',
    neck: 'neck_01',
    Head: 'Head',
    LeftShoulder: (_, s) => `clavicle_${s}`,
    LeftArm: (_, s) => `upperarm_${s}`,
    LeftForeArm: (_, s) => `lowerarm_${s}`,
    LeftHand: (_, s) => `hand_${s}`,
    LeftUpLeg: (_, s) => `thigh_${s}`,
    LeftLeg: (_, s) => `calf_${s}`,
    LeftFoot: (_, s) => `foot_${s}`,
    LeftToeBase: (_, s) => `ball_${s}`,
  }),
};
/** Meshy's own rig names its bones as Meshy does. */
export const MESHY = Object.fromEntries(Object.keys(NEXT).map((n) => [n, n]));

/** Every node's place, turn and scale in the world at time `t` of `anim` (or at rest, without
 *  one): a Map from node to { pos, rot, scale }. */
export function pose(doc, anim, t) {
  const curves = new Map();
  for (const c of anim?.listChannels() ?? []) {
    const n = c.getTargetNode();
    if (!curves.has(n)) curves.set(n, {});
    curves.get(n)[c.getTargetPath()] = c.getSampler();
  }
  const out = new Map();
  const visit = (node, parent) => {
    const c = curves.get(node) ?? {};
    const r = c.rotation ? qnorm(sample(c.rotation, t, true)) : node.getRotation();
    const tr = c.translation ? sample(c.translation, t, false) : node.getTranslation();
    const s = c.scale ? sample(c.scale, t, false) : node.getScale();
    const p = parent ?? { pos: [0, 0, 0], rot: [0, 0, 0, 1], scale: [1, 1, 1] };
    const w = {
      pos: qrot(p.rot, tr.map((v, i) => v * p.scale[i])).map((v, i) => v + p.pos[i]),
      rot: qmul(p.rot, r),
      scale: s.map((v, i) => v * p.scale[i]),
    };
    out.set(node, w);
    for (const ch of node.listChildren()) visit(ch, w);
  };
  for (const n of doc.getRoot().listScenes()[0].listChildren()) visit(n, null);
  return out;
}

/** A rig's mapped bones: Meshy's name to the node (`map` from MAPS, or MESHY). */
export function bones(doc, map) {
  const byName = new Map(doc.getRoot().listNodes().map((n) => [n.getName(), n]));
  const out = new Map();
  for (const [m, s] of Object.entries(map)) {
    const n = byName.get(s);
    if (!n) throw new Error(`no bone ${s} for ${m}`);
    out.set(m, n);
  }
  return out;
}

/** Puts on `doc` (whose rig `dst` is champions.mjs's skeleton: Meshy's name to { node, parent,
 *  t, world }) the clip `name`: `parts`, each { anim, from, to } of the library `src` ({ doc, map }),
 *  one after another, keyed `fps` times a second. The hips rise and fall by the library's, as a
 *  share of hip height; they never travel. Returns the new animation. */
export function retargetAcross(src, parts, doc, dst, name, fps = 30) {
  const sBones = bones(src.doc, src.map);
  const sRest = pose(src.doc, null, 0);
  const dRest = pose(doc, null, 0);
  const sAt = (m) => sRest.get(sBones.get(m));
  const dAt = (m) => dRest.get(dst.get(m).node);
  // Each bone's turn from the library's rest onto the champion's, parents first.
  const align = new Map();
  for (const m of dst.keys()) {
    if (!(m in NEXT)) continue;
    const next = NEXT[m];
    if (next) {
      const a = norm(sub(sAt(next).pos, sAt(m).pos));
      const b = norm(sub(dAt(next).pos, dAt(m).pos));
      align.set(m, arc(a, b));
    } else {
      const parent = dst.get(m).parent;
      align.set(m, m === 'Head' || !parent ? [0, 0, 0, 1] : align.get(parent));
    }
  }
  const names = [...dst.keys()].filter((m) => align.has(m));
  const times = [];
  const at = [];
  let t0 = 0;
  for (const p of parts) {
    const len = p.to - p.from;
    const n = Math.max(1, Math.round(len * fps));
    for (let i = times.length && at.length ? 1 : 0; i <= n; i++) {
      // A part's first key is the last one's last.
      times.push(t0 + (i / n) * len);
      at.push([p.anim, p.from + (i / n) * len]);
    }
    t0 += len;
  }
  const rots = new Map(names.map((m) => [m, []]));
  const hips = [];
  const sHips = sAt('Hips').pos[1];
  const dHips = dst.get('Hips');
  for (const [anim, t] of at) {
    const P = pose(src.doc, anim, t);
    const dWorld = new Map();
    for (const m of names) {
      const d = dst.get(m);
      const sw = P.get(sBones.get(m)).rot;
      const dw = qnorm(qmul(qmul(qmul(sw, qinv(sAt(m).rot)), qinv(align.get(m))), d.world));
      dWorld.set(m, dw);
      rots.get(m).push(...(d.parent && dWorld.has(d.parent) ? qnorm(qmul(qinv(dWorld.get(d.parent)), dw)) : dw));
    }
    const rise = (P.get(sBones.get('Hips')).pos[1] - sHips) / sHips;
    hips.push(dHips.t[0], dHips.t[1] * (1 + rise), dHips.t[2]);
  }
  const buffer = doc.getRoot().listBuffers()[0];
  const input = doc.createAccessor().setType('SCALAR').setArray(new Float32Array(times)).setBuffer(buffer);
  const out = doc.createAnimation(name);
  const add = (node, target, type, values) => {
    const s = doc
      .createAnimationSampler()
      .setInput(input)
      .setInterpolation('LINEAR')
      .setOutput(doc.createAccessor().setType(type).setArray(new Float32Array(values)).setBuffer(buffer));
    out.addSampler(s).addChannel(doc.createAnimationChannel().setTargetNode(node).setTargetPath(target).setSampler(s));
  };
  for (const m of names) add(dst.get(m).node, 'rotation', 'VEC4', rots.get(m));
  add(dHips.node, 'translation', 'VEC3', hips);
  return out;
}

/** A clip's length (s): its last key. */
export function length(anim) {
  return Math.max(
    ...anim.listChannels().map((c) => {
      const a = c.getSampler().getInput().getArray();
      return a[a.length - 1];
    }),
  );
}

/** When `anim` strikes (s): when the point `at` (a node, or a node and a point in its frame) moves
 *  fastest, within `[lo, hi]` of the clip (shares of its length). */
export function strike(doc, anim, at, lo = 0.1, hi = 0.9) {
  const len = length(anim);
  const [node, local] = Array.isArray(at) ? at : [at, [0, 0, 0]];
  const where = (t) => {
    const w = pose(doc, anim, t).get(node);
    return qrot(w.rot, local.map((v, i) => v * w.scale[i])).map((v, i) => v + w.pos[i]);
  };
  const dt = 1 / 60;
  let best = [0, len * lo];
  let prev = where(len * lo);
  for (let t = len * lo + dt; t <= len * hi; t += dt) {
    const p = where(t);
    const v = Math.hypot(...sub(p, prev)) / dt;
    if (v > best[0]) best = [v, t - dt / 2];
    prev = p;
  }
  return best[1];
}

const LINES = [
  ['Hips', 'Spine02'],
  ['Spine02', 'Spine01'],
  ['Spine01', 'Spine'],
  ['Spine', 'neck'],
  ['neck', 'Head'],
  ['Spine', 'LeftShoulder'],
  ['LeftShoulder', 'LeftArm'],
  ['LeftArm', 'LeftForeArm'],
  ['LeftForeArm', 'LeftHand'],
  ['Spine', 'RightShoulder'],
  ['RightShoulder', 'RightArm'],
  ['RightArm', 'RightForeArm'],
  ['RightForeArm', 'RightHand'],
  ['Hips', 'LeftUpLeg'],
  ['LeftUpLeg', 'LeftLeg'],
  ['LeftLeg', 'LeftFoot'],
  ['LeftFoot', 'LeftToeBase'],
  ['Hips', 'RightUpLeg'],
  ['RightUpLeg', 'RightLeg'],
  ['RightLeg', 'RightFoot'],
  ['RightFoot', 'RightToeBase'],
];

/** A sheet of stick figures: a row a clip, `frames` of it from the front and from the side (the
 *  figure's right in red, a prop's length in gold, the strike's frame boxed), as a PNG. `rows` are
 *  { name, anim, strike? }; `extra` is [[node, [x, y, z] in its frame]...] drawn from the node. */
export async function sheet(doc, map, rows, file, { frames = 8, extra = [] } = {}) {
  const b = bones(doc, map);
  const cell = 120;
  const w = cell * frames * 2 + 160;
  const h = cell * rows.length + 10;
  let svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><rect width="100%" height="100%" fill="#16181d"/>`;
  rows.forEach((row, r) => {
    const len = length(row.anim);
    const P0 = pose(doc, null, 0);
    const top = Math.max(...[...b.values()].map((n) => P0.get(n).pos[1]));
    const k = (cell * 0.8) / top;
    svg += `<text x="6" y="${r * cell + 20}" fill="#ddd" font-family="sans-serif" font-size="13">${row.name}</text>`;
    svg += `<text x="6" y="${r * cell + 38}" fill="#888" font-family="sans-serif" font-size="11">${len.toFixed(2)} s</text>`;
    if (row.strike != null)
      svg += `<text x="6" y="${r * cell + 54}" fill="#e8b84a" font-family="sans-serif" font-size="11">strike ${row.strike.toFixed(2)}</text>`;
    const strikeFrame = row.strike == null ? -1 : Math.round((row.strike / len) * (frames - 1));
    for (let f = 0; f < frames; f++) {
      const t = (f / (frames - 1)) * len;
      const P = pose(doc, row.anim, t);
      for (const view of [0, 1]) {
        const ox = 160 + (view * frames + f) * cell + cell / 2;
        const oy = r * cell + cell - 8;
        const pt = (p) => [ox + (view ? p[2] : -p[0]) * k, oy - p[1] * k];
        if (f === strikeFrame) svg += `<rect x="${ox - cell / 2 + 2}" y="${r * cell + 4}" width="${cell - 4}" height="${cell - 4}" fill="none" stroke="#e8b84a" stroke-width="1"/>`;
        for (const [a, c] of LINES) {
          const [x1, y1] = pt(P.get(b.get(a)).pos);
          const [x2, y2] = pt(P.get(b.get(c)).pos);
          const col = a.startsWith('Right') || c.startsWith('Right') ? '#e06a5a' : '#cfd6e0';
          svg += `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${col}" stroke-width="2"/>`;
        }
        for (const [node, local] of extra) {
          const wn = P.get(node);
          if (!wn) continue;
          const tip = qrot(wn.rot, local.map((v, i) => v * wn.scale[i])).map((v, i) => v + wn.pos[i]);
          const [x1, y1] = pt(wn.pos);
          const [x2, y2] = pt(tip);
          svg += `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="#e8b84a" stroke-width="2.5"/>`;
        }
        const [hx, hy] = pt(P.get(b.get('Head')).pos);
        svg += `<circle cx="${hx}" cy="${hy - 6}" r="6" fill="none" stroke="#cfd6e0" stroke-width="2"/>`;
      }
    }
  });
  svg += '</svg>';
  await sharp(Buffer.from(svg)).png().toFile(file);
}
