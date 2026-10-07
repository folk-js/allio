/**
 * Screen shaders: capture a screen region and draw it back through a WGSL fragment shader,
 * natively on the GPU.
 *
 * The client owns shader state and the server mirrors it, the reverse of windows and elements.
 * The complete set is pushed whenever it changes and whenever the socket opens. The server drops
 * a client's shaders when its connection closes.
 *
 *   const fx = allio.shader({
 *     region: { x: 100, y: 100, w: 800, h: 500 },
 *     uniforms: { radius: "f32" },
 *     wgsl: `@fragment fn fs(in: VsOut) -> @location(0) vec4f {
 *       let d = distance(u.region.xy + in.uv * u.region.zw, u.mouse);
 *       return textureSample(screen, samp, in.uv) * smoothstep(u.radius, 0.0, d);
 *     }`,
 *   });
 *   fx.set({ radius: 160 });
 *
 * The shader sees the region as `screen` (sampled with `samp`) and these built-ins, filled
 * natively each frame: `u.resolution` (px), `u.time` (s), `u.mouse` (screen points), `u.region`
 * (the region: x, y, w, h in screen points). Updates are latest-wins: all `set()` calls within
 * one animation frame are sent as one message, and the native side overwrites instead of queueing.
 */
import type { Hide } from "./types/generated/Hide";
import type { Region } from "./types/generated/Region";
import type { ShaderSpec } from "./types/generated/ShaderSpec";
import type { Source } from "./types/generated/Source";

export type { Hide, Region, Source };

export type UniformType = ShaderSpec["uniforms"][string];

export type UniformValue<T extends UniformType> = T extends "f32"
  ? number
  : T extends "vec2f"
  ? [number, number]
  : T extends "vec3f"
  ? [number, number, number]
  : T extends "vec4f"
  ? [number, number, number, number]
  : number[]; // vec4f[N]: 4N flat numbers

/** Declared uniforms: name to type. */
export type Uniforms = Record<string, UniformType>;
export type UniformValues<U extends Uniforms> = { [K in keyof U]: UniformValue<U[K]> };

export interface ShaderOptions<U extends Uniforms> {
  /** WGSL containing `@fragment fn fs(in: VsOut) -> @location(0) vec4f`. */
  wgsl: string;
  /** Screen region (points, top-left origin) to capture and draw over. */
  region: Region;
  uniforms?: U;
  /** Initial uniform values; anything unset is zero. */
  values?: Partial<UniformValues<U>>;
  /** Windows to leave out of `screen`, to see what is behind them. Default: none. */
  hide?: Hide;
  /** Screen points per cell of the `state` texture, for shaders that define `sim`. Default 4. */
  cell?: number;
  /** Simulation steps per frame (1 to 8), so a finer `cell` can keep the same speed. Default 1. */
  steps?: number;
  /**
   * A second capture of the screen with these windows left out, read as the texture `behind`.
   * Use it with `screen` to see a window and what is behind it at once. Change it with `fx.behind`.
   */
  behind?: Hide;
  /**
   * More textures, by name, read in WGSL as `texture_2d<f32>`s of that name: `{ window: id }` is a
   * window's own pixels even while it's covered (declare `NAME_rect: "vec4f"` to have its rect on
   * screen kept up to date), `{ region, hide? }` part of the screen, `null` a name with nothing in
   * it yet. Change with `fx.sources`; only adding or removing names rebuilds the shader.
   */
  sources?: Record<string, Source | null>;
  /**
   * Whether to draw every frame. By default a shader draws only when something it reads changed;
   * one that reads `u.time`/`u.frame` or defines `sim` draws every frame unless this is false.
   */
  animate?: boolean;
}

type Floats = Record<string, number[]>;

const SCALARS = { f32: 1, vec2f: 2, vec3f: 3, vec4f: 4 } as const;

