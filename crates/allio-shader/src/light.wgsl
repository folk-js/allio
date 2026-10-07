// Holographic radiance cascades (Freeman, Sannikov and Margel, 2025), ported from folkjs's
// `folk-holographic-rc.ts`. Light is solved on the grid of cells a shader's `scene` function
// wrote: `emit` (rgb light given off) and `matter` (rgb albedo, a opacity per point).
//
// Each frame, per cardinal direction: seed rays one cell long (A), extend them level by level to
// rays 2^n cells long (B), then merge cascades from the longest down to fluence at every cell
// (C). The four directions add into one fluence buffer, which is smoothed into `light` (as
// fluence / 2pi: the mean radiance arriving). Light that bounces is fed back next frame.
//
// Every global has its own binding, which is also its Metal slot (buffer or texture). Params are
// plain u32/f32 structs set with setBytes.

fn packF16(v: vec4f) -> vec2u { return vec2u(pack2x16float(v.xy), pack2x16float(v.zw)); }
fn unpackF16(p: vec2u) -> vec4f { return vec4f(unpack2x16float(p.x), unpack2x16float(p.y)); }

fn dirToSliceOffset(dirIdx: i32, intervalSize: i32) -> i32 {
  return dirIdx * 2 - intervalSize;
}

fn packRGB9E5(c: vec3f) -> u32 {
  let maxC = max(c.r, max(c.g, c.b));
  var exp_shared: i32;
  var scale: f32;
  if (maxC < 6.10352e-5) {
    exp_shared = 0;
    scale = 0.0;
  } else {
    let e = clamp(i32(ceil(log2(maxC))) + 15, 0, 31);
    exp_shared = e;
    scale = exp2(f32(-e + 15 + 9));
  }
  let r = u32(clamp(c.r * scale, 0.0, 511.0));
  let g = u32(clamp(c.g * scale, 0.0, 511.0));
  let b = u32(clamp(c.b * scale, 0.0, 511.0));
  return r | (g << 9u) | (b << 18u) | (u32(exp_shared) << 27u);
}

fn unpackRGB9E5(p: u32) -> vec3f {
  let r = f32(p & 0x1FFu);
  let g = f32((p >> 9u) & 0x1FFu);
  let b = f32((p >> 18u) & 0x1FFu);
  let e = f32((p >> 27u) & 0x1Fu) - 15.0 - 9.0;
  return vec3f(r, g, b) * exp2(e);
}

struct RayData { rad: vec3f, trans: f32 }

// Paper Eq. 7: Merge(<r_n, t_n>, <r_f, t_f>) = <r_n + t_n r_f, t_n t_f>
fn compositeRay(near: RayData, far: RayData) -> RayData {
  return RayData(near.rad + far.rad * near.trans, near.trans * far.trans);
}

const TWO_PI: f32 = 6.2831853;

@group(0) @binding(0) var emitTex: texture_2d<f32>;
@group(0) @binding(1) var matterTex: texture_2d<f32>;
@group(0) @binding(2) var bounceTex: texture_2d<f32>;

// ── A: seed, rays one cell long (paper §4.2, Alg. 1) ──
// Discrete Beer-Lambert: a cell passes (1 - opacity)^spacing of the light crossing it and gives
// off its emission (plus last frame's bounce) times what it stops.

struct SeedParams { probeCount: u32, sliceCount: u32, w: u32, h: u32, dir: u32, spacing: f32 }
@group(0) @binding(3) var<storage, read_write> seedOut: array<vec2u>;
@group(0) @binding(4) var<uniform> seedP: SeedParams;

