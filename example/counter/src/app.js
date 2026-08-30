const setStyles = (node, styles) => {
  for (const [property, value] of Object.entries(styles)) {
    node.style.setProperty(property, value);
  }
};

const windowNode = app.createElement("window");
windowNode.setAttribute("title", "LLRT counter — 0");
setStyles(windowNode, {
  "background-color": "#0f172aff",
});

const panel = app.createElement("flex");
setStyles(panel, {
  width: "100%",
  height: "100%",
  "flex-direction": "column",
  "align-items": "center",
  "justify-content": "center",
  gap: "16px",
  "background-color": "#111827ff",
});

const heading = app.createElement("text");
setStyles(heading, {
  "font-family": "Noto Sans",
  "font-size": "22px",
  "font-weight": "bold",
  color: "#94a3b8ff",
  "text-wrap": "nowrap",
});
heading.appendChild(app.createTextNode("LLRT setInterval counter"));

const value = app.createElement("text");
setStyles(value, {
  "font-family": "Noto Sans",
  "font-size": "72px",
  "font-weight": "bold",
  color: "#38bdf8ff",
  "text-wrap": "nowrap",
});
const valueText = app.createTextNode("0");
value.appendChild(valueText);

const caption = app.createElement("text");
setStyles(caption, {
  "font-family": "Noto Sans",
  "font-size": "16px",
  color: "#e2e8f0ff",
  "text-wrap": "nowrap",
});
caption.appendChild(app.createTextNode("Updated every second on the UI-thread LLRT runtime"));

panel.appendChild(heading);
panel.appendChild(value);
panel.appendChild(caption);
windowNode.appendChild(panel);
app.appendChild(windowNode);

let count = 0;
setInterval(() => {
  count += 1;
  valueText.data = String(count);
  windowNode.setAttribute("title", `LLRT counter — ${count}`);
  console.log(`[counter] ${count}`);
}, globalThis.__BUROKKU_COUNTER_INTERVAL_MS__ ?? 1000);
