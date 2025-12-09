// Re-export generated types from Rust via ts-rs
// Regenerate with: npm run typegen

// IDs
export type { ProcessId } from "./generated/ProcessId";
export type { WindowId } from "./generated/WindowId";
export type { ElementId } from "./generated/ElementId";

// Accessibility types (cross-platform)
export type { Role } from "./generated/Role";
export type { Action } from "./generated/Action";
export type { Value } from "./generated/Value";
export type { Notification } from "./generated/Notification";

// Core types
export type { AXElement } from "./generated/AXElement";
export type { AXWindow } from "./generated/AXWindow";
export type { Bounds } from "./generated/Bounds";
export type { TextSelection } from "./generated/TextSelection";

// Events & RPC
export type { Event } from "./generated/Event";
export type { Snapshot } from "./generated/Snapshot";
export type { RpcRequest } from "./generated/RpcRequest";
export type { RpcResponse } from "./generated/RpcResponse";