@compute @workgroup_size(16, 16)
fn seed(@builtin(global_invocation_id) gid: vec3u) {
  let probe = i32(gid.x);
  let slice = i32(gid.y);
  if (probe >= i32(seedP.probeCount) || slice >= i32(seedP.sliceCount)) { return; }
  let w = i32(seedP.w);
  let h = i32(seedP.h);
  var px: vec2i;
  switch (seedP.dir) {
    case 1u: { px = vec2i(slice, probe); }
    case 2u: { px = vec2i(w - 1 - probe, slice); }
    case 3u: { px = vec2i(slice, h - 1 - probe); }
    default: { px = vec2i(probe, slice); }
  }
  var rad = vec3f(0.0);
  var trans = 1.0;
  if (px.x >= 0 && px.y >= 0 && px.x < w && px.y < h) {
    let opacity = textureLoad(matterTex, px, 0).a;
    trans = pow(1.0 - clamp(opacity, 0.0, 1.0), seedP.spacing);
    rad = (textureLoad(emitTex, px, 0).rgb + textureLoad(bounceTex, px, 0).rgb) * (1.0 - trans);
  }
  seedOut[slice * i32(seedP.probeCount) + probe] = packF16(vec4f(rad, trans));
}

// ── B: extend, "merge up": rays of level n from pairs of level n - 1 (paper §4.1, Eq. 18-20) ──

struct ExtendParams { probeCount: u32, level: u32, prevRayW: u32, currRayW: u32, sliceCount: u32 }
@group(0) @binding(5) var<storage, read> prevRay: array<vec2u>;
@group(0) @binding(6) var<storage, read_write> currRay: array<vec2u>;
@group(0) @binding(7) var<uniform> extendP: ExtendParams;

fn loadPrev(probeIdx: i32, rayIdx: i32, sliceIdx: i32) -> RayData {
  let prevLevel = extendP.level - 1u;
  let prevNumProbes = i32((extendP.probeCount + (1u << prevLevel) - 1u) >> prevLevel);
  let prevNumRays = i32(1u << prevLevel) + 1;
  if (probeIdx < 0 || probeIdx >= prevNumProbes ||
      rayIdx < 0 || rayIdx >= prevNumRays ||
      sliceIdx < 0 || sliceIdx >= i32(extendP.sliceCount)) {
    return RayData(vec3f(0.0), 1.0);
  }
  var idx: i32;
  if (prevLevel == 0u) {
    idx = sliceIdx * i32(extendP.prevRayW) + probeIdx;
  } else {
    idx = sliceIdx * i32(extendP.prevRayW) + (probeIdx << prevLevel) + probeIdx + rayIdx;
  }
  let r = unpackF16(prevRay[idx]);
  return RayData(r.rgb, r.a);
}

@compute @workgroup_size(16, 16)
fn extend(@builtin(global_invocation_id) gid: vec3u) {
  let texelX = i32(gid.x);
  let sliceIdx = i32(gid.y);
  let interval = i32(1u << extendP.level);
  let numRays = interval + 1;
  let levelProbes = i32((extendP.probeCount + (1u << extendP.level) - 1u) >> extendP.level);
  let probeIdx = texelX / numRays;
  let rayIdx = texelX - probeIdx * numRays;
  if (probeIdx >= levelProbes || sliceIdx >= i32(extendP.sliceCount)) { return; }

  let prevInterval = interval / 2;
  let lower = rayIdx / 2;
  let upper = (rayIdx + 1) / 2;
  let crossA = compositeRay(
    loadPrev(probeIdx * 2, lower, sliceIdx),
    loadPrev(probeIdx * 2 + 1, upper, sliceIdx + dirToSliceOffset(lower, prevInterval)),
  );
  let crossB = compositeRay(
    loadPrev(probeIdx * 2, upper, sliceIdx),
    loadPrev(probeIdx * 2 + 1, lower, sliceIdx + dirToSliceOffset(upper, prevInterval)),
  );
  currRay[sliceIdx * i32(extendP.currRayW) + texelX] =
    packF16(vec4f((crossA.rad + crossB.rad) * 0.5, (crossA.trans + crossB.trans) * 0.5));
}

// ── C: merge, "merge down": fluence from the longest cascade to the shortest (paper §4.2) ──
// Odd probes composite one interval (Eq. 14); even probes two, Richardson-averaged with the
// coarser level (Eq. 15-17). Nothing comes from beyond the screen.

