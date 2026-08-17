// Build and run with: pnpm --filter @burokku/example-layouts dev
//
// The three colored panels are laid out by Taffy's flexbox algorithm. Their
// flex-grow values are 1 : 2 : 1, so the center panel receives twice as much
// of the available row width as either side panel.

import { createElement, setStyles, type BurokkuTagName } from "@burokku/runtime";

type PanelTag = Extract<BurokkuTagName, "div" | "flex" | "grid">;

const body = document.body;
setStyles(body, {
  padding: "32px",
  backgroundColor: "#111827",
});

const row = createElement("flex");
setStyles(row, {
  width: "100%",
  height: "360px",
  padding: "20px",
  gap: "16px",
  alignItems: "stretch",
  backgroundColor: "#1f2937",
});

function panel(tag: PanelTag, grow: number, color: string): HTMLElement {
  const node = createElement(tag);
  setStyles(node, {
    flexBasis: "0px",
    flexGrow: grow,
    backgroundColor: color,
  });
  return node;
}

row.appendChild(panel("div", 1, "#ef4444"));
row.appendChild(panel("grid", 2, "#22c55e"));
row.appendChild(panel("flex", 1, "#3b82f6"));
body.appendChild(row);

const footer = createElement("div");
setStyles(footer, {
  height: "72px",
  margin: "24px",
  backgroundColor: "#f59e0b",
});
body.appendChild(footer);

console.log("Layout demo: red/green/blue widths use a 1:2:1 flex ratio.");
