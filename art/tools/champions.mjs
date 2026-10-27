#!/usr/bin/env node
// The champions (docs/CHAMPIONS.md): Meshy's rigged characters made fit to ship, each with every
// clip it can play and the props it holds.
//   npm ci --prefix art/tools
//   node art/tools/champions.mjs <dir> [name ...]
// <dir> is what `art/tools/meshy.py fetch <dir>` wrote: champions/<name>.glb (rigged),
// champions/<name>.run.glb (its own run), champions/<set>.anim.glb (art/meshy.json's clips, made
// on one champion's rig), props/<prop>.glb. art/champions.json says how each prop is coloured,
// gripped and held.
//
// Each champion's web/assets/models/champions/<name>.glb has:
// - its mesh and skin, the tangents dropped (no normal map), the texture cut to 512 px JPEG;
// - its own run, and every shared clip retargeted onto it: each bone's turn from its rest pose, in
//   the world, put on its own rest pose (so a clip made on one body plays on another), and the
//   hips' rise and fall scaled by hip height. The hips never travel across the floor: where a
//   champion stands is the server's (Riot: a champion never "abandons" its hitbox);
// - each clip named as art/meshy.json names it (`run` and the sets' clips), its rotations as
//   16-bit integers, its redundant keys dropped;
// - its props on its bones (art/champions.json), each coloured by bands along its length;
// - its armature scaled so it stands 1 tall, feet at 0, facing +Z: the client sizes and turns it.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { NodeIO } from '@gltf-transform/core';
import { dedup, prune, resample, simplify, textureCompress, weld } from '@gltf-transform/functions';
import { MeshoptSimplifier } from 'meshoptimizer';
import sharp from 'sharp';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const OUT = path.join(ROOT, 'web/assets/models/champions');
const TASKS = JSON.parse(fs.readFileSync(path.join(ROOT, 'art/meshy.json'), 'utf8'));
const DRESS = JSON.parse(fs.readFileSync(path.join(ROOT, 'art/champions.json'), 'utf8'));
const CLIPS = JSON.parse(fs.readFileSync(path.join(ROOT, 'art/clips.json'), 'utf8'));
/** A champion's triangles, at most. */
const TRIANGLES = 6000;
/** How far along a hand bone (of the champion's height) the palm is: where a prop is gripped. */
const PALM = 0.045;

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
function slerp(a, b, t) {
  let d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
  if (d < 0) {
    b = b.map((v) => -v);
    d = -d;
  }
  if (d > 0.9995) return qnorm(a.map((v, i) => v + (b[i] - v) * t));
  const th = Math.acos(d);
  const s = Math.sin(th);
  const wa = Math.sin((1 - t) * th) / s;
  const wb = Math.sin(t * th) / s;
  return a.map((v, i) => v * wa + b[i] * wb);
}

/** A channel's value at time t (linear, or slerp for a rotation; a step holds). */
function sample(sampler, t, rotation) {
  const times = sampler.getInput().getArray();
  const out = sampler.getOutput();
  const n = out.getElementSize();
  const at = (i) => Array.from(out.getElement(i, new Array(n)));
  const last = times.length - 1;
  if (t <= times[0]) return at(0);
  if (t >= times[last]) return at(last);
  let i = 0;
  while (times[i + 1] < t) i++;
  if (sampler.getInterpolation() === 'STEP') return at(i);
  const u = (t - times[i]) / (times[i + 1] - times[i]);
  return rotation ? slerp(at(i), at(i + 1), u) : at(i).map((v, k) => v + (at(i + 1)[k] - v) * u);
}

/** A rig's joints from the hips down, parents first: each one's parent and rest pose, and its rest
 *  turn in the world (the armature above the hips only scales). */
function skeleton(doc) {
  const skin = doc.getRoot().listSkins()[0];
  const all = new Set(skin.listJoints());
  const joints = new Map();
  const visit = (node, parent) => {
    const r = node.getRotation();
    const world = parent ? qmul(joints.get(parent).world, r) : r;
    joints.set(node.getName(), { node, parent, r, t: node.getTranslation(), world });
    for (const c of node.listChildren()) if (all.has(c)) visit(c, node.getName());
  };
  visit([...all].find((j) => j.getName() === 'Hips'), null);
  return joints;
}

/** Adds `anim` (made on rig `src`) to `doc` (rig `dst`) as `name`, retargeted: its window
 *  [from, to] (s), keyed at its own times within it, from 0. `src` may be `dst` (a clip of its
 *  own). */
