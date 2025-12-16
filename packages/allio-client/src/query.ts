/**
 * Query engine for declarative data extraction from accessibility trees.
 *
 * Operates synchronously on the Allio client's cached element map.
 *
 * ## Query Syntax
 *
 * Full syntax: `(find_selector) match_selector { field:rename field2 }`
 *
 * - `(tree)` - Find first element matching selector from root, cache it
 * - `listitem` - Match descendants of the found root
 * - `{ checkbox:done textfield:text }` - Extract values, `:` for rename
 *
 * @example
 * ```ts
 * // Find tree, get listitems, extract checkbox->completed and textfield->text
 * const todos = queryString(allio, windowRootId, "(tree) listitem { checkbox:done textfield:text }");
 *
 * // Simple selector without find
 * const items = query(allio, treeId, { selector: "listitem" });
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

/** Parsed query with find, match, and extract parts */
export interface ParsedQuery {
  /** The (selector) part - find first match from root */
  find?: string;
  /** The selector part - match under the found/given root */
  match: string;
  /** Extraction map: { fieldName: role } */
  extract?: Record<string, string>;
}

/** Query options (legacy API) */
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
 * Parse the full query syntax string.
 *
 * Syntax: `(find_selector) match_selector { field:rename field2 }`
 *
 * @example
 * parseQuerySyntax("(tree) listitem { checkbox:done textfield:text }")
 * // => { find: "tree", match: "listitem", extract: { completed: "checkbox", text: "textfield" } }
 *
 * parseQuerySyntax("tree > listitem")
 * // => { match: "tree > listitem" }
 */
export function parseQuerySyntax(input: string): ParsedQuery {
  let rest = input.trim();
  let find: string | undefined;
  let extract: Record<string, string> | undefined;

  // Parse (find) part
  const findMatch = rest.match(/^\(([^)]+)\)\s*/);
  if (findMatch) {
    find = findMatch[1].trim();
    rest = rest.slice(findMatch[0].length);
  }

  // Parse { extract } part
  const extractMatch = rest.match(/\{([^}]+)\}\s*$/);
  if (extractMatch) {
    rest = rest.slice(0, rest.lastIndexOf("{")).trim();
    const fields = extractMatch[1].trim().split(/\s+/);
    extract = {};
    for (const field of fields) {
      if (field.includes(":")) {
        const [role, name] = field.split(":");
        extract[name] = role.toLowerCase();
      } else {
        extract[field.toLowerCase()] = field.toLowerCase();
      }
    }
  }

  return { find, match: rest, extract };
}

/**
 * Parse a CSS-like selector string into steps.
 *
 * Supports:
 * - Role names: "tree", "listitem", "textfield"
 * - Direct child: "parent > child"
 * - Descendant: "ancestor descendant"
 */
