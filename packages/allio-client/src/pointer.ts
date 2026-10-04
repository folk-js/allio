/**
 * Pointer fields: reshape how real mouse motion moves the pointer, and where it acts, natively,
 * on every move.
 *
 *   const field = allio.pointer({ gain: 0.5 });                // everything at half speed
 *   field.set({ targets: [{ rect, gain: 0.4, reach: 6 }] });   // sticky buttons
 *   field.set({ cuts: [{ shown, source }] });                  // part of the screen drawn elsewhere
 *   field.set({ lenses: [{ x, y, r: 150, mag: 3 }] });         // a fixed magnifier
 *   field.dispose();
 *
 * The host keeps a visual pointer that the hand moves, and puts the real cursor where the screen
 * drawn under it really is (through cuts and lenses), so the pointer always acts on what it
 * appears to be over. When the two differ the system cursor is hidden; draw the pointer at
 * `await allio.pointerState()`. Nothing is synthesised: the host rewrites real moves, so clicks,
 * drags, hover and the keyboard all behave as usual.
 *
 * Like shaders, the client owns the state: the complete set is pushed (at most once per frame)
 * whenever it changes and whenever the socket opens, and the host drops it when the connection
 * closes, so a reloaded or crashed page never leaves the cursor reshaped. Several fields combine:
 * gains multiply, the rest add up.
 */
import type { Cut } from "./types/generated/Cut";
import type { Lens } from "./types/generated/Lens";
import type { PointerSpec } from "./types/generated/PointerSpec";
import type { PointerState } from "./types/generated/PointerState";
import type { Rect } from "./types/generated/Rect";
import type { Target } from "./types/generated/Target";

export type { Cut, Lens, PointerSpec, PointerState, Rect, Target };

/** A cursor image: `src` is a data URL, sizes and the tip (`hot_x`, `hot_y`) are in points. */
export interface PointerShape {
  id: string;
  src: string;
  w: number;
  h: number;
  hot_x: number;
  hot_y: number;
}

export class Pointer {
  /** Why the host rejected the field set (e.g. no Accessibility permission), or null. */
  error: string | null = null;

  /** @internal Use `allio.pointer()`. */
  constructor(readonly id: string, private _spec: PointerSpec, private readonly owner: PointerSet) {}

  get spec(): PointerSpec {
    return this._spec;
  }

  /** Overwrites the given parts of the field. Latest wins; cheap enough to call every frame. */
  set(spec: Partial<PointerSpec>): void {
    this._spec = { ...this._spec, ...spec };
    this.owner.changed();
  }

  /** Stops reshaping the pointer. */
  dispose(): void {
    this.owner.remove(this.id);
  }
}

type Rpc = (method: string, args: Record<string, unknown>) => Promise<unknown>;

/** @internal The client's pointer fields, and the logic that pushes them to the host. */
export class PointerSet {
  private live = new Map<string, Pointer>();
  private scheduled = false;
  private nextId = 0;

  constructor(private rpc: Rpc, private connected: () => boolean) {}

  create(spec: PointerSpec): Pointer {
    const pointer = new Pointer(`p${++this.nextId}`, { ...spec }, this);
    this.live.set(pointer.id, pointer);
    this.changed();
    return pointer;
  }

  remove(id: string): void {
    this.live.delete(id);
    this.changed();
  }

  changed(): void {
    if (this.scheduled) return;
    this.scheduled = true;
    const nextFrame = typeof requestAnimationFrame === "function" ? requestAnimationFrame : (f: () => void) => setTimeout(f, 16);
    nextFrame(() => {
      this.scheduled = false;
      void this.sync();
    });
  }

  /** Where the pointer appears, and whether the page must draw it; null without a live field. */
  state(): Promise<PointerState | null> {
    return this.rpc("pointer_state", {}) as Promise<PointerState | null>;
  }

  /** The pointer's current shape as an image, when `state()` names one. */
  shape(): Promise<PointerShape | null> {
    return this.rpc("pointer_shape", {}) as Promise<PointerShape | null>;
  }

  /** Pushes the complete set. Called on change and whenever the socket opens. */
  async sync(): Promise<void> {
    if (!this.connected()) return;
    const fields = Object.fromEntries([...this.live].map(([id, p]) => [id, p.spec]));
    let error: string | null = null;
    try {
      await this.rpc("pointer_set", { fields });
    } catch (e) {
      error = (e as Error).message;
    }
    for (const p of this.live.values()) p.error = error;
  }
}
