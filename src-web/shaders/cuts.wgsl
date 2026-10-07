// WinCuts: parts of the screen drawn somewhere else, live. `cuts` holds up to MAX cuts as four
// vec4fs each: where the cut is `shown`, the `source` it shows, and the part of the source that
// is `visible` (in view: not scrolled away or outside its window), as (x, y, w, h) in screen
// points; then `origin.x`, the window source (w0 to w7) the pixels come from, or -1 for `screen`.
// A window source shows its window even while it's covered. For those cuts `source` and
// `visible` are in the window's own points (from its top-left), so the cut doesn't depend on
// where the window is: only its size (`wK_rect.zw`) is used.
// `count` says how many cuts are in use; later cuts are on top.
//
// Inside `shown` this draws the screen at the matching point of `source` (scaled when the sizes
// differ). Where the source is out of view it draws fog instead: the thing is there, but can't be
// seen from here. A `visible` with negative width means the source is gone altogether.
//
// The pointer field maps the visible part the same way (allio-pointer's `Cut`): there, the pointer
// acts on what it appears to be over.
//
// Each cut's chrome (a bar above it to drag by, with a close cross, and a corner tab to resize
// from) is drawn here too, so it moves with the cut exactly. `ui.x` is the cut whose close cross
// is under the pointer, or -1. The page lays matching invisible elements over the chrome.

const MAX: i32 = 8;
const BAR: f32 = 18.0;
const BAR_MIN: f32 = 56.0;
/// How far fog reaches into the visible part, in points.
const FADE: f32 = 8.0;

fn box_distance(p: vec2f, r: vec4f) -> f32 {
  let h = r.zw * 0.5;
  let q = abs(p - (r.xy + h)) - h;
  return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0);
}

/// Window source `k` at point `p` of the window (in its own points, from its top-left).
fn window_at(k: i32, p: vec2f) -> vec3f {
  switch k {
    case 0: { return textureSampleLevel(w0, samp, p / max(u.w0_rect.zw, vec2f(1.0)), 0.0).rgb; }
    case 1: { return textureSampleLevel(w1, samp, p / max(u.w1_rect.zw, vec2f(1.0)), 0.0).rgb; }
    case 2: { return textureSampleLevel(w2, samp, p / max(u.w2_rect.zw, vec2f(1.0)), 0.0).rgb; }
    case 3: { return textureSampleLevel(w3, samp, p / max(u.w3_rect.zw, vec2f(1.0)), 0.0).rgb; }
    case 4: { return textureSampleLevel(w4, samp, p / max(u.w4_rect.zw, vec2f(1.0)), 0.0).rgb; }
    case 5: { return textureSampleLevel(w5, samp, p / max(u.w5_rect.zw, vec2f(1.0)), 0.0).rgb; }
    case 6: { return textureSampleLevel(w6, samp, p / max(u.w6_rect.zw, vec2f(1.0)), 0.0).rgb; }
    case 7: { return textureSampleLevel(w7, samp, p / max(u.w7_rect.zw, vec2f(1.0)), 0.0).rgb; }
    default: { return vec3f(0.0); }
  }
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
    let shown = u.cuts[4 * i];

    // Chrome: the bar above, the corner tab below.
    let bar = vec4f(shown.x, shown.y - BAR, max(shown.z, BAR_MIN), BAR);
    let tab = vec4f(shown.x + shown.z - 20.0, shown.y + shown.w, 20.0, 10.0);
    if (p.y < shown.y && chrome_box(p, bar, vec4f(5.0, 5.0, 0.0, 0.0)) < 1.0) {
      var c = chrome_panel(vec4f(0.0), p, bar, vec4f(5.0, 5.0, 0.0, 0.0));
      c = chrome_grip(c, p, vec4f(bar.x + bar.z * 0.5 - 14.0, bar.y + 5.0, 28.0, 8.0), false);
      let lit = select(0.0, 1.0, i32(u.ui.x) == i);
      return chrome_cross(c, p, vec2f(bar.x + bar.z - 9.0, bar.y + BAR * 0.5), 8.0, lit);
    }
    if (p.y >= shown.y + shown.w && chrome_box(p, tab, vec4f(0.0, 0.0, 5.0, 5.0)) < 1.0) {
      let c = chrome_panel(vec4f(0.0), p, tab, vec4f(0.0, 0.0, 5.0, 5.0));
      return chrome_grip(c, p, vec4f(tab.x + 4.0, tab.y + 1.0, 12.0, 7.0), false);
    }

    let source = u.cuts[4 * i + 1];
    let visible = u.cuts[4 * i + 2];
    let origin = i32(u.cuts[4 * i + 3].x);
    let d = box_distance(p, shown);
    if (d < 0.0) {
      let src = source.xy + (p - shown.xy) * source.zw / max(shown.zw, vec2f(1.0));
      var real = textureSampleLevel(screen, samp, (src - u.region.xy) / u.region.zw, 0.0).rgb;
      if (origin >= 0) {
        real = window_at(origin, src);
      }
      var c = mix(real, fog(src), fogginess(src, source, visible));
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
    let d = box_distance(p - vec2f(0.0, 4.0), u.cuts[4 * i]);
    shade = max(shade, exp(-max(d, 0.0) / 10.0) * 0.35);
  }
  return vec4f(0.0, 0.0, 0.0, shade);
}
