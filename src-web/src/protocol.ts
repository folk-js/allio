/**
 * Protocol Types
 *
 * WebSocket message types mirroring Rust protocol.rs EXACTLY
 *
 * Design:
 * - Request/Response pairs co-located in namespaces (mirrors Rust modules)
 * - ClientMessage and ServerMessage discriminated unions
 * - Simple, clear structure with compile-time type safety
 */

import { AXNode, AXValue } from "./axio.ts";

// ============================================================================
// Window Types
// ============================================================================

export interface Window {
  id: string;
  title: string;
  app_name: string;
  x: number;
  y: number;
  w: number;
  h: number;
  focused: boolean;
  process_id: number;
}

// ============================================================================
// Request/Response Pair Namespaces
// ============================================================================

/** Get children of an accessibility element */
export namespace GetChildren {
  export interface Request {
    element_id: string;
    max_depth?: number;
    max_children_per_level?: number;
  }

  export interface Response {
    success: boolean;
    children?: AXNode[];
    error?: string;
  }
}

/** Write text to an accessibility element */
export namespace WriteToElement {
  export interface Request {
    element_id: string;
    text: string;
  }

  export interface Response {
    success: boolean;
    error?: string;
  }
}

/** Click an accessibility element */
export namespace ClickElement {
  export interface Request {
    element_id: string;
  }

  export interface Response {
    success: boolean;
    error?: string;
  }
}

/** Enable/disable click-through on overlay window */
export namespace SetClickthrough {
  export interface Request {
    enabled: boolean;
  }

  export interface Response {
    success: boolean;
    enabled: boolean;
    error?: string;
  }
}

/** Start watching an element for changes */
export namespace WatchNode {
  export interface Request {
    element_id: string;
    node_id: string;
  }

  export interface Response {
    success: boolean;
    node_id: string;
    error?: string;
  }
}

/** Stop watching an element */
export namespace UnwatchNode {
  export interface Request {
    element_id: string;
  }

  export interface Response {
    success: boolean;
  }
}

// ============================================================================
// Client -> Server Messages
// ============================================================================

export type ClientMessage =
  | ({ type: "get_children" } & GetChildren.Request)
  | ({ type: "write_to_element" } & WriteToElement.Request)
  | ({ type: "click_element" } & ClickElement.Request)
  | ({ type: "set_clickthrough" } & SetClickthrough.Request)
  | ({ type: "watch_node" } & WatchNode.Request)
  | ({ type: "unwatch_node" } & UnwatchNode.Request);

// ============================================================================
// Server -> Client Messages
// ============================================================================

export type ServerMessage =
  // Push Events
  | { type: "window_update"; windows: Window[] }
  | { type: "window_root_update"; window_id: string; root: AXNode }
  | { type: "mouse_position"; x: number; y: number }
  | { type: "element_update"; update: ElementUpdate }
  // Response Messages
  | ({ type: "get_children_response" } & GetChildren.Response)
  | ({ type: "write_to_element_response" } & WriteToElement.Response)
  | ({ type: "click_element_response" } & ClickElement.Response)
  | ({ type: "set_clickthrough_response" } & SetClickthrough.Response)
  | ({ type: "watch_node_response" } & WatchNode.Response)
  | ({ type: "unwatch_node_response" } & UnwatchNode.Response);

// ============================================================================
// Element Update Types
// ============================================================================

export type ElementUpdate =
  | { update_type: "ValueChanged"; element_id: string; value: AXValue }
  | { update_type: "TitleChanged"; element_id: string; title: string }
  | { update_type: "ElementDestroyed"; element_id: string };

// ============================================================================
// Optional: Helper Constructors (add as needed)
// ============================================================================

export namespace GetChildren {
  export function success(children: AXNode[]): Response {
    return { success: true, children };
  }

  export function error(error: string): Response {
    return { success: false, error };
  }
}

export namespace WriteToElement {
  export function success(): Response {
    return { success: true };
  }

  export function error(error: string): Response {
    return { success: false, error };
  }
}

export namespace ClickElement {
  export function success(): Response {
    return { success: true };
  }

  export function error(error: string): Response {
    return { success: false, error };
  }
}

export namespace SetClickthrough {
  export function success(enabled: boolean): Response {
    return { success: true, enabled };
  }

  export function error(enabled: boolean, error: string): Response {
    return { success: false, enabled, error };
  }
}

export namespace WatchNode {
  export function success(node_id: string): Response {
    return { success: true, node_id };
  }

  export function error(node_id: string, error: string): Response {
    return { success: false, node_id, error };
  }
}

export namespace UnwatchNode {
  export function success(): Response {
    return { success: true };
  }
}

// ============================================================================
// Legacy Type Aliases (DEPRECATED - Remove after migration)
// ============================================================================

/**
 * @deprecated Use GetChildren.Request instead (via ClientMessage type)
 */
export interface GetChildrenRequest {
  element_id: string;
  max_depth?: number;
  max_children_per_level?: number;
}

/**
 * @deprecated Use WriteToElement.Request instead (via ClientMessage type)
 */
export interface SetElementValueRequest {
  element_id: string;
  value: string;
}

/**
 * @deprecated Use ClickElement.Request instead (via ClientMessage type)
 */
export interface ClickElementRequest {
  element_id: string;
}

/**
 * @deprecated Use WatchNode.Request instead (via ClientMessage type)
 */
export interface WatchNodeRequest {
  element_id: string;
  node_id: string;
}

/**
 * @deprecated Use UnwatchNode.Request instead (via ClientMessage type)
 */
export interface UnwatchNodeRequest {
  element_id: string;
}

/**
 * @deprecated Use SetClickthrough.Request instead (via ClientMessage type)
 */
export interface SetClickthroughRequest {
  enabled: boolean;
}

/**
 * @deprecated Use ServerMessage with type: "get_children_response" instead
 */
export interface GetAccessibilityTreeResponse {
  success: boolean;
  tree?: any;
  error?: string;
}

/**
 * @deprecated Use GetChildren.Response instead (via ServerMessage type)
 */
export interface GetChildrenResponse {
  success: boolean;
  children?: any[];
  error?: string;
}

/**
 * @deprecated Use WriteToElement.Response instead (via ServerMessage type)
 */
export interface SetElementValueResponse {
  success: boolean;
  error?: string;
}

/**
 * @deprecated Use ClickElement.Response instead (via ServerMessage type)
 */
export interface ClickElementResponse {
  success: boolean;
  error?: string;
}

/**
 * @deprecated Use WatchNode.Response instead (via ServerMessage type)
 */
export interface WatchNodeResponse {
  success: boolean;
  node_id: string;
  error?: string;
}

/**
 * @deprecated Use UnwatchNode.Response instead (via ServerMessage type)
 */
export interface UnwatchNodeResponse {
  success: boolean;
}

/**
 * @deprecated Use SetClickthrough.Response instead (via ServerMessage type)
 */
export interface SetClickthroughResponse {
  success: boolean;
  enabled: boolean;
  error?: string;
}
