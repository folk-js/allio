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
import type { Region } from "./types/generated/Region";
import type { ShaderSpec } from "./types/generated/ShaderSpec";

export type { Region };

export type UniformType = ShaderSpec["uniforms"][string];

export type UniformValue<T extends UniformType> = T extends "f32"
  ? number
  : T extends "vec2f"
  ? [number, number]
  : T extends "vec3f"
  ? [number, number, number]
  : [number, number, number, number];

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
}

type Floats = Record<string, number[]>;

const FLOATS: Record<UniformType, number> = { f32: 1, vec2f: 2, vec3f: 3, vec4f: 4 };

export class Shader<U extends Uniforms = Uniforms> {
  /** Why the shader's definition was rejected (WGSL diagnostics, bad initial values), or null. */
  error: string | null = null;
  /** Called when `error` changes. */
  onerror?: (error: string | null) => void;

  private _wgsl: string;
  private readonly uniforms: Uniforms;
  private _region: Region;
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
      if (flat.length !== FLOATS[type])
        throw new Error(`shader ${this.id}: uniform '${name}' is ${type}, got ${flat.length} numbers`);
      floats[name] = flat;
    }
    Object.assign(this.values, floats);
    return floats;
  }

  /** @internal */
  spec(): ShaderSpec {
    return { wgsl: this._wgsl, uniforms: this.uniforms, values: this.values, region: this._region };
  }

  /** @internal */
  setError(error: string | null): void {
    if (error === this.error) return;
    this.error = error;
    this.onerror?.(error);
  }
}

type Rpc = (method: string, args: Record<string, unknown>) => Promise<unknown>;
type Patch = { values?: Floats; region?: Region };

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
    this.pending.set(id, merged);
    this.schedule();
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

  /** One flush per animation frame: the full state if the set changed, otherwise only patches. */
  private schedule(): void {
    if (this.scheduled) return;
    this.scheduled = true;
    const nextFrame = typeof requestAnimationFrame === "function" ? requestAnimationFrame : (f: () => void) => setTimeout(f, 16);
    nextFrame(() => {
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
