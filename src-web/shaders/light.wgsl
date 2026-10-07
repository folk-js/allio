// Lights out: the desktop at night, lit by global illumination. The pointer carries a lamp, and
// the brightest, most colourful bits of every window (badges, buttons, video, selections) give
// off light of their own colour. Light flows freely over the desktop, soaks into windows like
// fog (`density`, per point), bounces off them in their colours, and leaves shadows behind them.
//
// `scene` says what each 4 point cell is; the host solves the light (radiance cascades) into
// `light`. `fs` then dims the real screen by how dark it is there, so nothing is copied: the
// screen stays live, and only the lighting lags a capture behind.

const N: i32 = 24;
const CORNER: f32 = 10.0;
const LAMP_RADIUS: f32 = 8.0;
const LAMP_COLOUR: vec3f = vec3f(1.0, 0.78, 0.5);

fn rounded_box(p: vec2f, r: vec4f) -> f32 {
  let h = r.zw * 0.5;
  let q = abs(p - (r.xy + h)) - h + vec2f(CORNER);
  return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - CORNER;
}

fn in_window(p: vec2f) -> bool {
  for (var i = 0; i < N; i++) {
    let r = u.windows[i];
    if (r.z <= 0.0) { break; }
    if (rounded_box(p, r) < 0.0) { return true; }
  }
  return false;
}

// How much a colour (linear) glows: bright and saturated. Greys, white and black don't.
fn glowing(c: vec3f) -> f32 {
  let hi = max(c.r, max(c.g, c.b));
  let lo = min(c.r, min(c.g, c.b));
  return smoothstep(0.25, 0.6, hi - lo) * smoothstep(0.2, 0.5, hi);
}

fn linear(c: vec3f) -> vec3f {
  return pow(c, vec3f(2.2));
}

@fragment fn scene(in: VsOut) -> Scene {
  let p = u.region.xy + in.uv * u.region.zw;
  let colour = linear(textureSampleLevel(screen, samp, in.uv, 0.0).rgb);
  var s: Scene;
  s.emit = vec4f(0.0);
  s.matter = vec4f(0.0); // the desktop is open air
  if (in_window(p)) {
    let g = glowing(colour);
    s.matter = vec4f(colour, mix(u.density, 1.0, g));
    s.emit = vec4f(colour * g * u.glow, 0.0);
  }
  if (distance(p, u.mouse) < LAMP_RADIUS) {
    s.emit = vec4f(LAMP_COLOUR * u.lamp, 0.0);
    s.matter = vec4f(0.0, 0.0, 0.0, 1.0);
  }
  return s;
}

@fragment fn fs(in: VsOut) -> @location(0) vec4f {
  let p = u.region.xy + in.uv * u.region.zw;
  let l = textureSampleLevel(light, samp, in.uv, 0.0).rgb;
  let lit = u.ambient + (1.0 - u.ambient) * (1.0 - exp(-l * u.exposure));
  var bright = dot(lit, vec3f(0.2126, 0.7152, 0.0722));
  // What glows stays bright however dark it is around it.
  if (in_window(p)) {
    bright = max(bright, glowing(linear(textureSampleLevel(screen, samp, in.uv, 0.0).rgb)));
  }
  bright = clamp(bright, 0.0, 1.0);
  // Dim by how dark it is, and tint by the light's colour (premultiplied, over the screen).
  let tint = max(lit - vec3f(bright), vec3f(0.0)) * 0.35;
  return vec4f(tint, 1.0 - bright);
}
