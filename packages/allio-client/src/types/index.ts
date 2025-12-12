// Re-export generated types from Rust via ts-rs
// Regenerate with: npm run typegen

// === AX Namespace ===
// Use this to avoid collisions with browser's Window/Element types
// Example: import { AX } from 'allio'; then use AX.Element, AX.Window
export * as AX from "./ax";

// RPC types (no collision risk, used internally)
export type { RpcRequest } from "./generated/RpcRequest";
export type { RpcResponse } from "./generated/RpcResponse";
