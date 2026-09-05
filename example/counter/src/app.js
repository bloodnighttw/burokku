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
  gap: "4px",
  "background-color": "#111827ff",
});

const counterColumn = app.createElement("flex");
setStyles(counterColumn, {
  width: "100%",
  height: "100%",
  "flex-direction": "column",
  "align-items": "center",
  "justify-content": "center",
  gap: "16px",
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

const controls = app.createElement("flex");
setStyles(controls, {
  margin: "64px",
  padding: "32px",
  "flex-direction": "row",
  "align-items": "center",
  "justify-content": "center",
  gap: "8px",
  "background-color": "#1e293bff",
});

const controlsHeading = app.createElement("text");
setStyles(controlsHeading, {
  "font-family": "Noto Sans",
  "font-size": "16px",
  "font-weight": "bold",
  color: "#94a3b8ff",
  "text-wrap": "nowrap",
});
controlsHeading.appendChild(app.createTextNode("Click controls"));

const makeButton = (label, color) => {
  const button = app.createElement("flex");
  button.setAttribute("role", "button");
  setStyles(button, {
    width: "180px",
    height: "48px",
    "align-items": "center",
    "justify-content": "center",
    "background-color": color,
  });

  const text = app.createElement("text");
  setStyles(text, {
    "font-family": "Noto Sans",
    "font-size": "18px",
    "font-weight": "bold",
    color: "#ffffffff",
    "text-wrap": "nowrap",
  });
  text.appendChild(app.createTextNode(label));
  button.appendChild(text);
  return button;
};

const decrementButton = makeButton("− Decrease", "#dc2626ff");
const incrementButton = makeButton("+ Increase", "#16a34aff");
controls.appendChild(controlsHeading);
controls.appendChild(decrementButton);
controls.appendChild(incrementButton);

counterColumn.appendChild(heading);
counterColumn.appendChild(value);
counterColumn.appendChild(caption);
panel.appendChild(counterColumn);
panel.appendChild(controls);
windowNode.appendChild(panel);
app.appendChild(windowNode);

let count = 0;
decrementButton.addEventListener("click", () => {
  count -= 1;
  valueText.data = String(count);
  windowNode.setAttribute("title", `LLRT counter — ${count}`);
});
incrementButton.addEventListener("click", () => {
  count += 1;
  valueText.data = String(count);
  windowNode.setAttribute("title", `LLRT counter — ${count}`);
});
setInterval(() => {
  count += 1;
  valueText.data = String(count);
  windowNode.setAttribute("title", `LLRT counter — ${count}`);
  console.log(`[counter] ${count}`);
}, globalThis.__BUROKKU_COUNTER_INTERVAL_MS__ ?? 1000);
