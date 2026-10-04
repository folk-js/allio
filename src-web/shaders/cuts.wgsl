// WinCuts: parts of the screen drawn somewhere else, live. `cuts` holds up to MAX cuts as three
// rects each, (x, y, w, h) in screen points: where the cut is `shown`, the `source` it shows, and
// the part of the source that is `visible` (in view: not scrolled away or outside its window).
// `count` says how many are in use; later cuts are on top.
//
// Inside `shown` this draws the screen at the matching point of `source` (scaled when the sizes
// differ). Where the source is out of view it draws fog instead: the thing is there, but can't be
// seen from here. A `visible` with negative width means the source is gone altogether.
//
// The pointer field maps the visible part the same way (allio-pointer's `Cut`): there, the pointer
// acts on what it appears to be over.

const MAX: i32 = 8;
/// How far fog reaches into the visible part, in points.
const FADE: f32 = 8.0;

fn box_distance(p: vec2f, r: vec4f) -> f32 {
  let h = r.zw * 0.5;
  let q = abs(p - (r.xy + h)) - h;
  return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0);
}

fn hash(p: vec2f) -> f32 {
  var q = fract(p * vec2f(0.1031, 0.1030));
  q += dot(q, q.yx + 33.33);
  return fract((q.x + q.y) * q.x);
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

/// Slowly drifting fog, anchored to the source so it moves with it when it scrolls.
fn fog(p: vec2f) -> vec3f {
  let t = u.time;
  let n = 0.55 * noise(p / 38.0 + vec2f(t * 0.07, t * 0.03))
        + 0.3 * noise(p / 14.0 - vec2f(t * 0.11, -t * 0.05))
        + 0.15 * noise(p / 5.0 + vec2f(t * 0.2));
  return mix(vec3f(0.11, 0.12, 0.14), vec3f(0.36, 0.38, 0.42), n);
}

/// How much fog covers source point `p`: 0 well inside the visible part, 1 outside it. Only the
/// edges where the view actually cuts the source fade; edges the source shares with its visible
/// part stay crisp.
fn fogginess(p: vec2f, source: vec4f, visible: vec4f) -> f32 {
  if (visible.z <= 0.0 || visible.w <= 0.0) {
    return 1.0;
  }
  let lo = select(visible.xy, visible.xy - vec2f(FADE * 4.0), visible.xy <= source.xy + vec2f(0.5));
  let hi_v = visible.xy + visible.zw;
  let hi = select(hi_v, hi_v + vec2f(FADE * 4.0), hi_v >= source.xy + source.zw - vec2f(0.5));
  return smoothstep(-FADE, 0.0, box_distance(p, vec4f(lo, hi - lo)));
}

@fragment fn fs(in: VsOut) -> @location(0) vec4f {
  let p = u.region.xy + in.uv * u.region.zw;
  let n = min(i32(u.count), MAX);

  for (var i = n - 1; i >= 0; i--) {
    let shown = u.cuts[3 * i];
    let source = u.cuts[3 * i + 1];
    let visible = u.cuts[3 * i + 2];
    let d = box_distance(p, shown);
    if (d < 0.0) {
      let src = source.xy + (p - shown.xy) * source.zw / max(shown.zw, vec2f(1.0));
      let screen_rgb = textureSampleLevel(screen, samp, (src - u.region.xy) / u.region.zw, 0.0).rgb;
      var c = mix(screen_rgb, fog(src), fogginess(src, source, visible));
      if (visible.z < 0.0) {
        c *= 0.6; // gone, not just out of view
      }
      c = mix(vec3f(0.55), c, smoothstep(0.0, 1.0, -d)); // hairline edge
      return vec4f(c, 1.0);
    }
  }

  // A soft shadow under each cut, so it reads as lifted off the screen.
  var shade = 0.0;
  for (var i = 0; i < n; i++) {
    let d = box_distance(p - vec2f(0.0, 4.0), u.cuts[3 * i]);
    shade = max(shade, exp(-max(d, 0.0) / 10.0) * 0.35);
  }
  return vec4f(0.0, 0.0, 0.0, shade);
}
