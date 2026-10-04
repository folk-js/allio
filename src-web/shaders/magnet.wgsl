// Magnetic pointer highlight. The control the pointer is stuck to is lifted: its pixels are
// scaled up a touch inside a rounded plate, washed lighter, with a soft shadow beneath. The page
// animates `hl` (x, y, w, h of the control) so the plate slides from one control to the next,
// and `alpha` so it fades in and out.

const PAD: f32 = 4.0;
const CORNER: f32 = 8.0;
const LIFT: f32 = 0.06;

fn rounded_box(p: vec2f, r: vec4f, corner: f32) -> f32 {
  let h = r.zw * 0.5;
  let q = abs(p - (r.xy + h)) - h + vec2f(corner);
  return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - corner;
}

@fragment fn fs(in: VsOut) -> @location(0) vec4f {
  if (u.alpha <= 0.001 || u.hl.z <= 0.0) {
    return vec4f(0.0);
  }
  let p = u.region.xy + in.uv * u.region.zw;
  let plate = vec4f(u.hl.xy - vec2f(PAD), u.hl.zw + vec2f(2.0 * PAD));
  let corner = min(CORNER, 0.5 * min(plate.z, plate.w));
  let d = rounded_box(p, plate, corner);

  if (d < 0.0) {
    let centre = plate.xy + plate.zw * 0.5;
    let src = centre + (p - centre) / (1.0 + LIFT * u.alpha);
    var c = textureSampleLevel(screen, samp, (src - u.region.xy) / u.region.zw, 0.0).rgb;
    // Lighter towards the top, like light falling on a raised surface.
    let wash = 0.08 + 0.1 * (1.0 - (p.y - plate.y) / plate.w);
    c = mix(c, vec3f(1.0), wash);
    let a = u.alpha * clamp(-d, 0.0, 1.0);
    return vec4f(c * a, a);
  }

  // Shadow, dropped a little below the plate.
  let s = rounded_box(p - vec2f(0.0, 3.0), plate, corner);
  let shade = exp(-max(s, 0.0) / 6.0) * 0.3 * u.alpha;
  return vec4f(0.0, 0.0, 0.0, shade);
}
