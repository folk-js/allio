import type { AutomergeUrl } from "@automerge/automerge-repo";

/** A tool card placed on the desktop canvas. */
export type Embed = {
  id: string;
  /** automerge: URL of the embedded document. */
  docUrl: AutomergeUrl;
  /** Optional tool id; when absent the document's default tool is used. */
  toolId?: string;
  x: number;
  y: number;
  width: number;
  height: number;
  /** If set, this card is bidirectionally synced with a native app. */
  allioLink?: AllioLink;
};

/**
 * Describes a live link between an embedded doc and a native app surface.
 *
 * The mapping is expressed with Allio's declarative query syntax (the same one
 * the `query` demo uses), so any app can be mapped onto the shape the tool
 * expects — nothing is hard-coded to a specific application.
 *
 * @example
 * { app: "Reminders", query: "(tree) listitem { checkbox:done textfield:text }" }
 * { app: "Notes",     query: "(outline) row { checkbox:done statictext:text }" }
 */
export type AllioLink = {
  /** Native application name to bind to (matched against window app_name). */
  app: string;
  /** Query mapping the app's a11y tree onto `{ done, text }` rows. */
  query: string;
};

/** Default query: Apple Reminders' shape. Users can edit this per link. */
export const DEFAULT_QUERY = "(tree) listitem { checkbox:done textfield:text }";

export type AllioFrameDoc = {
  "@patchwork": { type: "allio-frame" };
  title: string;
  embeds: Embed[];
};

/**
 * Todo document shape. Compatible with the standard Patchwork `todo` shape,
 * but registered under our own `allio-todo` datatype so the tool is
 * self-contained and needs no external module in the account.
 */
export type Todo = {
  id: string;
  description: string;
  done: boolean;
};

export type TodoDoc = {
  "@patchwork": { type: "allio-todo" };
  title: string;
  todos: Todo[];
};

/** Datatype + tool id for the bundled todo cards. */
export const TODO_TYPE = "allio-todo";
