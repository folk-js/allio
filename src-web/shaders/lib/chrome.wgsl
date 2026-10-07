// Chrome for things placed on the screen (cuts, lenses, windows), drawn in the same pass as what
// it belongs to so the two always move together. Matches the `.chrome` styles in
// overlays/demo.css: one material, one dot grip, one close cross. Pages lay invisible elements
// over it for the pointer; only the pixels come from here.
//
// Colours are straight alpha; `chrome_*` functions paint over a premultiplied `under`.

const CHROME_FILL: vec4f = vec4f(0.11, 0.11, 0.125, 0.9);
const CHROME_EDGE: vec4f = vec4f(1.0, 1.0, 1.0, 0.2);
const CHROME_INK: vec4f = vec4f(1.0, 1.0, 1.0, 0.5);

/// Signed distance to rect `r` (x, y, w, h) with corner radii (top-left, top-right,
/// bottom-right, bottom-left). Negative inside.
fn chrome_box(p: vec2f, r: vec4f, radii: vec4f) -> f32 {
  let h = r.zw * 0.5;
  let q = p - (r.xy + h);
  let radius = select(select(radii.w, radii.z, q.x > 0.0), select(radii.x, radii.y, q.x > 0.0), q.y < 0.0);
  let d = abs(q) - h + vec2f(radius);
  return length(max(d, vec2f(0.0))) + min(max(d.x, d.y), 0.0) - radius;
}

/// Paints straight-alpha `colour` over premultiplied `under`, with coverage `cover`.
fn chrome_over(under: vec4f, colour: vec4f, cover: f32) -> vec4f {
  let k = colour.a * clamp(cover, 0.0, 1.0);
  return vec4f(colour.rgb * k, k) + under * (1.0 - k);
}

/// A panel of the chrome material: fill, then a hairline edge.
fn chrome_panel(under: vec4f, p: vec2f, r: vec4f, radii: vec4f) -> vec4f {
  let d = chrome_box(p, r, radii);
  let out = chrome_over(under, CHROME_FILL, 0.5 - d);
  return chrome_over(out, CHROME_EDGE, 1.0 - abs(d + 0.5) * 2.0);
}

/// Dots on a 4 point grid, centred in `r` (or in the circle of radius `r.z` at `r.xy` when
/// `round`): "this can be grabbed".
fn chrome_grip(under: vec4f, p: vec2f, r: vec4f, round: bool) -> vec4f {
  var inside = false;
  var centre = r.xy + r.zw * 0.5;
  if (round) {
    centre = r.xy;
    inside = distance(p, centre) < r.z;
  } else {
    inside = all(p >= r.xy) && all(p < r.xy + r.zw);
  }
  if (!inside) {
    return under;
  }
  let g = (fract((p - centre) / 4.0 + 0.5) - 0.5) * 4.0;
  return chrome_over(under, CHROME_INK, 1.0 - smoothstep(0.8, 1.2, length(g)));
}

/// A thin cross `size` points across, centred at `c`; `lit` (0 to 1) brightens it.
fn chrome_cross(under: vec4f, p: vec2f, c: vec2f, size: f32, lit: f32) -> vec4f {
  let q = p - c;
  if (max(abs(q.x), abs(q.y)) > size * 0.5) {
    return under;
  }
  let d = min(abs(q.x + q.y), abs(q.x - q.y)) * 0.7071;
  return chrome_over(under, vec4f(1.0, 1.0, 1.0, mix(0.55, 1.0, lit)), 1.0 - smoothstep(0.25, 0.85, d));
}

/// A round knob of radius `radius` at `c`, with a grip.
fn chrome_knob(under: vec4f, p: vec2f, c: vec2f, radius: f32) -> vec4f {
  let d = distance(p, c) - radius;
  var out = chrome_over(under, CHROME_FILL, 0.5 - d);
  out = chrome_over(out, CHROME_EDGE, 1.0 - abs(d + 0.5) * 2.0);
  return chrome_grip(out, p, vec4f(c, radius - 3.0, 0.0), true);
}
