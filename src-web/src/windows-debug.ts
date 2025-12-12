import { AXIO } from "@axio/client";

const axio = new AXIO();
const output = document.getElementById("output")!;

function renderFocusAndSelection(): string {
  const focused = axio.focusedElement;
  const selection = axio.selection;

  let html = '<div class="tier1-section">';
  html += '<div class="section-title">Tier 1: Focus & Selection</div>';

  // Focused element
  if (focused) {
    html += `
      <div class="focus-info">
        <div class="info-label">Focused Element</div>
        <div class="property"><span class="property-key">role</span><span class="property-value">${
          focused.role
        }</span></div>
        <div class="property"><span class="property-key">label</span><span class="property-value">${
          focused.label || "(none)"
        }</span></div>
        <div class="property"><span class="property-key">value</span><span class="property-value">${
          focused.value ? JSON.stringify(focused.value.value) : "(none)"
        }</span></div>
        <div class="property"><span class="property-key">id</span><span class="property-value mono">${
          focused.id
        }</span></div>
      </div>
    `;
  } else {
    html += '<div class="focus-info empty">No focused element</div>';
  }

  // Selection
  if (selection && selection.text) {
    html += `
      <div class="selection-info">
        <div class="info-label">Selected Text</div>
        <div class="selection-text">"${escapeHtml(selection.text)}"</div>
        ${
          selection.range
            ? `<div class="property"><span class="property-key">range</span><span class="property-value">${selection.range.start}..${selection.range.end}</span></div>`
            : ""
        }
      </div>
    `;
  } else {
    html += '<div class="selection-info empty">No text selected</div>';
  }

  html += "</div>";
  return html;
}

function escapeHtml(str: string): string {
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function render() {
  const windows = [...axio.windows.values()];

  let html = renderFocusAndSelection();

  if (windows.length === 0) {
    html += '<div class="connecting">No windows detected</div>';
    output.innerHTML = html;
    return;
  }

  html += windows
    .map((w) => {
      const { x, y, w: width, h: height } = w.bounds;
      return `
        <div class="window-item ${w.focused ? "focused" : ""}">
          <div class="window-title">${w.title || w.app_name || "Untitled"}</div>
          <div class="property"><span class="property-key">id</span><span class="property-value">${
            w.id
          }</span></div>
          <div class="property"><span class="property-key">app</span><span class="property-value">${
            w.app_name
          }</span></div>
          <div class="property"><span class="property-key">position</span><span class="property-value">(${x}, ${y})</span></div>
          <div class="property"><span class="property-key">size</span><span class="property-value">${width} × ${height}</span></div>
        </div>
      `;
    })
    .join("");

  output.innerHTML = html;
}

// Single pattern: connect, then render on any window/focus change
axio.connect().then(() => {
  // sync:init already populated axio.windows, just render
  render();

  // Re-render on any change
  const events = [
    "sync:init",
    "window:added",
    "window:changed",
    "window:removed",
    "focus:window",
    "focus:element",
    "selection:changed",
  ] as const;
  events.forEach((e) => axio.on(e, render));
});
