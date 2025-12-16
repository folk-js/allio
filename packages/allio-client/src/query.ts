/**
 * Query engine for declarative data extraction from accessibility trees.
 *
 * Operates synchronously on the Allio client's cached element map.
 *
 * @example
 * ```ts
 * // Get all listitems under a tree
 * const items = query(allio, treeId, { selector: "tree > listitem" });
 *
 * // Extract structured data from listitems
 * const todos = query(allio, treeId, {
 *   selector: "tree > listitem",
 *   extract: { completed: "checkbox", text: "textfield" }
 * });
 * // Returns: [{ completed: true, text: "Buy milk" }, ...]
 * ```
 */

import type { Allio } from "./allio";
import type { AX, TypedElement } from "./types";

/** Selector combinator type */
type Combinator = ">" | " ";

/** Parsed selector step */
interface SelectorStep {
  role: string;
  combinator: Combinator | null; // null for first step
}

/** Query options */
export interface QueryOptions {
  /** CSS-like selector (e.g., "tree > listitem", "menu menuitem") */
  selector: string;
  /** Optional field extraction map: { fieldName: roleSelector } */
  extract?: Record<string, string>;
}

/** Query result with extracted fields */
export type ExtractedResult = Record<string, unknown> & {
  /** The matched element */
  element: TypedElement;
};

/**
 * Parse a CSS-like selector string into steps.
 *
 * Supports:
 * - Role names: "tree", "listitem", "textfield"
 * - Direct child: "parent > child"
 * - Descendant: "ancestor descendant"
 *
 * @example
 * parseSelector("tree > listitem") // [{role: "tree", combinator: null}, {role: "listitem", combinator: ">"}]
 * parseSelector("menu menuitem")   // [{role: "menu", combinator: null}, {role: "menuitem", combinator: " "}]
 */
function parseSelector(selector: string): SelectorStep[] {
  const steps: SelectorStep[] = [];
  const tokens = selector.trim().split(/\s+/);

  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];

    if (token === ">") {
      // Next token is a direct child
      continue;
    }

    const prevToken = i > 0 ? tokens[i - 1] : null;
    const combinator: Combinator | null =
      steps.length === 0 ? null : prevToken === ">" ? ">" : " ";

    steps.push({ role: token.toLowerCase(), combinator });
  }

  return steps;
}

/**
 * Check if an element matches a role.
 * Note: element.role is already lowercase in TypedElement.
 */
function matchesRole(element: TypedElement, role: string): boolean {
  return element.role === role;
}

/**
 * Get all descendants of an element (breadth-first).
 */
function getDescendants(allio: Allio, elementId: AX.ElementId): TypedElement[] {
  const result: TypedElement[] = [];
  const queue: AX.ElementId[] = [elementId];
  const visited = new Set<AX.ElementId>();

  while (queue.length > 0) {
    const id = queue.shift()!;
    if (visited.has(id)) continue;
    visited.add(id);

    const element = allio.elements.get(id);
    if (!element) continue;

    // Don't include the starting element in descendants
    if (id !== elementId) {
      result.push(element);
    }

    // Add children to queue
    if (element.children) {
      for (const childId of element.children) {
        if (!visited.has(childId)) {
          queue.push(childId);
        }
      }
    }
  }

  return result;
}

/**
 * Get direct children of an element.
 */
function getDirectChildren(
  allio: Allio,
  elementId: AX.ElementId
): TypedElement[] {
  const element = allio.elements.get(elementId);
  if (!element?.children) return [];

  return element.children
    .map((id) => allio.elements.get(id))
    .filter((e): e is TypedElement => e !== undefined);
}

/**
 * Find all elements matching a selector starting from a root.
 */
function findMatches(
  allio: Allio,
  rootId: AX.ElementId,
  steps: SelectorStep[]
): TypedElement[] {
  if (steps.length === 0) return [];

  const root = allio.elements.get(rootId);
  if (!root) return [];

  // First step must match root
  if (!matchesRole(root, steps[0].role)) {
    return [];
  }

  // If only one step, root is the only match
  if (steps.length === 1) {
    return [root];
  }

  // Process remaining steps
  let currentMatches: TypedElement[] = [root];

  for (let i = 1; i < steps.length; i++) {
    const step = steps[i];
    const nextMatches: TypedElement[] = [];

    for (const element of currentMatches) {
      const candidates =
        step.combinator === ">"
          ? getDirectChildren(allio, element.id)
          : getDescendants(allio, element.id);

      for (const candidate of candidates) {
        if (matchesRole(candidate, step.role)) {
          nextMatches.push(candidate);
        }
      }
    }

    currentMatches = nextMatches;
  }

  return currentMatches;
}

/**
 * Find first descendant matching a role.
 */
function findDescendantByRole(
  allio: Allio,
  elementId: AX.ElementId,
  role: string
): TypedElement | undefined {
  const descendants = getDescendants(allio, elementId);
  return descendants.find((d) => matchesRole(d, role));
}

/**
 * Extract the "value" from an element based on its role.
 * Unwraps the value envelope ({ type, value }) to return the primitive.
 */
function extractValue(element: TypedElement): unknown {
  // For checkboxes, return boolean (value is { type: "Boolean", value: boolean })
  if (element.role === "checkbox") {
    return element.value?.value === true;
  }

  // For text fields, return the inner value or label (value is { type: "String", value: string })
  if (element.role === "textfield") {
    return element.value?.value ?? element.label ?? "";
  }

  // For static text, use label (statictext has no value type)
  if (element.role === "statictext") {
    return element.label ?? "";
  }

  // For other roles with values, unwrap the envelope
  // Cast to access .value since TypeScript can't narrow all role unions
  const val = element.value as { value: unknown } | null;
  return val?.value ?? element.label ?? null;
}

/**
 * Query the cached element tree for matching elements.
 *
 * @param allio - Allio client instance
 * @param rootId - Root element to query from
 * @param options - Query options
 * @returns Array of matched elements (with optional extracted fields)
 *
 * @example
 * ```ts
 * // Simple selector
 * const items = query(allio, treeId, { selector: "tree > listitem" });
 *
 * // With field extraction
 * const todos = query(allio, treeId, {
 *   selector: "tree > listitem",
 *   extract: { completed: "checkbox", text: "textfield" }
 * });
 * ```
 */
export function query(
  allio: Allio,
  rootId: AX.ElementId,
  options: QueryOptions
): ExtractedResult[] {
  const steps = parseSelector(options.selector);
  const matches = findMatches(allio, rootId, steps);

  // If no extraction, return matches with just the element
  if (!options.extract) {
    return matches.map((element) => ({ element }));
  }

  // Extract fields from each match
  return matches.map((element) => {
    const result: ExtractedResult = { element };

    for (const [fieldName, roleSelector] of Object.entries(options.extract!)) {
      const descendant = findDescendantByRole(
        allio,
        element.id,
        roleSelector.toLowerCase()
      );
      result[fieldName] = descendant ? extractValue(descendant) : null;
    }

    return result;
  });
}

/**
 * Convenience function to query with automatic typing.
 *
 * @example
 * ```ts
 * interface TodoItem {
 *   completed: boolean;
 *   text: string;
 * }
 *
 * const todos = queryAs<TodoItem>(allio, treeId, {
 *   selector: "tree > listitem",
 *   extract: { completed: "checkbox", text: "textfield" }
 * });
 * ```
 */
export function queryAs<T extends Record<string, unknown>>(
  allio: Allio,
  rootId: AX.ElementId,
  options: QueryOptions
): (T & { element: TypedElement })[] {
  return query(allio, rootId, options) as (T & { element: TypedElement })[];
}