function parseSelector(selector: string): SelectorStep[] {
  const steps: SelectorStep[] = [];
  const tokens = selector.trim().split(/\s+/);

  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];

    if (token === ">") {
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

    if (id !== elementId) {
      result.push(element);
    }

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
 * Find the first element matching a selector from a root.
 * Uses breadth-first search.
 */
export function findFirst(
  allio: Allio,
  rootId: AX.ElementId,
  selector: string
): TypedElement | undefined {
  const steps = parseSelector(selector);
  if (steps.length === 0) return undefined;

  const root = allio.elements.get(rootId);
  if (!root) return undefined;

  // For single step, check if root matches
  if (steps.length === 1) {
    if (matchesRole(root, steps[0].role)) return root;
    // Otherwise search descendants
    return getDescendants(allio, rootId).find((d) =>
      matchesRole(d, steps[0].role)
    );
  }

  // Multi-step: need to match the path
  // For now, support simple cases - first match of first role, then traverse
  let current: TypedElement | undefined = root;

  for (let i = 0; i < steps.length; i++) {
    const step = steps[i];

    if (i === 0) {
      // First step - check root or find in descendants
      if (matchesRole(current, step.role)) {
        continue;
      }
      current = getDescendants(allio, rootId).find((d) =>
        matchesRole(d, step.role)
      );
    } else {
      // Subsequent steps
      if (!current) return undefined;
      const candidates =
        step.combinator === ">"
          ? getDirectChildren(allio, current.id)
          : getDescendants(allio, current.id);
      current = candidates.find((c) => matchesRole(c, step.role));
    }

    if (!current) return undefined;
  }

  return current;
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

  // If first step doesn't match root, search descendants that do
  let currentMatches: TypedElement[];

  if (matchesRole(root, steps[0].role)) {
    currentMatches = [root];
  } else {
    // Find all descendants matching first step
    currentMatches = getDescendants(allio, rootId).filter((d) =>
      matchesRole(d, steps[0].role)
    );
  }

  if (steps.length === 1) {
    return currentMatches;
  }

  // Process remaining steps
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
 */
function extractValue(element: TypedElement): unknown {
  if (element.role === "checkbox") {
    return element.value === true;
  }

  if (element.role === "textfield") {
    return element.value ?? element.label ?? "";
  }

  if (element.role === "statictext") {
    return element.label ?? "";
  }

  return element.value ?? element.label ?? null;
}

/**
 * Execute a query using the full query syntax string.
 *
 * @param allio - Allio client instance
 * @param rootId - Root element to start from (window root typically)
 * @param queryStr - Query string like "(tree) listitem { checkbox:done textfield:text }"
 * @returns Array of matched elements with extracted fields
 */
export function queryString(
  allio: Allio,
  rootId: AX.ElementId,
  queryStr: string
): ExtractedResult[] {
  const parsed = parseQuerySyntax(queryStr);

  // Find the target root if (find) was specified
  let targetRootId = rootId;
  if (parsed.find) {
    const found = findFirst(allio, rootId, parsed.find);
    if (!found) return [];
    targetRootId = found.id;
  }

  // Parse and execute the match selector
  const steps = parseSelector(parsed.match);
  const matches = findMatches(allio, targetRootId, steps);

  // If no extraction, return matches with just the element
  if (!parsed.extract) {
    return matches.map((element) => ({ element }));
  }

  // Extract fields from each match (all-or-nothing: exclude if any field missing)
  const results: ExtractedResult[] = [];

  for (const element of matches) {
    const result: ExtractedResult = { element };
    let allFieldsFound = true;

    for (const [fieldName, role] of Object.entries(parsed.extract!)) {
      const descendant = findDescendantByRole(allio, element.id, role);
      if (!descendant) {
        allFieldsFound = false;
        break;
      }
      result[fieldName] = extractValue(descendant);
    }

    if (allFieldsFound) {
      results.push(result);
    }
  }

  return results;
}

/**
 * Query the cached element tree for matching elements.
 * (Legacy API - use queryString for the new syntax)
 */
export function query(
  allio: Allio,
  rootId: AX.ElementId,
  options: QueryOptions
): ExtractedResult[] {
  const steps = parseSelector(options.selector);
  const matches = findMatches(allio, rootId, steps);

  if (!options.extract) {
    return matches.map((element) => ({ element }));
  }

  // All-or-nothing: exclude if any field missing
  const results: ExtractedResult[] = [];

  for (const element of matches) {
    const result: ExtractedResult = { element };
    let allFieldsFound = true;

    for (const [fieldName, roleSelector] of Object.entries(options.extract!)) {
      const descendant = findDescendantByRole(
        allio,
        element.id,
        roleSelector.toLowerCase()
      );
      if (!descendant) {
        allFieldsFound = false;
        break;
      }
      result[fieldName] = extractValue(descendant);
    }

    if (allFieldsFound) {
      results.push(result);
    }
  }

  return results;
}

/**
 * Convenience function to query with automatic typing.
 */
export function queryAs<T extends Record<string, unknown>>(
  allio: Allio,
  rootId: AX.ElementId,
  options: QueryOptions
): (T & { element: TypedElement })[] {
  return query(allio, rootId, options) as (T & { element: TypedElement })[];
}

/**
 * Convenience function to query with string syntax and automatic typing.
 */
export function queryStringAs<T extends Record<string, unknown>>(
  allio: Allio,
  rootId: AX.ElementId,
  queryStr: string
): (T & { element: TypedElement })[] {
  return queryString(allio, rootId, queryStr) as (T & {
    element: TypedElement;
  })[];
}
