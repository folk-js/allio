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
    this.axio.on("window:active", (d) => {
      console.log("🎯 window:active", d);
      this.render();
    });
    this.axio.on("element:discovered", (el) => {
      console.log("📦 element:discovered", el.role, el.label);
      this.render();
    });
    this.axio.on("element:updated", (d) => {
      console.log("📝 element:updated", d.element.role);
      this.render();
    });
    this.axio.on("sync:snapshot", (snapshot) => {
      console.log("📸 sync:snapshot", {
        windows: snapshot.windows.length,
        active: snapshot.active_window,
      });
      this.render();
    });
    this.axio.on("window:updated", (win) => {
      console.log(
        "🪟 window:updated",
        win.id,
        "children:",
        win.children?.length ?? "null"
      );
      this.render();
    });

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
    console.log("🎨 render()", {
      activeWindow: win?.title ?? null,
      activeId: this.axio.activeWindow,
      windowsCount: this.axio.windows.size,
      elementsCount: this.axio.elements.size,
    });

    // No active window - remove tree
    if (!win) {
      this.treeEl?.remove();
      this.treeEl = null;
      return;
    }

    // Get window's children
    const children = this.axio.getChildren(win);
    console.log(
      "  children:",
      children.length,
      "discovered:",
      win.children !== null
    );

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
    const hasChildren = children.length > 0;
    const notDiscovered = el.children === null;
    const isExpanded = this.expanded.has(el.id);
    const isLoading = el.children === null && this.expanded.has(el.id);

    // Indicator: + (load), ▸/▾ (expand/collapse), • (leaf), ⋯ (loading)
    let indicator = "•";
    if (isLoading) indicator = "⋯";
    else if (notDiscovered) indicator = "+";
    else if (hasChildren) indicator = isExpanded ? "▾" : "▸";

    // Format value
    let valueStr = "";
    if (el.value) {
      const v = el.value.value;
      valueStr = typeof v === "string" ? `"${v}"` : String(v);
    }

    // Count
    const count = notDiscovered ? "?" : hasChildren ? children.length : 0;
    const isTextInput = el.role === "textbox" || el.role === "searchbox";

    // Bounds for hover outline
    const boundsAttr = el.bounds
      ? `data-bounds="${el.bounds.position.x},${el.bounds.position.y},${el.bounds.size.width},${el.bounds.size.height}"`
      : "";

    return `
      <div class="tree-node" data-id="${el.id}" ${boundsAttr}>
        <div class="tree-node-content" style="padding-left: ${
          depth * 12 + 4
        }px">
          <span class="tree-indicator ${
            notDiscovered || hasChildren ? "clickable" : ""
          }" 
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
            <input class="tree-input" 
                   data-action="write" 
                   value="${this.escapeHtml(String(el.value?.value ?? ""))}"
                   placeholder="Enter text..." />
          `
              : ""
          }
        </div>
      </div>
      ${hasChildren && isExpanded ? this.renderNodes(children, depth + 1) : ""}
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

        if (el.children === null) {
          // Load children
          this.expanded.add(id); // Mark as loading
          this.render();
          try {
            await this.axio.children(id);
            await this.axio.watch(id);
          } catch (err) {
            console.error("Failed to load children:", err);
            this.expanded.delete(id);
          }
          this.render();
        } else if (this.expanded.has(id)) {
          // Collapse
          this.expanded.delete(id);
          this.axio.unwatch(id).catch(() => {});
          this.render();
        } else {
          // Expand
          this.expanded.add(id);
          this.axio.watch(id).catch(() => {});
          this.render();
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

    // Hover outline
    this.treeEl.addEventListener("mouseover", (e) => {
      const node = (e.target as HTMLElement).closest(
        ".tree-node"
      ) as HTMLElement;
      if (node?.dataset.bounds) {
        const [x, y, w, h] = node.dataset.bounds.split(",").map(Number);
        this.showOutline(x, y, w, h);
      }
    });

    this.treeEl.addEventListener("mouseout", (e) => {
      const node = (e.target as HTMLElement).closest(
        ".tree-node"
      ) as HTMLElement;
      if (node?.dataset.bounds) {
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