struct MergeParams {
  probeCount: u32,
  levelProbes: u32,
  numRays: u32,
  isTop: u32,
  fluenceW: u32,
  fluenceH: u32,
  level: u32,
  sliceCount: u32,
  mergeStride: u32,
  mergeInWidth: u32,
  dir: u32,
}
@group(0) @binding(8) var<storage, read> rayBuf: array<vec2u>;
@group(0) @binding(9) var<storage, read> mergeIn: array<u32>;
@group(0) @binding(10) var<storage, read_write> mergeOut: array<u32>;
@group(0) @binding(11) var<uniform> mergeP: MergeParams;
@group(0) @binding(12) var<storage, read_write> fluence: array<u32>;

fn loadRay(probeIdx: i32, rayIdx: i32, sliceIdx: i32) -> RayData {
  if (probeIdx < 0 || probeIdx >= i32(mergeP.levelProbes) ||
      rayIdx < 0 || rayIdx >= i32(mergeP.numRays) ||
      sliceIdx < 0 || sliceIdx >= i32(mergeP.sliceCount)) {
    return RayData(vec3f(0.0), 1.0);
  }
  var texX: i32;
  var rowW: i32;
  if (mergeP.level == 0u) {
    texX = probeIdx;
    rowW = i32(mergeP.levelProbes);
  } else {
    texX = (probeIdx << mergeP.level) + probeIdx + rayIdx;
    rowW = i32(mergeP.levelProbes * mergeP.numRays);
  }
  let r = unpackF16(rayBuf[sliceIdx * rowW + texX]);
  return RayData(r.rgb, r.a);
}

fn loadMerge(texX: i32, sliceIdx: i32) -> vec3f {
  if (mergeP.isTop == 1u || texX < 0 || texX >= i32(mergeP.mergeInWidth) ||
      sliceIdx < 0 || sliceIdx >= i32(mergeP.sliceCount)) {
    return vec3f(0.0);
  }
  return unpackRGB9E5(mergeIn[sliceIdx * i32(mergeP.mergeStride) + texX]);
}

// Paper Eq. 13: the angle of cone `s` of the 2^(level+1) at this level.
fn coneArc(s: i32) -> f32 {
  let n = f32(2 << mergeP.level);
  let fs = f32(s);
  return atan2(2.0 * fs - n + 2.0, n) - atan2(2.0 * fs - n, n);
}

@compute @workgroup_size(16, 16)
fn merge(@builtin(global_invocation_id) gid: vec3u) {
  let probeAngIdx = i32(gid.x);
  let sliceIdx = i32(gid.y);
  let level = mergeP.level;
  let numDirections = i32(1u << level);
  let probeIdx = probeAngIdx >> level;
  let angBinIdx = probeAngIdx & (numDirections - 1);
  if (probeIdx >= i32(mergeP.levelProbes) || sliceIdx >= i32(mergeP.sliceCount)) { return; }

  let isEven = (probeIdx % 2 == 0);
  let farStep = select(1, 2, isEven);
  var result = vec3f(0.0);
  for (var side = 0; side < 2; side++) {
    let subBin = angBinIdx * 2 + side;
    let rayIdx = angBinIdx + side;
    let weight = coneArc(subBin);
    let ray = loadRay(probeIdx, rayIdx, sliceIdx);
    let sliceOff = dirToSliceOffset(rayIdx, numDirections);
    let farFluence = loadMerge(((probeIdx + farStep) << level) + subBin, sliceIdx + sliceOff * farStep);
    if (isEven) {
      let ext = loadRay(probeIdx + 1, rayIdx, sliceIdx + sliceOff);
      let cRad = ray.rad + ext.rad * ray.trans;
      let merged = cRad * weight + farFluence * (ray.trans * ext.trans);
      let coarse = loadMerge((probeIdx << level) + subBin, sliceIdx);
      result += (merged + coarse) * 0.5;
    } else {
      result += ray.rad * weight + farFluence * ray.trans;
    }
  }

  if (numDirections > 1) {
    mergeOut[sliceIdx * i32(mergeP.mergeStride) + (probeIdx << level) + angBinIdx] = packRGB9E5(result);
    return;
  }
  let fw = i32(mergeP.fluenceW);
  let fh = i32(mergeP.fluenceH);
  var fc: vec2i;
  switch (mergeP.dir) {
    case 1u: { fc = vec2i(sliceIdx, probeIdx - 1); }
    case 2u: { fc = vec2i(fw - probeIdx, sliceIdx); }
    case 3u: { fc = vec2i(sliceIdx, fh - probeIdx); }
    default: { fc = vec2i(probeIdx - 1, sliceIdx); }
  }
  if (fc.x >= 0 && fc.x < fw && fc.y >= 0 && fc.y < fh) {
    let fi = fc.y * fw + fc.x;
    fluence[fi] = packRGB9E5(unpackRGB9E5(fluence[fi]) + result);
  }
}