/** How many numbers a uniform takes: `vec4f[N]` takes 4N. */
function floatCount(type: UniformType): number {
  return type in SCALARS
    ? SCALARS[type as keyof typeof SCALARS]
    : 4 * Number(type.slice("vec4f[".length, -1));
}

export class Shader<U extends Uniforms = Uniforms> {
  /** Why the shader's definition was rejected (WGSL diagnostics, bad initial values), or null. */
  error: string | null = null;
  /** Called when `error` changes. */
  onerror?: (error: string | null) => void;

  private _wgsl: string;
  private readonly uniforms: Uniforms;
  private _region: Region;
  private _hide: Hide;
  private _behind?: Hide;
  private readonly cell?: number;
  private readonly steps?: number;
  private _sources: Record<string, Source | null>;
  private _animate?: boolean;
  private values: Floats = {};

  /** @internal Use `allio.shader()`. */
  constructor(
    readonly id: string,
    options: ShaderOptions<U>,
    private readonly owner: ShaderSet
  ) {
    this._wgsl = options.wgsl;
    this.uniforms = { ...options.uniforms };
    this._region = { ...options.region };
    this._hide = options.hide ?? "none";
    this.cell = options.cell;
    this.steps = options.steps;
    this._behind = options.behind;
    this._sources = { ...options.sources };
    this._animate = options.animate;
    if (options.values) this.store(options.values);
  }

  get wgsl(): string {
    return this._wgsl;
  }

  /**
   * Replaces the shader source. If it fails to compile the previous version keeps running and
   * `error` explains why. Uniform declarations can't change.
   */
  set wgsl(source: string) {
    this._wgsl = source;
    this.owner.changed();
  }

  get hide(): Hide {
    return this._hide;
  }

  /** Changes which windows are left out of `screen`. Cheap enough to do as the pointer moves. */
  set hide(hide: Hide) {
    this._hide = hide;
    this.owner.patch(this.id, { hide });
  }

  get behind(): Hide | undefined {
    return this._behind;
  }

  /** Changes which windows are left out of `behind`. The shader must have declared `behind`. */
  set behind(hide: Hide | undefined) {
    if (this._behind === undefined || hide === undefined)
      throw new Error(`shader ${this.id}: declare \`behind\` in the options to change it`);
    this._behind = hide;
    this.owner.patch(this.id, { behind: hide });
  }

  /**
   * Reads one cell of the simulation state (`state`) at a screen point, as `[r, g, b, a]` in
   * whatever the shader's `sim` wrote. Rejects if the shader has no `sim`.
   */
  probe(x: number, y: number): Promise<number[]> {
    return this.owner.probe(this.id, x, y);
  }

  get sources(): Record<string, Source | null> {
    return this._sources;
  }

  /** Changes what the named sources show. Cheap unless names are added or removed. */
  set sources(sources: Record<string, Source | null>) {
    this._sources = { ...sources };
    this.owner.changed();
  }

  get animate(): boolean | undefined {
    return this._animate;
  }

  /** Draw every frame (true), only on change (false), or decide from the shader (undefined). */
  set animate(animate: boolean | undefined) {
    this._animate = animate;
    this.owner.changed();
  }

  get region(): Region {
    return this._region;
  }

  set region(region: Region) {
    this._region = { ...region };
    this.owner.patch(this.id, { region: this._region });
  }

  /**
   * Overwrites uniform values. Latest wins; cheap enough to call on every pointer event.
   * Throws on an undeclared uniform or a wrong number of components.
   */
  set(values: Partial<UniformValues<U>>): void {
    this.owner.patch(this.id, { values: this.store(values) });
  }

  /** Stops the shader and removes its window. */
  dispose(): void {
    this.owner.remove(this.id);
  }

