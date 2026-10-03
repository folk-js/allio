// Lava that eats windows, and what it eats matters.
//
// A simulation (`sim`) runs on a grid of cells covering the screen and remembers between frames.
// Each cell holds, in `state`:
//   r  lava, from 0 (none) to 1.5; or -1 for stone, which lava turns into when it meets water
//   g  burn: how much of the window under this cell has been eaten, from 0 to 1
//   b  what the window under this cell is made of (see "Materials"); 0 if no window
//   a  1 if a window covers this cell (worked out once per step, then read by neighbours), plus
//      2 * the last `reset` value seen, so the page can wipe the simulation
//
// Windows are solid until burnt through, so lava pools on them, spills over their edges, and wears
// them away from the contact point. A burnt cell becomes empty and lava runs through.
//
// Materials. What a window is made of is read from its colour (from `screen`, which has the
// windows in it), a cell at a time:
//   fuel   white paper, green plants, red/orange things: burns fast, and catches fire from burning
//          neighbours, so a fire runs across a page much faster than lava eats it
//   water  blue things: lava touching them steams and cools into stone instead
//   coal   very dark things (text, dark backgrounds): slow to burn, glows like embers
//   metal  everything else: heats up red, orange, white-hot; conducts heat slowly
//
// The display pass (`fs`) draws each material burning in its own way, holes with a view of the bare
// desktop (`behind`), stone, and the lava itself, one cell at a time.
//
// `windows` is bound by the host (see aura.wgsl).

const N: i32 = 24;
const SPEED: f32 = 0.75;    // fraction of a possible fall taken each step: lava is slow
const SPREAD: f32 = 0.2;    // how fast neighbouring cells level out

const OPEN: i32 = 0;
const SOLID: i32 = 1;
const DRAIN: i32 = 2;

const METAL: f32 = 1.0;
const FUEL: f32 = 2.0;
const WATER: f32 = 3.0;
const COAL: f32 = 4.0;

fn cells() -> vec2i {
  return vec2i(u.state_size);
}

fn within(c: vec2i) -> bool {
  let n = cells();
  return c.x >= 0 && c.y >= 0 && c.x < n.x && c.y < n.y;
}

fn cell_point(c: vec2i) -> vec2f {
  return u.region.xy + (vec2f(c) + vec2f(0.5)) * (u.region.zw / u.state_size);
}

fn in_window(p: vec2f) -> bool {
  for (var i = 0; i < N; i++) {
    let r = u.windows[i];
    if (r.z <= 0.0) { break; }
    if (p.x >= r.x && p.x < r.x + r.z && p.y >= r.y && p.y < r.y + r.w) { return true; }
  }
  return false;
}

fn is_window(s: vec4f) -> bool {
  return (i32(s.a) & 1) == 1;
}

fn seen_reset(s: vec4f) -> f32 {
  return f32(i32(s.a) >> 1);
}

// What a cell is: free space, something solid (an intact window, or stone), or the drain below the
// screen.
fn kind(c: vec2i) -> i32 {
  if (c.y >= cells().y) { return DRAIN; }
  if (!within(c)) { return SOLID; }
  let s = textureLoad(state, c, 0);
  if (s.r < -0.5) { return SOLID; }
  if (is_window(s) && s.g < 1.0) { return SOLID; }
  return OPEN;
}

fn mass(c: vec2i) -> f32 {
  if (kind(c) != OPEN) { return 0.0; }
  return textureLoad(state, c, 0).r;
}

// Lava moving from `c` to the cell below it this step.
fn fall(c: vec2i) -> f32 {
  if (kind(c) != OPEN) { return 0.0; }
  let m = textureLoad(state, c, 0).r;
  let below = c + vec2i(0, 1);
  let k = kind(below);
  if (k == SOLID) { return 0.0; }
  if (k == DRAIN) { return m; }
  return min(m, max(0.0, 1.0 - textureLoad(state, below, 0).r)) * SPEED;
}

// What is left in `c` once it has fallen.
fn rest(c: vec2i) -> f32 {
  return mass(c) - fall(c);
}

// What is left in `c` and resting on a window or other lava, so it can only move sideways.
// Lava still in the air has nothing to level out against: it just falls.
fn resting(c: vec2i) -> f32 {
  let below = c + vec2i(0, 1);
  let k = kind(below);
  if (k == DRAIN) { return 0.0; }
  if (k == OPEN && textureLoad(state, below, 0).r < 0.95) { return 0.0; }
  return rest(c);
}

// Lava moving sideways from `a` to its neighbour `b`. Both cells compute the same number with
// opposite signs, so lava is neither created nor lost.
fn spill(a: vec2i, b: vec2i) -> f32 {
  if (kind(a) != OPEN || kind(b) != OPEN) { return 0.0; }
  return (resting(a) - resting(b)) * SPREAD;
}

// The lava and burn of a neighbouring cell (zeros off the screen).
fn contact(n: vec2i) -> vec2f {
  if (!within(n)) { return vec2f(0.0); }
  return vec2f(mass(n), textureLoad(state, n, 0).g);
}

