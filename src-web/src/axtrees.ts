/**
 * AXTree Overlay - Accessibility tree viewer
 * Uses the new Allio architecture: elements are primary, trees are views.
 */

import { Allio, AX, AllioPassthrough } from "allio";

class AXTreeOverlay {
  private container: HTMLElement;
  private allio: Allio;

  // Minimal local state
  private expanded = new Set<AX.ElementId>();
  private treeEl: HTMLElement | null = null;
  private outlineEl: HTMLElement | null = null;
  // Track which windows we've fetched the tree root for
  private fetchedRoots = new Set<AX.WindowId>();

  constructor() {
    this.container = document.getElementById("windowContainer")!;
    this.allio = new Allio();
    // Declarative passthrough: allio-opaque elements capture, rest passes through
    new AllioPassthrough(this.allio);
    this.init();
  }

  private async init() {
    const render = () => this.render();

    // Fetch root when we get initial sync data
    this.allio.on("sync:init", async () => {
      if (this.allio.focusedWindow) {
        await this.fetchWindowRoot(this.allio.focusedWindow);
      }
      render();
    });

    // Fetch root elements when active window changes
    this.allio.on("focus:window", async (data) => {
      if (data.window_id) {
        await this.fetchWindowRoot(data.window_id);
        render();
      }
    });

    // Fetch root when a new window is added (in case it's focused)
    this.allio.on("window:added", async () => {
      if (this.allio.focusedWindow) {
        await this.fetchWindowRoot(this.allio.focusedWindow);
      }
      render();
    });

    // Clean up tracked roots when windows are removed
    this.allio.on("window:removed", (data) => {
      this.fetchedRoots.delete(data.window_id);
      render();
    });

    // Re-render on window/element changes
    // Note: Tier 2 auto-watches text elements on focus, so element:changed fires automatically
    (
      [
        "focus:window",
        "focus:element", // Tier 1: element focus changes
        "selection:changed", // Tier 1: text selection changes
        "window:changed",
        "element:added",
        "element:changed",
        "element:removed",
      ] as const
    ).forEach((e) => this.allio.on(e, render));

    // Clickthrough is now handled declaratively by PointerPassthroughManager
    // (tree element is marked with allio-opaque attribute)

    await this.allio.connect();
  }

  /** Fetch root element and its immediate children for a window */
  private async fetchWindowRoot(windowId: AX.WindowId): Promise<void> {
    try {
      const root = await this.allio.windowRoot(windowId);
      if (root) {
        this.fetchedRoots.add(windowId);
        // Also fetch immediate children so tree is usable
        await this.allio.children(root.id);
      }
    } catch (err) {
      console.error("Failed to fetch window root:", err);
    }
  }

  private render() {
    const win = this.allio.focused;

    // No focused window - remove tree
    if (!win) {
      this.treeEl?.remove();
      this.treeEl = null;
      return;
    }

    // Get the root element for this window (elements with root=true)
    const rootElement = this.allio.getRootElement(win.id);

    // Create tree container if needed
    if (!this.treeEl) {
      this.treeEl = document.createElement("div");
      this.treeEl.className = "accessibility-tree";
      this.treeEl.setAttribute("ax-io", "opaque"); // Capture pointer events on tree
      this.container.appendChild(this.treeEl);
      this.attachHandlers();
    }

    // Always update position (using bounds)
    Object.assign(this.treeEl.style, {
      left: `${win.bounds.x + win.bounds.w + 10}px`,
      top: `${win.bounds.y}px`,
      height: `${win.bounds.h}px`,
    });

    // Only render content if we have the root element
    if (!rootElement) {
      this.treeEl.innerHTML = `<div class="tree-loading">Loading...</div>`;
      return;
    }

    // Render content starting from the tracked root
    this.treeEl.innerHTML = `
      <div class="tree-legend">
        <span class="legend-item"><span class="tree-role">role</span></span>
        <span class="legend-item"><span class="tree-label">label</span></span>
        <span class="legend-item"><span class="tree-value">value</span></span>
        <span class="legend-item"><span class="tree-actions">[actions]</span></span>
        <span class="legend-item"><span class="tree-pid">[pid]</span></span>
      </div>
      <div class="tree-content">${this.renderNodes([rootElement])}</div>
    `;
  }

  private renderNodes(elements: AX.Element[], depth = 0): string {
    return elements.map((el) => this.renderNode(el, depth)).join("");
  }