// ── Smoothing (paper Eq. 21) into `light` ──
// A cross blur that cancels the even/odd checkerboard, skipping neighbours across an edge in
// opacity so light doesn't leak through walls.

struct GridParams { w: u32, h: u32 }
@group(0) @binding(13) var<storage, read> fluenceIn: array<u32>;
@group(0) @binding(14) var lightOut: texture_storage_2d<rgba16float, write>;
@group(0) @binding(15) var<uniform> gridP: GridParams;

fn loadFluence(p: vec2i) -> vec3f {
  return unpackRGB9E5(fluenceIn[p.y * i32(gridP.w) + p.x]);
}

@compute @workgroup_size(16, 16)
fn blur(@builtin(global_invocation_id) gid: vec3u) {
  let p = vec2i(gid.xy);
  let dim = vec2i(i32(gridP.w), i32(gridP.h));
  if (p.x >= dim.x || p.y >= dim.y) { return; }
  let opacity = textureLoad(matterTex, p, 0).a;
  var sum = loadFluence(p) * 4.0;
  var wt = 4.0;
  let off = array<vec2i, 4>(vec2i(-1, 0), vec2i(1, 0), vec2i(0, -1), vec2i(0, 1));
  for (var i = 0; i < 4; i++) {
    let n = clamp(p + off[i], vec2i(0), dim - 1);
    if (abs(textureLoad(matterTex, n, 0).a - opacity) < 0.5) {
      sum += loadFluence(n);
      wt += 1.0;
    }
  }
  textureStore(lightOut, p, vec4f(sum / wt / TWO_PI, 1.0));
}

// ── Bounce: what last frame's light makes each cell give off ──
// Fluence inside a solid is zero, so a solid cell takes the light of the open cells around it.
// It re-emits that times its albedo, per channel, so coloured things tint what they light.

@group(0) @binding(16) var lightIn: texture_2d<f32>;
@group(0) @binding(17) var bounceOut: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(16, 16)
fn bounce(@builtin(global_invocation_id) gid: vec3u) {
  let p = vec2i(gid.xy);
  let dim = vec2i(textureDimensions(bounceOut));
  if (p.x >= dim.x || p.y >= dim.y) { return; }
  let matter = textureLoad(matterTex, p, 0);
  let albedo = clamp(matter.rgb, vec3f(0.0), vec3f(1.0));
  if (all(albedo == vec3f(0.0))) {
    textureStore(bounceOut, p, vec4f(0.0));
    return;
  }
  let own = textureLoad(lightIn, p, 0).rgb;
  var ext = vec3f(0.0);
  var extWeight = 0.0;
  let off = array<vec2i, 4>(vec2i(-1, 0), vec2i(1, 0), vec2i(0, -1), vec2i(0, 1));
  for (var i = 0; i < 4; i++) {
    let n1 = clamp(p + off[i], vec2i(0), dim - 1);
    let n2 = clamp(p + off[i] * 2, vec2i(0), dim - 1);
    let w = 1.0 - textureLoad(matterTex, n1, 0).a;
    ext += (textureLoad(lightIn, n1, 0).rgb + textureLoad(lightIn, n2, 0).rgb) * 0.5 * w;
    extWeight += w;
  }
  if (extWeight > 0.001) {
    ext /= extWeight;
  }
  let arriving = own * (1.0 - matter.a) + ext * matter.a;
  textureStore(bounceOut, p, vec4f(arriving * albedo, 0.0));
}