// Whether `n` is an intact window made of water.
fn wet(n: vec2i) -> bool {
  if (!within(n)) { return false; }
  let s = textureLoad(state, n, 0);
  return is_window(s) && s.b == WATER && s.g < 1.0;
}

fn colour_at(point: vec2f) -> vec3f {
  return textureSampleLevel(screen, samp, (point - u.region.xy) / u.region.zw, 0.0).rgb;
}

// What a window is made of, from the average colour of four samples around the cell's middle.
fn material_at(p: vec2f) -> f32 {
  let h = 0.35 * (u.region.z / u.state_size.x);
  let rgb = 0.25 * (colour_at(p + vec2f(-h, -h)) + colour_at(p + vec2f(h, -h))
    + colour_at(p + vec2f(-h, h)) + colour_at(p + vec2f(h, h)));
  let lum = dot(rgb, vec3f(0.299, 0.587, 0.114));
  let spread = max(rgb.r, max(rgb.g, rgb.b)) - min(rgb.r, min(rgb.g, rgb.b));
  if (rgb.b > rgb.r * 1.25 && rgb.b > rgb.g * 1.05 && rgb.b > 0.3) { return WATER; }
  if (lum < 0.22) { return COAL; }
  let paper = lum > 0.72 && spread < 0.2;
  let plant = rgb.g > rgb.r * 1.15 && rgb.g > rgb.b * 1.15;
  let warm = rgb.r > rgb.g * 1.35 && rgb.r > rgb.b * 1.35;
  if (paper || plant || warm) { return FUEL; }
  return METAL;
}

// How fast lava burns a material, relative to the `rate` slider.
fn lava_burn(m: f32) -> f32 {
  if (m == FUEL) { return 1.6; }
  if (m == COAL) { return 0.45; }
  if (m == WATER) { return 0.4; }
  return 1.0;
}

// How much burn a cell picks up each step from a burning neighbour, regardless of lava: fire runs
// through fuel, heat creeps through metal, and coal and water hardly pass it on.
fn spread_burn(m: f32) -> f32 {
  if (m == FUEL) { return 0.12; }
  if (m == METAL) { return 0.015; }
  if (m == COAL) { return 0.004; }
  return 0.0;
}

@fragment fn sim(in: VsOut) -> @location(0) vec4f {
  let c = vec2i(in.pos.xy);
  let here = textureLoad(state, c, 0);
  let wiped = 2.0 * u.reset;

  // A new `reset` value wipes everything.
  if (u.reset != seen_reset(here)) { return vec4f(0.0, 0.0, 0.0, wiped); }

  let p = cell_point(c);
  let flag = select(0.0, 1.0, in_window(p)) + wiped;
  let tap = distance(p, u.mouse) < u.radius && u.pour > 0.5;

  // Stone stays.
  if (here.r < -0.5) { return vec4f(-1.0, here.g, 0.0, flag); }

  var lava = 0.0;
  var burn = here.g;
  var material = select(0.0, here.b, is_window(here));

  let k = kind(c);
  if (k == OPEN) {
    lava = here.r - fall(c) + fall(c + vec2i(0, -1))
      - spill(c, c + vec2i(-1, 0)) - spill(c, c + vec2i(1, 0));
    if (tap) { lava = max(lava, 1.0); }
    lava = clamp(lava, 0.0, 1.5);
    // Lava next to water cools into stone.
    if (lava > 0.02 && (wet(c + vec2i(1, 0)) || wet(c + vec2i(-1, 0)) || wet(c + vec2i(0, 1)) || wet(c + vec2i(0, -1)))) {
      return vec4f(-1.0, burn, 0.0, flag);
    }
  } else if (k == SOLID && within(c)) {
    // An intact window: lava touching it wears it away, and burning neighbours set it alight.
    material = material_at(p);
    let touching = max(
      max(contact(c + vec2i(1, 0)), contact(c + vec2i(-1, 0))),
      max(contact(c + vec2i(0, 1)), contact(c + vec2i(0, -1))));
    let from_lava = u.rate * lava_burn(material) * smoothstep(0.15, 0.6, touching.x);
    let from_fire = spread_burn(material) * smoothstep(0.5, 0.9, touching.y);
    burn = here.g + from_lava + from_fire;
    // The tap is hot: it burns through whatever is under it.
    if (tap) { burn = burn + u.rate * 6.0; }
    burn = min(burn, 1.0);
  }
  return vec4f(lava, burn, material, flag);
}

fn hash(p: vec2f) -> f32 {
  return fract(sin(dot(p, vec2f(12.9898, 78.233))) * 43758.5453);
}

fn lava_at(c: vec2i) -> f32 {
  if (!within(c)) { return 0.0; }
  return textureLoad(state, c, 0).r;
}

fn intact(c: vec2i) -> f32 {
  return select(0.0, 1.0, within(c) && kind(c) == SOLID);
}

