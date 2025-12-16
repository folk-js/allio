/*!
Role-based typed elements.

TypeScript enforces exhaustive role→value mapping.
Check `el.role === "..."` to narrow value type, or use `accepts(el, kind)` for polymorphism.

```ts
// Role-specific
if (el.role === "slider") {
  const num = el.value?.value; // number
}

// Value-type polymorphic  
if (accepts(el, "string")) {
  await allio.set(el, "hello"); // any string-accepting role
}
```
*/

import type { Element } from "./generated/Element";
import type { Role } from "./generated/Role";
import type { Color } from "./generated/Color";
import type * as AX from "./ax";

/** Value type kinds */
type ValueKind = "string" | "number" | "boolean" | "color";

/** Role → value type. TypeScript enforces all roles are mapped. */
export const ROLE_VALUES = {
  // Roles with string values
  textfield: "string",
  textarea: "string",
  searchfield: "string",
  combobox: "string",
  // Roles with boolean values
  checkbox: "boolean",
  switch: "boolean",
  radiobutton: "boolean",
  // Roles with number values
  slider: "number",
  progressbar: "number",
  stepper: "number",
  // Roles with color values
  colorwell: "color",
  // Roles without values
  application: null,
  window: null,
  document: null,
  group: null,
  scrollarea: null,
  toolbar: null,
  menu: null,
  menubar: null,
  menuitem: null,
  tab: null,
  tablist: null,
  list: null,
  listitem: null,
  table: null,
  row: null,
  column: null,
  cell: null,
  tree: null,
  treeitem: null,
  button: null,
  link: null,
  statictext: null,
  heading: null,
  image: null,
  separator: null,
  genericgroup: null,
  genericelement: null,
  unknown: null,
} as const satisfies Record<Role, ValueKind | null>;

// === Derived types ===

/** Value envelope for a value kind */
type ValueEnvelope<K extends ValueKind> = K extends "string"
  ? { type: "String"; value: string }
  : K extends "number"
  ? { type: "Number"; value: number }
  : K extends "boolean"
  ? { type: "Boolean"; value: boolean }
  : K extends "color"
  ? { type: "Color"; value: Color }
  : never;

/** Element narrowed by role */
type ElementWithRole<R extends Role> = Omit<Element, "role" | "value"> & {
  role: R;
  value: (typeof ROLE_VALUES)[R] extends ValueKind
    ? ValueEnvelope<(typeof ROLE_VALUES)[R]> | null
    : null;
};

/** Element discriminated by role. Check `el.role === "..."` to narrow value type. */
export type TypedElement = { [R in Role]: ElementWithRole<R> }[Role];

/** Element with specific role - for function parameters. */
export type ElementOfRole<R extends Role> = ElementWithRole<R>;

/** Primitive value type for a role (string/number/boolean/Color, or never if no value). */
export type PrimitiveForRole<R extends Role> =
  (typeof ROLE_VALUES)[R] extends "string"
    ? string
    : (typeof ROLE_VALUES)[R] extends "number"
    ? number
    : (typeof ROLE_VALUES)[R] extends "boolean"
    ? boolean
    : (typeof ROLE_VALUES)[R] extends "color"
    ? Color
    : never;

/** Roles that accept a specific value type. */
export type RolesWithValueType<V extends ValueKind> = {
  [R in Role]: (typeof ROLE_VALUES)[R] extends V ? R : never;
}[Role];

/** Roles that accept any value (writable roles). */
export type WritableRole = {
  [R in Role]: (typeof ROLE_VALUES)[R] extends null ? never : R;
}[Role];

// === Type guards ===

/**
 * Type guard: check if element accepts a specific value type.
 * Enables polymorphic value handling without checking specific roles.
 *
 * @example
 * if (accepts(el, "string")) {
 *   await allio.set(el, "hello"); // any string-accepting role
 * }
 * if (accepts(el, "number")) {
 *   await allio.set(el, 42); // slider, stepper, or progressbar
 * }
 */
export function accepts<V extends ValueKind>(
  el: TypedElement | AX.Element,
  kind: V
): el is ElementOfRole<RolesWithValueType<V>> {
  return ROLE_VALUES[el.role as AX.Role] === kind;
}