  /** Validates, remembers and returns values as flat float arrays. */
  private store(values: Partial<UniformValues<U>>): Floats {
    const floats: Floats = {};
    for (const [name, value] of Object.entries(values as Record<string, number | number[]>)) {
      const type = this.uniforms[name];
      if (!type) throw new Error(`shader ${this.id}: uniform '${name}' is not declared`);
      const flat = Array.isArray(value) ? value : [value];
      if (flat.length !== floatCount(type))
        throw new Error(`shader ${this.id}: uniform '${name}' is ${type}, got ${flat.length} numbers`);
      floats[name] = flat;
    }
    Object.assign(this.values, floats);
    return floats;
  }

  /** @internal */
  spec(): ShaderSpec {
    return {
      wgsl: this._wgsl,
      uniforms: this.uniforms,
      values: this.values,
      region: this._region,
      hide: this._hide,
      cell: this.cell,
      steps: this.steps,
      behind: this._behind,
      sources: this._sources,
      animate: this._animate,
    };
  }

  /** @internal */
  setError(error: string | null): void {
    if (error === this.error) return;
    this.error = error;
    this.onerror?.(error);
  }
}

type Rpc = (method: string, args: Record<string, unknown>) => Promise<unknown>;
type Patch = { values?: Floats; region?: Region; hide?: Hide; behind?: Hide };

/** @internal The client's shaders, and the logic that pushes them to the server. */
export class ShaderSet {
  private live = new Map<string, Shader<any>>();
  private pending = new Map<string, Patch>();
  private setChanged = false;
  private scheduled = false;
  private nextId = 0;

  constructor(private rpc: Rpc, private connected: () => boolean) {}

  create<U extends Uniforms>(options: ShaderOptions<U>): Shader<U> {
    const shader = new Shader(`s${++this.nextId}`, options, this);
    this.live.set(shader.id, shader);
    this.changed();
    return shader;
  }

  remove(id: string): void {
    this.live.delete(id);
    this.pending.delete(id);
    this.changed();
  }

  /** Something other than values or regions changed: push the complete state. */
  changed(): void {
    this.setChanged = true;
    this.schedule();
  }

  /** Queues a hot update; updates to the same shader within a frame are merged. */
  patch(id: string, patch: Patch): void {
    const merged = this.pending.get(id) ?? {};
    if (patch.values) merged.values = { ...merged.values, ...patch.values };
    if (patch.region) merged.region = patch.region;
    if (patch.hide) merged.hide = patch.hide;
    if (patch.behind) merged.behind = patch.behind;
    this.pending.set(id, merged);
    this.schedule();
  }

  probe(id: string, x: number, y: number): Promise<number[]> {
    return this.rpc("shader_probe", { id, x, y }) as Promise<number[]>;
  }

  /** Pushes the complete desired state. Called when shaders are added, removed or edited, and whenever the socket opens. */
  async sync(): Promise<void> {
    this.setChanged = false;
    this.pending.clear(); // current values and regions travel inside the specs
    if (!this.connected()) return;
    const shaders = Object.fromEntries([...this.live].map(([id, shader]) => [id, shader.spec()]));
    try {
      const { errors } = (await this.rpc("shaders_set", { shaders })) as { errors: Record<string, string> };
      for (const [id, shader] of this.live) shader.setError(errors[id] ?? null);
    } catch {
      // Not connected or timed out: state is kept, and the next open pushes it again.
    }
  }

  /**
   * One flush at the end of the current task: the full state if the set changed, otherwise only
   * patches. Not at the next animation frame: a page that moves its own elements in a frame and
   * sets shader values in the same frame would then always see the shader a frame behind.
   */
  private schedule(): void {
    if (this.scheduled) return;
    this.scheduled = true;
    queueMicrotask(() => {
      this.scheduled = false;
      if (this.setChanged) return void this.sync();
      const batch = [...this.pending];
      this.pending.clear();
      if (!this.connected()) return;
      for (const [id, patch] of batch) {
        this.rpc("shader_patch", { id, ...patch }).catch((e: Error) => console.error(`shader ${id}:`, e.message));
      }
    });
  }
}
