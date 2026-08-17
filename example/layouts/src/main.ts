// Build and run with: pnpm --filter @burokku/example-layouts dev
//
// The three colored panels are laid out by Taffy's flexbox algorithm. Their
// flex-grow values are 1 : 2 : 1, so the center panel receives twice as much
// of the available row width as either side panel.

const body = document.body;
body.style.padding = "32px";
body.style.backgroundColor = "#111827";

const row = document.createElement("flex");
row.style.width = "100%";
row.style.height = "360px";
row.style.padding = "20px";
row.style.gap = "16px";
row.style.alignItems = "stretch";
row.style.backgroundColor = "#1f2937";

function panel(tag: string, grow: number, color: string): HTMLElement {
  const node = document.createElement(tag);
  node.style.flexBasis = "0px";
  node.style.flexGrow = String(grow);
  node.style.backgroundColor = color;
  return node;
}

row.appendChild(panel("div", 1, "#ef4444"));
row.appendChild(panel("grid", 2, "#22c55e"));
row.appendChild(panel("flex", 1, "#3b82f6"));
body.appendChild(row);

const footer = document.createElement("div");
footer.style.height = "72px";
footer.style.margin = "24px";
footer.style.backgroundColor = "#f59e0b";
body.appendChild(footer);

console.log("Layout demo: red/green/blue widths use a 1:2:1 flex ratio.");
