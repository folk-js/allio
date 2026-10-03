import type { DatatypeImplementation } from "@inkandswitch/patchwork-plugins";
import type { DocHandle } from "@automerge/automerge-repo";
import { TODO_TYPE, type Todo, type TodoDoc } from "./types";
import "./todo.css";

function uuid(): string {
  return typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : Math.random().toString(36).slice(2);
}

export const TodoDatatype: DatatypeImplementation<TodoDoc> = {
  init(doc: TodoDoc) {
    doc["@patchwork"] = { type: TODO_TYPE };
    doc.title = "Todo";
    doc.todos = [];
  },
  getTitle(doc: TodoDoc) {
    return doc.title || "Todo";
  },
  setTitle(doc: TodoDoc, title: string) {
    doc.title = title.trim();
  },
};

/**
 * Minimal, self-contained todo renderer (no external module needed). Renders
 * into the light DOM of a `<patchwork-view>` host; all CSS is scoped under
 * `.allio-todo` and derived from the theme.
 */
export function renderTodo(
  handle: DocHandle<TodoDoc>,
  element: HTMLElement
): () => void {
  element.innerHTML = "";
  const root = document.createElement("div");
  root.className = "allio-todo";
  element.appendChild(root);

  // Skip re-render while the user is editing a field, so the bridge's inbound
  // updates don't clobber an in-progress edit.
  let editing = false;

  function render() {
    if (editing) return;
    const doc = handle.doc();
    root.innerHTML = "";
    if (!doc) return;

    const list = document.createElement("div");
    list.className = "allio-todo-list";

    if (!doc.todos || doc.todos.length === 0) {
      const empty = document.createElement("div");
      empty.className = "allio-todo-empty";
      empty.textContent = "No todos yet.";
      list.appendChild(empty);
    } else {
      doc.todos.forEach((todo, index) => {
        list.appendChild(renderRow(todo, index));
      });
    }

    const addBar = document.createElement("div");
    addBar.className = "allio-todo-add";
    const addInput = document.createElement("input");
    addInput.type = "text";
    addInput.placeholder = "Add a todo…";
    addInput.addEventListener("focus", () => (editing = true));
    addInput.addEventListener("blur", () => (editing = false));
    addInput.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key !== "Enter") return;
      const description = addInput.value.trim();
      if (!description) return;
      handle.change((d) => {
        d.todos.push({ id: uuid(), description, done: false } as Todo);
      });
      addInput.value = "";
    });
    addBar.appendChild(addInput);

    root.append(list, addBar);
  }

  function renderRow(todo: Todo, index: number): HTMLElement {
    const row = document.createElement("div");
    row.className = `allio-todo-row${todo.done ? " done" : ""}`;

    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = todo.done;
    cb.addEventListener("change", () => {
      handle.change((d) => {
        if (d.todos[index]) d.todos[index].done = cb.checked;
      });
    });

    const text = document.createElement("input");
    text.type = "text";
    text.value = todo.description;
    text.addEventListener("focus", () => (editing = true));
    text.addEventListener("blur", () => {
      editing = false;
      if (text.value !== todo.description) {
        handle.change((d) => {
          if (d.todos[index]) d.todos[index].description = text.value;
        });
      }
      render();
    });
    text.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Enter") text.blur();
      if (e.key === "Escape") {
        text.value = todo.description;
        text.blur();
      }
    });

    const del = document.createElement("button");
    del.className = "allio-todo-del";
    del.textContent = "✕";
    del.title = "Delete";
    del.addEventListener("click", () => {
      handle.change((d) => {
        const i = d.todos.findIndex((t) => t.id === todo.id);
        if (i >= 0) d.todos.splice(i, 1);
      });
    });

    row.append(cb, text, del);
    return row;
  }

  const onChange = () => render();
  handle.on("change", onChange);
  render();

  return () => {
    handle.off("change", onChange);
    element.innerHTML = "";
  };
}