function retarget(anim, src, doc, dst, name, [from, to]) {
  let keys = [];
  for (const c of anim.listChannels()) {
    const t = c.getSampler().getInput().getArray();
    if (t.length > keys.length) keys = Array.from(t);
  }
  const t0 = keys[0] + from;
  const times = [t0, ...keys.filter((t) => t > t0 + 1e-4 && t < keys[0] + to - 1e-4), keys[0] + to];
  const names = [...dst.keys()].filter((n) => src.has(n));
  const curves = new Map(anim.listChannels().map((c) => [`${c.getTargetNode().getName()}.${c.getTargetPath()}`, c]));
  const rots = new Map(names.map((n) => [n, []]));
  const hips = [];
  const [sHips, dHips] = [src.get('Hips'), dst.get('Hips')];
  const k = dHips.t[1] / sHips.t[1];
  for (const t of times) {
    const sWorld = new Map();
    const dWorld = new Map();
    for (const n of names) {
      const s = src.get(n);
      const d = dst.get(n);
      const c = curves.get(`${n}.rotation`);
      const local = c ? sample(c.getSampler(), t, true) : s.r;
      const sw = s.parent ? qmul(sWorld.get(s.parent), local) : local;
      sWorld.set(n, sw);
      // The bone's turn from rest, in the world, on the other body's rest.
      const dw = qnorm(qmul(qmul(sw, qinv(s.world)), d.world));
      dWorld.set(n, dw);
      rots.get(n).push(...(d.parent ? qnorm(qmul(qinv(dWorld.get(d.parent)), dw)) : dw));
    }
    const c = curves.get('Hips.translation');
    const st = c ? sample(c.getSampler(), t, false) : sHips.t;
    // Up and down only, scaled by hip height.
    hips.push(dHips.t[0], dHips.t[1] + (st[1] - sHips.t[1]) * k, dHips.t[2]);
  }
  const buffer = doc.getRoot().listBuffers()[0];
  const input = doc
    .createAccessor()
    .setType('SCALAR')
    .setArray(new Float32Array(times.map((t) => t - t0)))
    .setBuffer(buffer);
  const out = doc.createAnimation(name);
  const add = (node, target, type, values) => {
    const s = doc
      .createAnimationSampler()
      .setInput(input)
      .setInterpolation('LINEAR')
      .setOutput(doc.createAccessor().setType(type).setArray(new Float32Array(values)).setBuffer(buffer));
    out.addSampler(s).addChannel(doc.createAnimationChannel().setTargetNode(node).setTargetPath(target).setSampler(s));
  };
  for (const n of names) add(dst.get(n).node, 'rotation', 'VEC4', rots.get(n));
  add(dHips.node, 'translation', 'VEC3', hips);
  return out;
}

/** A prop's mesh: stood up along +Y with its grip at the origin, `length` long (of a champion 1
 *  tall), each vertex coloured by the band its place along that length falls in; the bands that
 *  glow in a primitive of their own, with an emissive material. */
