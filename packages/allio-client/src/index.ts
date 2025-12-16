// Allio Client - TypeScript client for Allio accessibility system
// Types are auto-generated from Rust via ts-rs

export * from "./types";
export { Allio } from "./allio";
export { AllioOcclusion } from "./occlusion";
export { AllioPassthrough, type PassthroughMode } from "./passthrough";
export {
  query,
  queryAs,
  queryString,
  queryStringAs,
  findFirst,
  parseQuerySyntax,
  type QueryOptions,
  type ExtractedResult,
  type ParsedQuery,
} from "./query";
