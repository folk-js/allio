import { AXIO, AXElement } from "@axio/client";

class AXTreeOverlay {
  private container: HTMLElement;
  private axio: AXIO;

  // Minimal local state
  private expanded = new Set<string>();
  private treeEl: HTMLElement | null = null;
  private outlineEl: HTMLElement | null = null;
  private isClickthroughEnabled = true;

  constructor() {
    this.container = document.getElementById("windowContainer")!;
    this.axio = new AXIO();
    this.init();
  }

  private async init() {
    // Simple event handlers - just re-render when things change
    this.axio.on("window:active", () => this.render());
    this.axio.on("element:discovered", (el) => {
      // Auto-watch textboxes for value changes
      if (el.role === "textbox" || el.role === "searchbox") {
        console.log("👁️ Auto-watching textbox:", el.label ?? el.id);
        this.axio
          .watch(el.id)
          .catch((err) => console.error("Watch failed:", err));
      }
      this.render();
    });
    this.axio.on("element:updated", ({ element, changed }) => {
      console.log(
        "📝 element:updated",
        element.role,
        element.label,
        "changed:",
        changed
      );
      this.render();
    });
    this.axio.on("sync:snapshot", () => this.render());
    this.axio.on("window:updated", () => this.render());

    // Mouse tracking for clickthrough
    this.axio.on("mouse:position", ({ x, y }) => {
      const el = document.elementFromPoint(x, y);
      const overTree = el && this.treeEl?.contains(el);
      const shouldClickthrough = !overTree;

      if (shouldClickthrough !== this.isClickthroughEnabled) {
        this.isClickthroughEnabled = shouldClickthrough;
        this.axio.setClickthrough(shouldClickthrough).catch(() => {});
      }
    });

    await this.axio.connect();
    console.log("📡 AXTree connected");
  }

  private render() {
    const win = this.axio.active;

    // No active window - remove tree
    if (!win) {
      this.treeEl?.remove();
      this.treeEl = null;
      return;
    }

    // Get window's children
    const children = this.axio.getChildren(win);

    // Create tree container if needed
    if (!this.treeEl) {
      this.treeEl = document.createElement("div");
      this.treeEl.className = "accessibility-tree";
      this.container.appendChild(this.treeEl);
      this.attachHandlers();
    }

    // Always update position (even if no children yet)
    Object.assign(this.treeEl.style, {
      left: `${win.x + win.w + 10}px`,
      top: `${win.y}px`,
      height: `${win.h}px`,
    });

    // Only render content if we have children
    if (children.length === 0) {
      this.treeEl.innerHTML = `<div class="tree-loading">Loading...</div>`;
      return;
    }

    // Render content
    this.treeEl.innerHTML = `
      <div class="tree-legend">
        <span class="legend-item"><span class="tree-role">role</span></span>
        <span class="legend-item"><span class="tree-label">label</span></span>
        <span class="legend-item"><span class="tree-value">value</span></span>
      </div>
      <div class="tree-content">${this.renderNodes(children)}</div>
    `;
  }

  private renderNodes(elements: AXElement[], depth = 0): string {
    return elements.map((el) => this.renderNode(el, depth)).join("");
  }

  private renderNode(el: AXElement, depth: number): string {
    const children = this.axio.getChildren(el);
    const notDiscovered = el.children === null;
    // Has children if IDs exist (even if not yet loaded into elements Map)
    const hasChildIds = (el.children?.length ?? 0) > 0;
    const hasLoadedChildren = children.length > 0;
    const isExpanded = this.expanded.has(el.id);
    const isLoading =
      this.expanded.has(el.id) && hasChildIds && !hasLoadedChildren;
    const isWatched = this.axio.watched.has(el.id);

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
    const isTextInput = el.role === "textbox" || el.role === "searchbox";

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
          ${
            el.subrole && el.subrole !== el.role
              ? `<span class="tree-subrole">:${el.subrole}</span>`
              : ""
          }
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
          ${count ? `<span class="tree-count">(${count})</span>` : ""}
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

      const id = node.dataset.id!;
      const action = target.dataset.action;

      if (action === "toggle") {
        e.stopPropagation();
        const el = this.axio.get(id);
        if (!el) return;

        const loadedChildren = this.axio.getChildren(el);
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
              await this.axio.children(id);
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
          try {
            await this.axio.write(node.dataset.id, target.value);
            console.log(`✅ Wrote "${target.value}"`);
          } catch (err) {
            console.error("Failed to write:", err);
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
          const el = await this.axio.refresh(node.dataset.id);
          if (el.bounds) {
            const { x, y } = el.bounds.position;
            const { width: w, height: h } = el.bounds.size;
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