async function prop(io, doc, name, def, file) {
  const src = await io.read(file);
  const prim = src.getRoot().listMeshes()[0].listPrimitives()[0];
  // By place alone, so its normals come out smooth.
  for (const a of prim.listSemantics()) if (a !== 'POSITION') prim.setAttribute(a, null);
  await src.transform(weld());
  const pos = prim.getAttribute('POSITION').getArray();
  const idx = prim.getIndices().getArray();
  const n = pos.length / 3;
  // Its length is its longest side.
  const lo = [Infinity, Infinity, Infinity];
  const hi = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < n; i++)
    for (let a = 0; a < 3; a++) {
      lo[a] = Math.min(lo[a], pos[3 * i + a]);
      hi[a] = Math.max(hi[a], pos[3 * i + a]);
    }
  const axis = [0, 1, 2].reduce((m, a) => (hi[a] - lo[a] > hi[m] - lo[m] ? a : m), 0);
  const len = hi[axis] - lo[axis];
  const along = (i) => {
    const u = (pos[3 * i + axis] - lo[axis]) / len;
    return def.flip ? 1 - u : u;
  };
  const band = (u) => def.bands.findIndex((b) => u <= b[0] + 1e-6);
  const lin = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  const scale = def.length / len;
  const others = [0, 1, 2].filter((a) => a !== axis);
  const mid = others.map((a) => (lo[a] + hi[a]) / 2);
  const at = new Float32Array(n * 3);
  const colors = new Uint8Array(n * 4);
  for (let i = 0; i < n; i++) {
    const u = along(i);
    at.set(
      [(pos[3 * i + others[0]] - mid[0]) * scale, (u - def.grip) * def.length, (pos[3 * i + others[1]] - mid[1]) * scale],
      3 * i,
    );
    const rgb = def.bands[Math.max(0, band(u))][1].match(/[0-9a-f]{2}/g).map((h) => lin(parseInt(h, 16) / 255));
    colors.set([...rgb.map((c) => Math.round(c * 255)), 255], 4 * i);
  }
  // Normals from the faces.
  const nrm = new Float32Array(n * 3);
  for (let f = 0; f < idx.length; f += 3) {
    const [a, b, c] = [idx[f], idx[f + 1], idx[f + 2]].map((i) => [at[3 * i], at[3 * i + 1], at[3 * i + 2]]);
    const e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    const e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    const fn = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
    for (const i of [idx[f], idx[f + 1], idx[f + 2]]) for (let k = 0; k < 3; k++) nrm[3 * i + k] += fn[k];
  }
  for (let i = 0; i < n; i++) {
    const l = Math.hypot(nrm[3 * i], nrm[3 * i + 1], nrm[3 * i + 2]) || 1;
    for (let k = 0; k < 3; k++) nrm[3 * i + k] /= l;
  }
  const buffer = doc.getRoot().listBuffers()[0];
  const acc = (type, arr) => doc.createAccessor().setType(type).setArray(arr).setBuffer(buffer);
  const [position, normal, color] = [acc('VEC3', at), acc('VEC3', nrm), acc('VEC4', colors).setNormalized(true)];
  const mesh = doc.createMesh(name);
  // A triangle glows when its first corner's band does.
  const groups = new Map();
  for (let f = 0; f < idx.length; f += 3) {
    const glow = def.bands[Math.max(0, band(along(idx[f])))][2] ?? null;
    const key = JSON.stringify(glow);
    if (!groups.has(key)) groups.set(key, { glow, tris: [] });
    groups.get(key).tris.push(idx[f], idx[f + 1], idx[f + 2]);
  }
  for (const { glow, tris } of groups.values()) {
    const mat = doc
      .createMaterial(glow ? `${name}.glow` : name)
      .setRoughnessFactor(def.metal ? 0.45 : 0.8)
      .setMetallicFactor(def.metal ? 0.5 : 0)
      .setEmissiveFactor(glow ?? [0, 0, 0])
      .setDoubleSided(true);
    mesh.addPrimitive(
      doc
        .createPrimitive()
        .setAttribute('POSITION', position)
        .setAttribute('NORMAL', normal)
        .setAttribute('COLOR_0', color)
        .setIndices(acc('SCALAR', new Uint16Array(tris)))
        .setMaterial(mat),
    );
  }
  return mesh;
}

/** Hangs `mesh` on the joint `def.bone` so that at rest it points `def.dir` (in the champion's
 *  space: +Y up, +Z ahead), turned `def.roll` degrees about its length and moved by `def.offset`
 *  (of the champion's height). `scale` is the armature's: the champion is `1 / scale` of its
 *  units tall. */
function hold(doc, joints, scale, mesh, def) {
  const bone = joints.get(def.bone);
  // +Y (the prop's length) turned onto dir, then about it.
  const d = qnorm([...def.dir, 0]).slice(0, 3);
  const c = d[1];
  let want = c < -0.9999 ? [1, 0, 0, 0] : qnorm([d[2], 0, -d[0], 1 + c]);
  const roll = ((def.roll ?? 0) * Math.PI) / 360;
  want = qmul(want, [0, Math.sin(roll), 0, Math.cos(roll)]);
  // In the bone's frame, its rest turn undone: gripped in the palm (a little way along a hand
  // bone), then moved by offset.
  const palm = def.bone.endsWith('Hand') ? [0, PALM / scale, 0] : [0, 0, 0];
  const off = def.offset ? qrot(qinv(bone.world), def.offset.map((v) => v / scale)) : [0, 0, 0];
  bone.node.addChild(
    doc
      .createNode(def.prop)
      .setMesh(mesh)
      .setTranslation(palm.map((v, i) => v + off[i]))
      .setRotation(qnorm(qmul(qinv(bone.world), want)))
      .setScale([1 / scale, 1 / scale, 1 / scale]),
  );
}

/** The skin's weights as bytes (each vertex's four still summing to one) and its texture
 *  coordinates as 16-bit integers: glTF's own normalised types, which Bevy widens as it loads. */
function packSkin(prim) {
  const w = prim.getAttribute('WEIGHTS_0');
  const f = w.getArray();
  const b = new Uint8Array(f.length);
  for (let v = 0; v < f.length; v += 4) {
    const q = [0, 1, 2, 3].map((k) => Math.round(f[v + k] * 255));
    const big = [0, 1, 2, 3].reduce((m, k) => (q[k] > q[m] ? k : m), 0);
    q[big] += 255 - q.reduce((a, c) => a + c, 0);
    b.set(q, v);
  }
  w.setArray(b).setNormalized(true);
  const uv = prim.getAttribute('TEXCOORD_0');
  const u = uv.getArray();
  if (u.every((x) => x >= 0 && x <= 1)) uv.setArray(Uint16Array.from(u, (x) => Math.round(x * 65535))).setNormalized(true);
}

