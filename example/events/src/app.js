const setStyles = (node, styles) => {
  for (const [property, value] of Object.entries(styles)) {
    node.style.setProperty(property, value);
  }
};

const makeText = (content, styles = {}) => {
  const element = app.createElement("text");
  setStyles(element, {
    "font-family": "Noto Sans",
    "font-size": "16px",
    color: "#e2e8f0ff",
    "text-wrap": "wrap",
    ...styles,
  });
  element.appendChild(app.createTextNode(content));
  return element;
};

const windowNode = app.createElement("window");
windowNode.setAttribute("title", "Burokku event dispatch");
setStyles(windowNode, {
  width: "720px",
  height: "560px",
  "background-color": "#0f172aff",
});

const shell = app.createElement("flex");
setStyles(shell, {
  width: "100%",
  height: "100%",
  padding: "24px",
  gap: "16px",
  "flex-direction": "column",
  "background-color": "#0f172aff",
});

shell.appendChild(makeText("UI element event dispatch", {
  "font-size": "30px",
  "font-weight": "bold",
  color: "#ffffffff",
  "text-wrap": "nowrap",
}));
shell.appendChild(makeText(
  "Interact with the element below. Dispatched events will update this window in the next step.",
  { color: "#94a3b8ff" },
));

const eventArea = app.createElement("flex");
eventArea.setAttribute("data-testid", "event-area");
setStyles(eventArea, {
  width: "100%",
  "flex-basis": "0px",
  "flex-grow": "1",
  padding: "16px",
  gap: "12px",
  "flex-direction": "column",
  "background-color": "#1e293bff",
});

eventArea.appendChild(makeText("Event target", {
  "font-size": "18px",
  "font-weight": "bold",
  color: "#7dd3fcff",
  "text-wrap": "nowrap",
}));

const target = app.createElement("flex");
target.setAttribute("data-testid", "event-target");
target.setAttribute("role", "button");
setStyles(target, {
  width: "100%",
  "flex-basis": "0px",
  "flex-grow": "1",
  "align-items": "center",
  "justify-content": "center",
  "background-color": "#0369a1ff",
});
target.appendChild(makeText("Click, drag, or scroll here", {
  "font-size": "22px",
  "font-weight": "bold",
  color: "#ffffffff",
  "text-wrap": "nowrap",
}));
eventArea.appendChild(target);

const status = makeText("Status: waiting for input", {
  "font-weight": "bold",
  color: "#fbbf24ff",
  "text-wrap": "nowrap",
});
status.setAttribute("data-testid", "event-status");
eventArea.appendChild(status);

eventArea.appendChild(makeText(
  "Events bubble from the blue target through this panel. Press any key while the window is focused.",
  { color: "#cbd5e1ff" },
));

shell.appendChild(eventArea);
windowNode.appendChild(shell);
app.appendChild(windowNode);
