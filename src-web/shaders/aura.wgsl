// Window auras. Every window gets a glowing halo whose colour drifts around its edge, pulses, and
// brightens toward the cursor; the focused window burns brighter. The halos are drawn additively
// onto the transparent overlay, so the screen itself is never copied (and never delayed).
//
// `windows` and `focused` are bound by the host: the on-screen windows' (x, y, w, h) in screen
// points, frontmost first and zero-padded, and the focused window's index in that list (or -1).

const N: i32 = 24;
const CORNER: f32 = 10.0;

// Signed distance to a window's outline: negative inside.
fn rounded_box(p: vec2f, r: vec4f) -> f32 {
  let h = r.zw * 0.5;
  let q = abs(p - (r.xy + h)) - h + vec2f(CORNER);
  return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - CORNER;
}

fn palette(t: f32) -> vec3f {
  return 0.5 + 0.5 * cos(6.28318 * (vec3f(t) + vec3f(0.0, 0.33, 0.67)));
}

@fragment fn fs(in: VsOut) -> @location(0) vec4f {
  let p = u.region.xy + in.uv * u.region.zw;

  // The frontmost window covering this pixel. Halos of windows behind it must not draw over it.
  var covering = N;
  for (var i = 0; i < N; i++) {
    let r = u.windows[i];
    if (r.z <= 0.0) { break; }
    if (rounded_box(p, r) < 0.0) { covering = i; break; }
  }

  var glow = vec3f(0.0);
  for (var i = 0; i < N; i++) {
    let r = u.windows[i];
    if (r.z <= 0.0 || i > covering) { break; }
    let d = rounded_box(p, r);
    if (d < 0.0) { continue; }

    let centre = r.xy + r.zw * 0.5;
    let angle = atan2(p.y - centre.y, p.x - centre.x);
    let hue = f32(i) * 0.13 + angle * 0.159 + u.time * 0.08;
    let near = exp(-distance(p, u.mouse) / 140.0);
    let pulse = 0.55 + 0.45 * sin(angle * 4.0 - u.time * 2.5 + f32(i));
    let boost = select(1.0, 1.7, f32(i) == u.focused);

    let halo = exp(-d / u.width) * 0.6 * pulse;
    let rim = exp(-d * 0.9) * 0.8;
    glow += palette(hue) * (halo + rim) * (0.45 + 1.4 * near) * boost * u.intensity;
  }

  glow = min(glow, vec3f(1.0));
  let strength = max(glow.r, max(glow.g, glow.b));
  return vec4f(glow, strength * 0.25);
}
