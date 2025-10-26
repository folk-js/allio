/**
 * Protocol Types
 *
 * WebSocket message types mirroring Rust protocol.rs
 * All request/response pairs are defined here for type safety
 */

// ============================================================================
// Request Types (Client -> Server)
// ============================================================================

export interface GetAccessibilityTreeRequest {
  window_id?: string;
  max_depth?: number;
  max_children_per_level?: number;
}

export interface GetChildrenRequest {
  element_id: string;
  max_depth?: number;
  max_children_per_level?: number;
}

export interface SetElementValueRequest {
  element_id: string;
  value: string; // For now just string, will expand to AXValue later
}

export interface ClickElementRequest {
  element_id: string;
}

export interface WatchNodeRequest {
  element_id: string;
  node_id: string;
}

export interface UnwatchNodeRequest {
  element_id: string;
}

export interface SetClickthroughRequest {
  enabled: boolean;
}

// ============================================================================
// Response Types (Server -> Client)
// ============================================================================

export interface GetAccessibilityTreeResponse {
  success: boolean;
  tree?: any; // AXNode type
  error?: string;
}

export interface GetChildrenResponse {
  success: boolean;
  children?: any[]; // AXNode[]
  error?: string;
}

export interface SetElementValueResponse {
  success: boolean;
  error?: string;
}

export interface ClickElementResponse {
  success: boolean;
  error?: string;
}

export interface WatchNodeResponse {
  success: boolean;
  node_id: string;
  error?: string;
}

export interface UnwatchNodeResponse {
  success: boolean;
}

export interface SetClickthroughResponse {
  success: boolean;
  enabled: boolean;
  error?: string;
}