// A window cell part-way through burning, as colour (premultiplied) and alpha.
fn burning(m: f32, g: f32, steady: f32, flicker: f32) -> vec4f {
  if (g < 0.25) {
    // Scorched, whatever it is.
    let a = select(0.3, 0.55, g > 0.12) + (steady - 0.5) * 0.15;
    return vec4f(vec3f(0.04, 0.02, 0.0) * a, a);
  }
  if (m == FUEL) {
    // Flames: flickering reds and yellows, with the odd cell of black ash.
    if (flicker < 0.2) { return vec4f(0.05, 0.03, 0.02, 1.0); }
    return vec4f(mix(vec3f(1.0, 0.25, 0.02), vec3f(1.0, 0.85, 0.25), flicker * (0.4 + g * 0.6)), 1.0);
  }
  if (m == WATER) {
    // Steam.
    let a = 0.75 * smoothstep(0.25, 0.7, g) * (0.5 + 0.5 * flicker);
    return vec4f(vec3f(0.92, 0.96, 1.0) * a, a);
  }
  if (m == COAL) {
    // Embers.
    let heat = g * g + (flicker - 0.5) * 0.12;
    return vec4f(mix(vec3f(0.18, 0.02, 0.0), vec3f(1.0, 0.4, 0.06), clamp(heat, 0.0, 1.0)), 1.0);
  }
  // Metal heats up: red, orange, then white-hot.
  let warm = mix(vec3f(0.5, 0.05, 0.0), vec3f(1.0, 0.55, 0.1), clamp((g - 0.25) * 2.0, 0.0, 1.0));
  return vec4f(mix(warm, vec3f(1.0, 0.95, 0.75), smoothstep(0.75, 1.0, g)), 1.0);
}

// Everything is drawn a whole cell at a time, for a crisp, pixelated look.
@fragment fn fs(in: VsOut) -> @location(0) vec4f {
  let cell = vec2i(clamp(floor(in.uv * u.state_size), vec2f(0.0), u.state_size - vec2f(1.0)));
  let s = textureLoad(state, cell, 0);
  let steady = hash(vec2f(cell));
  let flicker = hash(vec2f(cell) + floor(u.time * 8.0) * 17.0);

  // Premultiplied colour and alpha.
  var rgb = vec3f(0.0);
  var a = 0.0;

  if (s.r < -0.5) {
    // Stone: dark glassy rock with the odd glint.
    rgb = mix(vec3f(0.08, 0.07, 0.11), vec3f(0.24, 0.22, 0.31), steady);
    if (steady > 0.94) { rgb = vec3f(0.55, 0.55, 0.72); }
    a = 1.0;
  } else if (is_window(s)) {
    if (s.g >= 1.0) {
      // Eaten through: the desktop, with the walls of the hole lit from the top-left like a bevel.
      rgb = textureSampleLevel(behind, samp, in.uv, 0.0).rgb;
      a = 1.0;
      let facing_light = min(intact(cell + vec2i(0, 1)) + intact(cell + vec2i(1, 0)), 1.0);
      let facing_away = min(intact(cell + vec2i(0, -1)) + intact(cell + vec2i(-1, 0)), 1.0);
      rgb = rgb * (1.0 - 0.5 * facing_away);
      rgb = rgb + (vec3f(1.0) - rgb) * 0.35 * facing_light;
    } else if (s.g > 0.05) {
      let b = burning(s.b, s.g, steady, flicker);
      rgb = b.rgb;
      a = b.a;
    }
  }

  // Lava, in four flat shades with a little flicker. Thin films read as cooler crust.
  if (s.r > 0.08) {
    let level = clamp(s.r + (flicker - 0.5) * 0.35, 0.0, 1.0);
    rgb = vec3f(0.45, 0.04, 0.0);
    if (level > 0.3) { rgb = vec3f(0.85, 0.15, 0.02); }
    if (level > 0.55) { rgb = vec3f(1.0, 0.45, 0.05); }
    if (level > 0.8) { rgb = vec3f(1.0, 0.85, 0.25); }
    a = 1.0;
  } else if (s.r > -0.5) {
    // A faint glow on the cells just beside it.
    let beside = max(
      max(lava_at(cell + vec2i(1, 0)), lava_at(cell + vec2i(-1, 0))),
      max(lava_at(cell + vec2i(0, 1)), lava_at(cell + vec2i(0, -1))));
    if (beside > 0.3) {
      rgb = rgb + vec3f(0.5, 0.15, 0.0) * 0.5;
      a = max(a, 0.2);
    }
  }

  // The tap, while pouring.
  let ring = (1.0 - smoothstep(0.0, 2.0, abs(distance(u.region.xy + in.uv * u.region.zw, u.mouse) - u.radius))) * u.pour;
  rgb = rgb + vec3f(1.0, 0.5, 0.1) * ring * 0.8;
  a = max(a, ring * 0.5);
  return vec4f(rgb, a);
}