/** Rotations as normalised 16-bit integers: half the bytes, a ten-thousandth of a turn. */
function shortRotations(doc) {
  for (const anim of doc.getRoot().listAnimations())
    for (const ch of anim.listChannels()) {
      if (ch.getTargetPath() !== 'rotation') continue;
      const out = ch.getSampler().getOutput();
      const f = out.getArray();
      const s = new Int16Array(f.length);
      for (let i = 0; i < f.length; i++) s[i] = Math.round(Math.max(-1, Math.min(1, f[i])) * 32767);
      out.setArray(s).setNormalized(true);
    }
}

async function champion(io, dir, name, ref) {
  const doc = await io.read(path.join(dir, 'champions', `${name}.glb`));
  const joints = skeleton(doc);
  for (const a of doc.getRoot().listAnimations()) a.dispose();
  // The clips it plays (el_client_core::pose's kit): its own run, the rest shared.
  const window = (clip) => {
    const w = DRESS.clips[clip];
    const want = CLIPS.clips[clip].length;
    if (!w || Math.abs(w[1] - w[0] - want) > 0.02) throw new Error(`${clip}'s window isn't ${want} s long`);
    return w;
  };
  const plays = CLIPS.champions[name];
  const run = await io.read(path.join(dir, 'champions', `${name}.run.glb`));
  retarget(run.getRoot().listAnimations()[0], skeleton(run), doc, joints, 'run', window('run'));
  for (const [set, s] of Object.entries(TASKS.animations)) {
    const anims = (await io.read(path.join(dir, 'champions', `${set}.anim.glb`))).getRoot().listAnimations();
    if (anims.length !== s.clips.length) throw new Error(`${set}: ${anims.length} clips, not ${s.clips.length}`);
    // Meshy keeps the order asked for.
    s.clips.forEach(([clip], i) => plays.includes(clip) && retarget(anims[i], ref, doc, joints, clip, window(clip)));
  }
  // Stood 1 tall. The skin is bound at rest, so the mesh's own highest vertex is the champion's
  // height as it stands (Meshy's vertices are metres; its bones are centimetres, under an armature
  // scaled by a hundredth, undone by the inverse bind matrices).
  const skinned = doc
    .getRoot()
    .listNodes()
    .find((n) => n.getSkin());
  const prim = skinned.getMesh().listPrimitives()[0];
  let top = 0;
  const p = prim.getAttribute('POSITION');
  for (let i = 0; i < p.getCount(); i++) top = Math.max(top, p.getElement(i, [0, 0, 0])[1]);
  const armature = doc.getRoot().listScenes()[0].listChildren()[0];
  const scale = armature.getScale()[0] / top;
  armature.setScale([scale, scale, scale]);
  // No normal map: no tangents.
  if (!prim.getMaterial()?.getNormalTexture()) prim.setAttribute('TANGENT', null);
  for (const def of DRESS.held[name] ?? []) {
    const mesh = await prop(io, doc, def.prop, DRESS.props[def.prop], path.join(dir, 'props', `${def.prop}.glb`));
    hold(doc, joints, scale, mesh, def);
  }
  await doc.transform(
    weld(),
    // About Riot's six thousand triangles a champion (Garen's 6,336).
    simplify({ simplifier: MeshoptSimplifier, ratio: TRIANGLES / (prim.getIndices().getCount() / 3), error: 0.002 }),
    resample({ tolerance: 1e-4 }),
    dedup(),
    prune(),
    textureCompress({ encoder: sharp, targetFormat: 'jpeg', resize: [512, 512], quality: 85 }),
  );
  packSkin(skinned.getMesh().listPrimitives()[0]);
  shortRotations(doc);
  const file = path.join(OUT, `${name}.glb`);
  await io.write(file, doc);
  const tris = doc
    .getRoot()
    .listMeshes()
    .flatMap((m) => m.listPrimitives())
    .reduce((n, p) => n + p.getIndices().getCount() / 3, 0);
  const clips = doc.getRoot().listAnimations().length;
  console.log(`${name} ${Math.round(fs.statSync(file).size / 1024)} KiB, ${tris} triangles, ${clips} clips`);
}

const [dir, ...names] = process.argv.slice(2);
if (!dir) {
  console.error('node art/tools/champions.mjs <dir> [name ...]');
  process.exit(1);
}
fs.mkdirSync(OUT, { recursive: true });
const io = new NodeIO();
const refName = Object.values(TASKS.animations)[0].rig;
const ref = skeleton(await io.read(path.join(dir, 'champions', `${refName}.glb`)));
for (const name of names.length ? names : Object.keys(TASKS.champions)) await champion(io, dir, name, ref);
