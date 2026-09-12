const setStyles = (node, styles) => {
  for (const [property, value] of Object.entries(styles)) {
    node.style.setProperty(property, value);
  }
};

const makeText = (content, styles = {}) => {
  const text = app.createElement("text");
  setStyles(text, {
    "font-family": "Noto Sans",
    "font-size": "16px",
    color: "#e2e8f0ff",
    "text-wrap": "wrap",
    ...styles,
  });
  text.appendChild(app.createTextNode(content));
  return text;
};

const makeCard = (heading, body, color) => {
  const card = app.createElement("flex");
  setStyles(card, {
    width: "240px",
    height: "300px",
    padding: "18px",
    gap: "12px",
    "flex-direction": "column",
    "background-color": color,
  });
  card.appendChild(makeText(heading, {
    "font-size": "22px",
    "font-weight": "bold",
    color: "#ffffffff",
    "text-wrap": "nowrap",
  }));
  card.appendChild(makeText(body, { "font-size": "15px" }));
  return card;
};

/*
<window title="Burokku mixed positioned layout">
  <flex id="shell">
    <text id="title">Position × mixed layout</text>
    <text id="subtitle">
      Flex shell → relative grid → static wrapper. Coral is absolute; yellow is fixed.
    </text>
    <text id="stage-label">RELATIVE GRID ↓ containing block for the coral overlay</text>
    <grid id="stage" position="relative">
      <flex id="flow-card" position="static">
        <text>Static flex item</text>
        <text>This card participates normally in the grid.</text>
      </flex>
      <div id="static-wrapper" position="static">
        <flex id="overlay-host" position="static">
          <text>Static block wrapper</text>
          <text>Both positioned boxes remain nested here in the DOM.</text>
          <flex id="absolute-overlay" position="absolute" z-index="10" top="20px" right="20px">
            <text>ABSOLUTE — z-index: 10</text>
          </flex>
          <flex id="fixed-footer" position="fixed" z-index="20" bottom="0px" left="0px">
            <text>FIXED — z-index: 20</text>
          </flex>
        </flex>
      </div>
      <flex id="grid-sibling" position="static">
        <text>Normal grid sibling</text>
        <text>The absolute overlay consumes no grid track.</text>
      </flex>
    </grid>
  </flex>
</window>
*/
const windowNode = app.createElement("window");
windowNode.setAttribute("title", "Burokku mixed positioned layout");
setStyles(windowNode, {
  width: "900px",
  height: "640px",
  "background-color": "#020617ff",
});

const shell = app.createElement("flex");
setStyles(shell, {
  width: "100%",
  height: "520px",
  padding: "20px",
  gap: "12px",
  "flex-direction": "column",
  "background-color": "#0f172aff",
});

shell.appendChild(makeText("Position × mixed layout", {
  "font-size": "30px",
  "font-weight": "bold",
  color: "#ffffffff",
  "text-wrap": "nowrap",
}));
shell.appendChild(makeText(
  "Flex shell → relative grid → static wrapper. Coral is absolute; yellow is fixed.",
  { color: "#94a3b8ff", "text-wrap": "nowrap" },
));
shell.appendChild(makeText(
  "RELATIVE GRID ↓ containing block for the coral overlay",
  {
    "font-size": "14px",
    "font-weight": "bold",
    color: "#67e8f9ff",
    "text-wrap": "nowrap",
  },
));

const stage = app.createElement("grid");
setStyles(stage, {
  position: "relative",
  width: "100%",
  height: "360px",
  padding: "16px",
  gap: "16px",
  "grid-auto-flow": "column",
  "align-items": "center",
  "justify-content": "center",
  "background-color": "#1e293bff",
});

const flowCard = makeCard(
  "Static flex item",
  "This card participates normally in the relative grid's column flow.",
  "#065f46ff",
);

const staticWrapper = app.createElement("div");
setStyles(staticWrapper, {
  width: "240px",
  height: "300px",
  "background-color": "#334155ff",
});

const overlayHost = app.createElement("flex");
setStyles(overlayHost, {
  width: "100%",
  height: "100%",
  padding: "18px",
  gap: "12px",
  "flex-direction": "column",
});
overlayHost.appendChild(makeText("Static block wrapper", {
  "font-size": "22px",
  "font-weight": "bold",
  color: "#ffffffff",
  "text-wrap": "nowrap",
}));
overlayHost.appendChild(makeText(
  "The coral and yellow boxes are nested here in the DOM, but use different layout parents.",
  { "font-size": "15px" },
));
staticWrapper.appendChild(overlayHost);

const gridSibling = makeCard(
  "Normal grid sibling",
  "The absolute overlay consumes no grid track, so this remains the third in-flow column.",
  "#1e3a8aff",
);

const absoluteOverlay = app.createElement("flex");
setStyles(absoluteOverlay, {
  position: "absolute",
  "z-index": "10",
  top: "20px",
  right: "20px",
  width: "190px",
  height: "96px",
  padding: "12px",
  "align-items": "center",
  "justify-content": "center",
  "background-color": "#e11d48ee",
});
absoluteOverlay.appendChild(makeText(
  "ABSOLUTE\nz-index: 10; top/right: 20px",
  {
    "font-size": "15px",
    "font-weight": "bold",
    color: "#ffffffff",
  },
));

const fixedFooter = app.createElement("flex");
setStyles(fixedFooter, {
  position: "fixed",
  "z-index": "20",
  bottom: "0px",
  left: "0px",
  width: "100%",
  height: "64px",
  "align-items": "center",
  "justify-content": "center",
  "background-color": "#ca8a04ff",
});
fixedFooter.appendChild(makeText(
  "FIXED — z-index: 20; bottom: 0; containing block: window",
  {
    "font-size": "17px",
    "font-weight": "bold",
    color: "#fff7edff",
    "text-wrap": "nowrap",
  },
));

overlayHost.appendChild(absoluteOverlay);
overlayHost.appendChild(fixedFooter);
stage.appendChild(flowCard);
stage.appendChild(staticWrapper);
stage.appendChild(gridSibling);
shell.appendChild(stage);
windowNode.appendChild(shell);
app.appendChild(windowNode);
