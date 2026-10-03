// Window blobs. A handful of monochrome metaballs wander over the screen. They keep a little apart
// from each other, are shoved out of windows, and are drawn toward window edges, where they cling;
// and because windows are metaballs too, a blob near a window fuses with it in a bridge of goo.
// The cursor is another ball in the same field.
//
// Each blob is one texel of `state` (its position and velocity), advanced by `sim`. The display
// pass (`fs`) draws the smooth union of all blobs and windows, outside windows only: the real
// windows are shown there.
//
// `windows` is bound by the host (see aura.wgsl).

const N: i32 = 24;
const MAX_BLOBS: i32 = 12;
const CORNER: f32 = 10.0;
// One simulation step, in seconds. Blobs move at the same speed whatever the display refresh rate
// is only if it is 60 Hz; on a faster display they simply move faster.
const DT: f32 = 0.016666;

fn rounded_box(p: vec2f, r: vec4f) -> f32 {
  let h = r.zw * 0.5;
  let q = abs(p - (r.xy + h)) - h + vec2f(CORNER);
  return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - CORNER;
}

// Direction pointing away from a window, at a point outside it.
fn away(p: vec2f, r: vec4f) -> vec2f {
  let e = vec2f(1.0, 0.0);
  let g = vec2f(
    rounded_box(p + e.xy, r) - rounded_box(p - e.xy, r),
    rounded_box(p + e.yx, r) - rounded_box(p - e.yx, r));
  return g / max(length(g), 0.0001);
}

// 1 up to `near`, easing down to 0 at `far`. (`smoothstep` with its edges swapped is undefined.)
fn falloff(near: f32, far: f32, x: f32) -> f32 {
  return 1.0 - smoothstep(near, far, x);
}

// Smooth minimum: the union of two distance fields with their meeting point rounded off over `k`.
fn smin(a: f32, b: f32, k: f32) -> f32 {
  let h = max(k - abs(a - b), 0.0) / k;
  return min(a, b) - h * h * k * 0.25;
}

fn hash(p: vec2f) -> f32 {
  return fract(sin(dot(p, vec2f(12.9898, 78.233))) * 43758.5453);
}

fn noise(p: vec2f) -> f32 {
  let i = floor(p);
  let f = fract(p);
  let s = f * f * (3.0 - 2.0 * f);
  return mix(
    mix(hash(i), hash(i + vec2f(1.0, 0.0)), s.x),
    mix(hash(i + vec2f(0.0, 1.0)), hash(i + vec2f(1.0, 1.0)), s.x),
    s.y);
}

fn radius_of(i: i32) -> f32 {
  return u.size * (0.55 + 0.9 * hash(vec2f(f32(i), 3.0)));
}

@fragment fn sim(in: VsOut) -> @location(0) vec4f {
  let c = vec2i(in.pos.xy);
  if (c.y != 0 || c.x >= MAX_BLOBS) { return vec4f(0.0); }
  let i = c.x;
  let here = textureLoad(state, c, 0);
  let seed = vec2f(f32(i), 1.0);

  // First step: scatter the blobs over the region, heading in random directions.
  if (u.frame < 1.5) {
    let start = u.region.xy + vec2f(hash(seed), hash(seed + 7.0)) * u.region.zw;
    let heading = vec2f(hash(seed + 13.0), hash(seed + 29.0)) - 0.5;
    return vec4f(start, heading * 160.0 * u.speed);
  }

  var p = here.xy;
  var v = here.zw;
  let radius = radius_of(i);
  var push = vec2f(0.0);

  // Meandering: a slowly changing random push.
  let t = u.time * 0.3 + f32(i) * 10.0;
  push += (vec2f(noise(vec2f(t, 1.0)), noise(vec2f(1.0, t))) - 0.5) * 200.0 * u.speed;

  // Windows: a blob can't sit inside one, and is drawn toward the edge of any near it.
  for (var w = 0; w < N; w++) {
    let r = u.windows[w];
    if (r.z <= 0.0) { break; }
    let d = rounded_box(p, r);
    let outward = away(p, r);
    if (d < radius * 0.3) {
      push += outward * (300.0 + (radius * 0.3 - d) * 12.0);
    } else if (d < radius * 3.0) {
      push -= outward * u.cling * 140.0 * falloff(radius * 0.9, radius * 3.0, d);
    }
  }

  // The cursor shoos them away a little.
  let from_cursor = p - u.mouse;
  let cursor_distance = max(length(from_cursor), 1.0);
  push += from_cursor / cursor_distance * 220.0 * falloff(radius, radius + 90.0, cursor_distance);

  // Other blobs: keep a little apart.
  for (var j = 0; j < MAX_BLOBS; j++) {
    if (j == i) { continue; }
    let other = textureLoad(state, vec2i(j, 0), 0).xy;
    let gap = distance(p, other);
    let wanted = (radius + radius_of(j)) * 1.1;
    if (gap < wanted && gap > 0.01) {
      push += (p - other) / gap * (wanted - gap) * 5.0;
    }
  }

  // Walls: the edges of the region.
  let low = u.region.xy + vec2f(radius);
  let high = u.region.xy + u.region.zw - vec2f(radius);
  push += max(low - p, vec2f(0.0)) * 40.0 - max(p - high, vec2f(0.0)) * 40.0;

  v = (v + push * DT) * 0.992;
  let cap = 160.0 * u.speed + 30.0;
  v = v * min(1.0, cap / max(length(v), 0.001));
  p = p + v * DT;
  return vec4f(p, v);
}

@fragment fn fs(in: VsOut) -> @location(0) vec4f {
  let p = u.region.xy + in.uv * u.region.zw;

  // The smooth union of the blobs, the cursor's ball, and the windows.
  var field = select(1.0e6, length(p - u.mouse) - u.ball, u.ball > 0.0);
  for (var i = 0; i < MAX_BLOBS; i++) {
    if (f32(i) >= u.count) { break; }
    let blob = textureLoad(state, vec2i(i, 0), 0).xy;
    field = smin(field, distance(p, blob) - radius_of(i), u.goo);
  }
  for (var w = 0; w < N; w++) {
    let r = u.windows[w];
    if (r.z <= 0.0) { break; }
    let d = rounded_box(p, r);
    if (d < 0.0) { return vec4f(0.0); }
    field = smin(field, d, u.goo);
  }

  // The surface wobbles a little.
  field = field + (noise(p * 0.04 + vec2f(u.time * 0.5, 0.0)) - 0.5) * u.wobble;

  let cover = clamp(0.5 - field, 0.0, 1.0);
  return vec4f(vec3f(u.tone) * cover, cover);
}
