// A window drawn deformed. `win` is the real window (x, y, w, h). Each point p of the screen shows
// the point of the real window that the map sends it to, in window-local points: first the affine
// map (`affine` = a, b, c, d and `shift.xy` = tx, ty: (x, y) goes to (a·x + b·y + tx,
// c·x + d·y + ty)), then the displacement `grid` (`dims.x` × `dims.y` points over the window grown by
// `shift.z` on every side, two points per vec4f, used when `shift.w` is 1). Where p falls outside
// the window, what was under the real window shows instead (`behind`, the screen without it), so
// nothing else is deformed. Windows in front (`above`, `dims.z` of them) are left alone.
//
// The pointer field uses the same map (allio-pointer's `Warp`): while the pointer appears over the
// deformed window, it really is over the matching point of the real one. Keep the two in step.
//
// When `dims.w` is 1, two round knobs are drawn at `knobs.xy` and `knobs.zw` (turning and
// scaling), in the same pass so they move with the window.

const MAX_ABOVE: i32 = 8;
/// macOS window corner radius, in points.
const CORNER: f32 = 10.0;
/// How far a window's own shadow reaches: covered with `behind` so the old outline doesn't linger.
const SHADOW: f32 = 48.0;

fn inside(p: vec2f, r: vec4f) -> bool {
  return p.x >= r.x && p.x < r.x + r.z && p.y >= r.y && p.y < r.y + r.w;
}

/// Signed distance from local point p to the window's rounded outline: negative inside.
fn window_distance(p: vec2f, size: vec2f) -> f32 {
  let h = size * 0.5;
  let q = abs(p - h) - h + vec2f(CORNER);
  return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - CORNER;
}

fn grid_at(k: i32) -> vec2f {
  let v = u.grid[k / 2];
  return select(v.xy, v.zw, k % 2 == 1);
}

/// The grid's displacement at window-local point p. Must match allio-pointer's `grid_offset`.
fn grid_offset(p: vec2f) -> vec2f {
  let cols = i32(u.dims.x);
  let rows = i32(u.dims.y);
  if (u.shift.w < 0.5 || cols < 2 || rows < 2) {
    return vec2f(0.0);
  }
  let last = vec2f(f32(cols - 1), f32(rows - 1));
  let margin = u.shift.z;
  let f = (p + vec2f(margin)) / (u.win.zw + vec2f(2.0 * margin)) * last;
  if (any(f < vec2f(0.0)) || any(f > last)) {
    return vec2f(0.0);
  }
  let cell = min(floor(f), last - vec2f(1.0));
  let t = f - cell;
  let k = i32(cell.y) * cols + i32(cell.x);
  let top = mix(grid_at(k), grid_at(k + 1), t.x);
  let bottom = mix(grid_at(k + cols), grid_at(k + cols + 1), t.x);
  return mix(top, bottom, t.y);
}

@fragment fn fs(in: VsOut) -> @location(0) vec4f {
  let p = u.region.xy + in.uv * u.region.zw;
  var out = deformed(p, in.uv);
  if (u.dims.w > 0.5) {
    out = chrome_knob(out, p, u.knobs.xy, 8.0);
    out = chrome_knob(out, p, u.knobs.zw, 8.0);
  }
  return out;
}

fn deformed(p: vec2f, uv: vec2f) -> vec4f {
  if (u.win.z <= 0.0) {
    return vec4f(0.0);
  }
  for (var i = 0; i < min(i32(u.dims.z), MAX_ABOVE); i++) {
    if (inside(p, u.above[i])) {
      return vec4f(0.0);
    }
  }

  let q = p - u.win.xy;
  let a = vec2f(u.affine.x * q.x + u.affine.y * q.y, u.affine.z * q.x + u.affine.w * q.y) + u.shift.xy;
  let r = a + grid_offset(a);
  let d = window_distance(r, u.win.zw);

  // Under the deformed window: what was under the real one, where the real one (or its shadow)
  // is; elsewhere the screen as it is. Then a soft shadow, so the new shape reads as a window.
  var under = vec4f(0.0);
  if (inside(p, vec4f(u.win.xy - vec2f(SHADOW), u.win.zw + vec2f(2.0 * SHADOW)))) {
    under = vec4f(textureSampleLevel(behind, samp, uv, 0.0).rgb, 1.0);
  }
  let shade = select(0.0, 0.28 * exp(-max(d, 0.0) / 12.0), d < 60.0);
  under = vec4f(under.rgb * (1.0 - shade), under.a + (1.0 - under.a) * shade);

  let real = (u.win.xy + r - u.region.xy) / u.region.zw;
  let window = vec4f(textureSampleLevel(screen, samp, real, 0.0).rgb, 1.0);
  return mix(under, window, clamp(0.5 - d, 0.0, 1.0));
}