  private renderNode(el: AX.Element, depth: number): string {
    const children = this.allio.getChildren(el);
    const notDiscovered = el.children === null;
    // Has children if IDs exist (even if not yet loaded into elements Map)
    const hasChildIds = (el.children?.length ?? 0) > 0;
    const hasLoadedChildren = children.length > 0;
    const isExpanded = this.expanded.has(el.id);
    const isLoading =
      this.expanded.has(el.id) && hasChildIds && !hasLoadedChildren;
    const isWatched = this.allio.watched.has(el.id);

    // Indicator: + (not discovered), ⋯ (loading), ▸/▾ (expand/collapse), •/◉ (leaf)
    let indicator = isWatched ? "◉" : "•";
    if (notDiscovered) indicator = "+";
    else if (isLoading) indicator = "⋯";
    else if (hasChildIds) indicator = isExpanded ? "▾" : "▸";

    // Format value
    let valueStr = "";
    if (el.value) {
      const v = el.value.value;
      valueStr = typeof v === "string" ? `"${v}"` : String(v);
    }

    // Count
    const count = notDiscovered ? "?" : hasChildIds ? el.children!.length : 0;
    const isTextInput =
      el.role === "textfield" ||
      el.role === "searchfield" ||
      el.role === "textarea";

    return `
      <div class="tree-node" data-id="${el.id}">
        <div class="tree-node-content" style="padding-left: ${
          depth * 12 + 4
        }px">
          <span class="tree-indicator ${
            notDiscovered || hasChildIds ? "clickable" : ""
          } ${isWatched ? "watched" : ""}" 
                data-action="toggle">${indicator}</span>
          <span class="tree-role">${el.role}</span>
          <span class="tree-subrole" title="${el.platform_role}">${
      el.platform_role.includes("/") ? `:${el.platform_role.split("/")[1]}` : ""
    }</span>
          ${
            el.label
              ? `<span class="tree-label">"${this.escapeHtml(el.label)}"</span>`
              : ""
          }
          ${
            valueStr
              ? `<span class="tree-value">= ${this.escapeHtml(valueStr)}</span>`
              : ""
          }
          ${el.focused ? `<span class="tree-state">[focused]</span>` : ""}
          ${
            el.actions.length > 0
              ? `<span class="tree-actions">[${el.actions.join(", ")}]</span>`
              : ""
          }
          ${count ? `<span class="tree-count">(${count})</span>` : ""}
          <span class="tree-pid">[${el.pid}]</span>
          ${
            isTextInput
              ? `
            <input class="tree-text-input" 
                   data-action="write" 
                   value="${this.escapeHtml(String(el.value?.value ?? ""))}"
                   placeholder="Enter text..." />
          `
              : ""
          }
        </div>
      </div>
      ${
        hasLoadedChildren && isExpanded
          ? this.renderNodes(children, depth + 1)
          : ""
      }
    `;
  }

  private escapeHtml(str: string): string {
    return str.replace(
      /[&<>"']/g,
      (c) =>
        ({
          "&": "&amp;",
          "<": "&lt;",
          ">": "&gt;",
          '"': "&quot;",
          "'": "&#39;",
        }[c] || c)
    );
  }

  private attachHandlers() {
    if (!this.treeEl) return;

    // Click handler (event delegation)
    this.treeEl.addEventListener("click", async (e) => {
      const target = e.target as HTMLElement;
      const node = target.closest(".tree-node") as HTMLElement;
      if (!node) return;

      const id = parseInt(node.dataset.id!);
      const action = target.dataset.action;

      if (action === "toggle") {
        e.stopPropagation();
        const el = this.allio.get(id);
        if (!el) return;

        const loadedChildren = this.allio.getChildren(el);
        const hasChildIds = (el.children?.length ?? 0) > 0;
        const needsLoad =
          el.children === null || (hasChildIds && loadedChildren.length === 0);

        if (this.expanded.has(id) && !needsLoad) {
          // Collapse (only if children are loaded)
          this.expanded.delete(id);
          this.render();
        } else {
          // Expand (and load if needed)
          this.expanded.add(id);
          this.render();

          if (needsLoad) {
            try {
              await this.allio.children(id);
            } catch (err) {
              console.error("Failed to load children:", err);
              this.expanded.delete(id);
            }
            this.render();
          }
        }
      }
    });

    // Keyboard handler for text inputs
    this.treeEl.addEventListener("keydown", async (e) => {
      const target = e.target as HTMLInputElement;
      if (target.dataset.action === "write" && e.key === "Enter") {
        e.preventDefault();
        const node = target.closest(".tree-node") as HTMLElement;
        if (node?.dataset.id) {
          const el = this.allio.get(parseInt(node.dataset.id!));
          if (el) {
            try {
              await this.allio.writeValue(el, target.value);
              console.log(`✅ Wrote "${target.value}"`);
            } catch (err) {
              console.error("Failed to write:", err);
            }
          }
        }
      }
    });

    // Stop input clicks from bubbling
    this.treeEl.addEventListener("click", (e) => {
      if ((e.target as HTMLElement).tagName === "INPUT") {
        e.stopPropagation();
      }
    });

    // Hover outline - refresh to get current bounds
    this.treeEl.addEventListener("mouseover", async (e) => {
      const node = (e.target as HTMLElement).closest(
        ".tree-node"
      ) as HTMLElement;
      if (node?.dataset.id) {
        try {
          const el = await this.allio.refresh(parseInt(node.dataset.id!));
          if (el.bounds) {
            const { x, y, w, h } = el.bounds;
            this.showOutline(x, y, w, h);
          }
        } catch {
          // Element may no longer exist, ignore
        }
      }
    });

    this.treeEl.addEventListener("mouseout", (e) => {
      const node = (e.target as HTMLElement).closest(
        ".tree-node"
      ) as HTMLElement;
      if (node?.dataset.id) {
        this.hideOutline();
      }
    });
  }

  private showOutline(x: number, y: number, w: number, h: number) {
    if (!this.outlineEl) {
      this.outlineEl = document.createElement("div");
      this.outlineEl.className = "hover-outline";
      this.outlineEl.style.cssText = `
        position: absolute;
        pointer-events: none;
        border: 2px solid #007aff;
        background: rgba(0, 122, 255, 0.1);
        z-index: 999;
        transition: all 0.1s ease-out;
      `;
      this.container.appendChild(this.outlineEl);
    }

    Object.assign(this.outlineEl.style, {
      left: `${x}px`,
      top: `${y}px`,
      width: `${w}px`,
      height: `${h}px`,
      display: "block",
    });
  }

  private hideOutline() {
    if (this.outlineEl) {
      this.outlineEl.style.display = "none";
    }
  }
}

// Initialize
document.addEventListener("DOMContentLoaded", () => {
  new AXTreeOverlay();
});
