// X-ray. The window under the cursor has a hole punched through it at the pointer, with a rounded
// rim. Two concentric circles define the effect:
//
//   outside the outer circle   the window, untouched
//   between the circles        the window's surface curving over the edge of the hole: flat at the
//                              outer circle, steeper and steeper, until it runs straight down
//                              away from the viewer at the inner circle
//   inside the inner circle    the hole: what is behind the window
//
// The rim is a quarter circle in profile, `thickness` points wide. The window's own pixels are
// drawn on it, lit from the top-left and warped the way a texture is when it follows a surface
// that turns away from the viewer: squeezed toward the hole, by arc length. Everything drawn is
// opaque.
//
// Two captures are used: `screen` is the screen as it is, `behind` is the screen without the
// window (the page asks the host to leave the hovered window out of it). `windows` is bound by
// the host (see aura.wgsl). `radius` is the radius of the hole.

const N: i32 = 24;
// A little tighter than the system's window corners, so the hole always covers the window's own
// rounded edge.
const CORNER: f32 = 8.0;
// How far the hole extends past the window's edge, to cover the 1px rim the system draws there.
const OVERCUT: f32 = 1.5;
// Direction toward the light (x right, y down, z toward the viewer), unit length: from the
// top-left, and in front.
const LIGHT: vec3f = vec3f(-0.55, -0.55, 0.63);
const SHININESS: f32 = 24.0;
// Light that reaches surfaces facing away from the light.
const AMBIENT: f32 = 0.62;

fn rounded_box(p: vec2f, r: vec4f) -> f32 {
  let h = r.zw * 0.5;
  let q = abs(p - (r.xy + h)) - h + vec2f(CORNER);
  return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - CORNER;
}

fn uv_of(point: vec2f) -> vec2f {
  return (point - u.region.xy) / u.region.zw;
}

@fragment fn fs(in: VsOut) -> @location(0) vec4f {
  let p = u.region.xy + in.uv * u.region.zw;

  // The frontmost window under the cursor.
  var plate = vec4f(0.0);
  for (var i = 0; i < N; i++) {
    let r = u.windows[i];
    if (r.z <= 0.0) { break; }
    if (rounded_box(u.mouse, r) < 0.0) { plate = r; break; }
  }
  if (plate.z <= 0.0) { return vec4f(0.0); }

  let rim = max(u.thickness, 1.0);
  let to_centre = u.mouse - p;
  let dist = length(to_centre);
  let inward = to_centre / max(dist, 0.001);
  let inner = u.radius;
  let outer = u.radius + rim;
  if (dist >= outer) { return vec4f(0.0); }

  let d_plate = rounded_box(p, plate);
  if (d_plate > OVERCUT) { return vec4f(0.0); }

  // How much of this pixel is hole (crisp, one point of anti-aliasing) and how much is window.
  let hole = clamp(0.5 - (dist - inner), 0.0, 1.0) * clamp(0.5 - (d_plate - OVERCUT), 0.0, 1.0);
  let on_plate = clamp(0.5 - d_plate, 0.0, 1.0);

  // Where we are on the rim's quarter circle: x runs from 0 at the outer circle to 1 at the inner
  // one, and the surface normal tilts from straight up to sideways, toward the middle of the hole.
  let x = clamp((outer - dist) / rim, 0.0, 1.0);
  let normal = vec3f(inward * x, sqrt(max(1.0 - x * x, 0.0)));

  // Where the window's texture comes from. `warp` 0 is the plain window as seen from above; 1
  // wraps it onto the curved surface (squeezed toward the hole, by arc length); -1 is the reverse.
  let along = mix(x * rim, rim * asin(x), u.warp);
  let source = u.mouse - inward * (outer - along);
  let inside = rounded_box(source, plate) < -1.0;
  let window = textureSampleLevel(screen, samp, uv_of(select(p, source, inside)), 0.0).rgb;

  // Light it: a little ambient plus diffuse, normalised so the flat window is unchanged. Where the
  // surface turns edge-on to the viewer it also catches light on every side (grazing reflection),
  // which is what makes the lip read as rolling over the edge.
  let diffuse = clamp(dot(normal, LIGHT), 0.0, 1.0) / LIGHT.z;
  let lit = AMBIENT + (1.0 - AMBIENT) * diffuse;
  let halfway = normalize(LIGHT + vec3f(0.0, 0.0, 1.0));
  let shine = max(pow(max(dot(normal, halfway), 0.0), SHININESS) - pow(halfway.z, SHININESS), 0.0);
  let grazing = pow(1.0 - normal.z, 3.0);
  let rim_colour = window * lit + vec3f(shine * 0.5 + grazing * 0.4);

  // The hole: what is behind the window, straight through.
  let floor_colour = textureSampleLevel(behind, samp, in.uv, 0.0).rgb;

  let ring = mix(floor_colour, rim_colour, on_plate);
  let colour = mix(ring, floor_colour, hole);
  let alpha = max(hole, on_plate);
  return vec4f(colour * alpha, alpha);
}
