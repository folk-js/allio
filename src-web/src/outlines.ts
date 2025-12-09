/**
 * Minimal window outlines demo - absolute minimum rendering work.
 * Uses CSS transforms for GPU compositing.
 *
 * Includes timing instrumentation to debug latency.
 */

import { AXIO, WindowId } from "@axio/client";

const axio = new AXIO();
const container = document.getElementById("container")!;

// Pre-allocated outline elements (reuse to avoid DOM churn)
const outlineElements = new Map<WindowId, HTMLDivElement>();

// Timing stats
let lastEventTime = 0;
let pendingUpdate = false;

function render() {
  const jsReceiveTime = performance.now();

  const windows = axio.windows;
  const existingIds = new Set(outlineElements.keys());

  // Update or create outlines for each window
  for (const [id, win] of windows) {
    existingIds.delete(id);

    let el = outlineElements.get(id);
    if (!el) {
      el = document.createElement("div");
      el.className = "window-outline";
      container.appendChild(el);
      outlineElements.set(id, el);
    }

    // Use CSS transform for GPU-composited movement (faster than left/top)
    const { x, y, w, h } = win.bounds;
    el.style.transform = `translate3d(${x}px, ${y}px, 0)`;
    el.style.width = `${w}px`;
    el.style.height = `${h}px`;

    // Focused state
    el.classList.toggle("focused", win.focused);
  }

  // Remove stale outlines
  for (const id of existingIds) {
    const el = outlineElements.get(id);
    if (el) {
      el.remove();
      outlineElements.delete(id);
    }
  }

  const domUpdateTime = performance.now();
  console.log(
    `[timing] JS render: ${(domUpdateTime - jsReceiveTime).toFixed(2)}ms`
  );
}

// Alternative: use rAF to batch updates and measure frame timing
function renderOnNextFrame() {
  if (pendingUpdate) return;
  pendingUpdate = true;

  requestAnimationFrame(() => {
    const rafTime = performance.now();
    render();
    pendingUpdate = false;
    console.log(
      `[timing] rAF delay: ${(rafTime - lastEventTime).toFixed(2)}ms`
    );
  });
}

// Connect and render on any window change
axio.connect().then(() => {
  render();

  // Minimal event set - just window changes
  axio.on("sync:init", render);
  axio.on("window:added", render);

  // Track event timing
  axio.on("window:changed", (data) => {
    lastEventTime = performance.now();
    // Try immediate render vs rAF - toggle to compare
    render(); // immediate
    // renderOnNextFrame(); // rAF batched
  });

  axio.on("window:removed", render);
});
