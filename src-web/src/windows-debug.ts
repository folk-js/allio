import { Allio } from "allio";

const allio = new Allio();
const output = document.getElementById("output")!;

function renderFocusAndSelection(): string {
  const focused = allio.focusedElement;
  const selection = allio.selection;

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
          focused.value != null ? JSON.stringify(focused.value) : "(none)"
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

/** A Space as people know it: "Desktop 2", or the full-screen window's app. */
function spaceName(id: number): string {
  const s = allio.spaces.find((x) => x.id === id);
  if (!s) return String(id);
  if (s.kind === "desktop") return `Desktop ${s.index + 1}`;
  const app = [...allio.windows.values()].find((w) => w.spaces.includes(id))?.app_name;
  return `${app ?? "Full screen"} (full screen)`;
}

function renderSpaces(): string {
  const items = allio.spaces
    .map((s) => `<span class="property-value">${s.current ? "▸ " : ""}${escapeHtml(spaceName(s.id))} · ${s.id}</span>`)
    .join("<br>");
  return `<div class="window-item"><div class="window-title">Spaces</div>${items || "none"}</div>`;
}

function render() {
  const windows = [...allio.windows.values()];

  let html = renderFocusAndSelection() + renderSpaces();

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
          <div class="property"><span class="property-key">presence</span><span class="property-value">${w.presence}</span></div>
          <div class="property"><span class="property-key">spaces</span><span class="property-value">${
            w.spaces.map(spaceName).join(", ") || "none"
          }</span></div>
        </div>
      `;
    })
    .join("");

  output.innerHTML = html;
}

// Single pattern: connect, then render on any window/focus change
allio.connect().then(() => {
  // sync:init already populated allio.windows, just render
  render();

  // Re-render on any change
  const events = [
    "sync:init",
    "window:added",
    "window:changed",
    "window:removed",
    "focus:window",
    "focus:element",
    "spaces:changed",
    "selection:changed",
  ] as const;
  events.forEach((e) => allio.on(e, render));
});
