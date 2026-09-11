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
  "Interact with the div below, or resize the window to watch both layout sizes.",
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

const target = app.createElement("div");
target.setAttribute("data-testid", "event-target");
target.setAttribute("role", "button");
setStyles(target, {
  width: "100%",
  "flex-basis": "0px",
  "flex-grow": "1",
  "background-color": "#0369a1ff",
});
const targetContent = app.createElement("flex");
setStyles(targetContent, {
  width: "100%",
  height: "100%",
  "align-items": "center",
  "justify-content": "center",
});
targetContent.appendChild(makeText("Click, drag, or scroll here", {
  "font-size": "22px",
  "font-weight": "bold",
  color: "#ffffffff",
  "text-wrap": "nowrap",
}));
target.appendChild(targetContent);
eventArea.appendChild(target);

const status = makeText("Status: waiting for input", {
  "font-weight": "bold",
  color: "#fbbf24ff",
  "text-wrap": "nowrap",
});
status.setAttribute("data-testid", "event-status");
eventArea.appendChild(status);

const bubbleStatus = makeText("Bubbling: waiting for input", {
  color: "#a7f3d0ff",
  "text-wrap": "nowrap",
});
bubbleStatus.setAttribute("data-testid", "bubble-status");
eventArea.appendChild(bubbleStatus);

const windowSize = makeText("Window size: waiting for layout", {
  "font-size": "14px",
  "text-wrap": "nowrap",
});
windowSize.setAttribute("data-testid", "window-size");
eventArea.appendChild(windowSize);

const divSize = makeText("Div size: waiting for layout", {
  "font-size": "14px",
  "text-wrap": "nowrap",
});
divSize.setAttribute("data-testid", "div-size");
eventArea.appendChild(divSize);

eventArea.appendChild(makeText(
  "Events bubble through the panel. Press any key while the window is focused.",
  { color: "#cbd5e1ff" },
));

let clicks = 0;
const show = message => {
  status.textContent = `Target: ${message}`;
  console.log(`[events] ${message}`);
};

target.addEventListener("pointerenter", () => {
  target.style.setProperty("background-color", "#0284c7ff");
  show("pointer entered");
});
target.addEventListener("pointerleave", () => {
  target.style.setProperty("background-color", "#0369a1ff");
  show("pointer left");
});
target.addEventListener("pointerdown", event => {
  target.setPointerCapture(event.pointerId);
  show(`pointer ${event.pointerId} down; buttons=${event.buttons}`);
});
target.addEventListener("pointermove", event => {
  if (event.buttons !== 0) {
    show(`dragging at (${event.clientX}, ${event.clientY})`);
  }
});
target.addEventListener("pointerup", event => {
  show(`pointer ${event.pointerId} up`);
  if (target.hasPointerCapture(event.pointerId)) {
    target.releasePointerCapture(event.pointerId);
  }
});
target.addEventListener("gotpointercapture", () => show("pointer captured"));
target.addEventListener("lostpointercapture", () => show("pointer released"));
target.addEventListener("wheel", event => {
  event.preventDefault();
  show(`wheel delta=(${event.deltaX}, ${event.deltaY}), mode=${event.deltaMode}`);
});
target.addEventListener("click", event => {
  clicks += 1;
  show(`click #${clicks} at (${event.clientX}, ${event.clientY})`);
  windowNode.setAttribute("title", `Burokku event dispatch — ${clicks} clicks`);
});

eventArea.addEventListener("click", event => {
  bubbleStatus.textContent =
    `Bubbled: ${event.target.localName} → ${event.currentTarget.localName}`;
});

windowNode.addEventListener("keydown", event => {
  const modifiers = [
    event.metaKey && "meta",
    event.ctrlKey && "ctrl",
    event.altKey && "alt",
    event.shiftKey && "shift",
  ].filter(Boolean).join("+");
  show(`key down: ${modifiers ? `${modifiers}+` : ""}${event.key}`);
});

shell.appendChild(eventArea);
windowNode.appendChild(shell);
app.appendChild(windowNode);

// Keep the size labels outside the observed div so updating them does not change
// its contents. One observer receives changes for both targets in the same batch.
const resizeObserver = new ResizeObserver(entries => {
  for (const entry of entries) {
    const { width, height } = entry.size;
    const dimensions = `${width.toFixed(1)} × ${height.toFixed(1)} logical px`;
    if (entry.target === windowNode) {
      windowSize.textContent = `Window size: ${dimensions}`;
      console.log(`[resize] window: ${width} × ${height}`);
    } else if (entry.target === target) {
      divSize.textContent = `Div size: ${dimensions}`;
      console.log(`[resize] div: ${width} × ${height}`);
    }
  }
});
resizeObserver.observe(windowNode);
resizeObserver.observe(target);
