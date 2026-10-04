// A fixed magnifier. `lens` is (x, y, radius, magnification). Inside `radius * FLAT` the screen is
// magnified evenly; out to the rim the magnification eases back to 1, so the lens joins the
// screen without a seam.
//
// The pointer field uses the same map (allio-pointer's `Lens`, `LENS_FLAT`): while the pointer
// appears at p, the real cursor is at the point this shader draws at p, so the pointer acts on
// exactly what it appears to be over. Keep the two in step.

const FLAT: f32 = 0.72;

@fragment fn fs(in: VsOut) -> @location(0) vec4f {
  let p = u.region.xy + in.uv * u.region.zw;
  let centre = u.lens.xy;
  let radius = u.lens.z;
  let mag = u.lens.w;
  let d = p - centre;
  let r = length(d);
  if (mag <= 1.0 || r >= radius) {
    return vec4f(0.0);
  }

  let band = smoothstep(radius * FLAT, radius, r);
  let src = centre + d * mix(1.0 / mag, 1.0, band);
  var c = textureSampleLevel(screen, samp, (src - u.region.xy) / u.region.zw, 0.0).rgb;

  // A faint vignette in the easing band and a hairline rim, so the lens reads as glass.
  c *= 1.0 - 0.1 * band;
  let rim = 1.0 - smoothstep(0.0, 1.25, abs(r - (radius - 1.5)));
  c = mix(c, vec3f(1.0), rim * 0.5);

  let a = 1.0 - smoothstep(radius - 1.0, radius, r);
  return vec4f(c * a, a);
}
