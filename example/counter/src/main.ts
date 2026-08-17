// Build and run with: pnpm --filter @burokku/example-counter dev
//
// Click the purple plus button. Each click increments the counter, updates the
// label, grows the colored counter bar, and cycles its color.

import { createElement, setStyles } from "@burokku/runtime";

const colors = ["#22c55e", "#3b82f6", "#f59e0b", "#ef4444", "#a855f7"] as const;
let count = 0;

setStyles(document.body, {
  padding: "32px",
  backgroundColor: "#111827",
});

const card = createElement("flex");
setStyles(card, {
  flexDirection: "column",
  width: "440px",
  padding: "24px",
  gap: "20px",
  backgroundColor: "#1f2937",
});

const title = createElement("text");
title.textContent = "Click counter";

const counterLabel = createElement("text");

// The colored bar makes counter changes visible even while text rendering is
// still minimal: it changes color and grows after every click.
const counterBar = createElement("div");
setStyles(counterBar, {
  height: "88px",
  backgroundColor: colors[0],
});

const controls = createElement("flex");
setStyles(controls, {
  height: "96px",
  gap: "18px",
  alignItems: "center",
});

const plusButton = createElement("flex");
plusButton.setAttribute("role", "button");
plusButton.setAttribute("aria-label", "Increment counter");
setStyles(plusButton, {
  flexDirection: "column",
  width: "72px",
  height: "72px",
  padding: "12px",
  gap: "2px",
  backgroundColor: "#7c3aed",
});

// Draw a visible plus from colored div cells, so the example does not depend
// on glyph rendering for the button icon.
for (const rowPattern of [
  [false, true, false],
  [true, true, true],
  [false, true, false],
]) {
  const iconRow = createElement("flex");
  setStyles(iconRow, {
    width: "48px",
    height: "14px",
    gap: "2px",
  });

  for (const filled of rowPattern) {
    const cell = createElement("div");
    setStyles(cell, {
      width: "14px",
      height: "14px",
      backgroundColor: filled ? "#ffffff" : "#7c3aed",
    });
    iconRow.appendChild(cell);
  }
  plusButton.appendChild(iconRow);
}

const hint = createElement("text");
hint.textContent = "Press + to increment";

function renderCounter(): void {
  counterLabel.textContent = `Count: ${count}`;
  setStyles(counterBar, {
    width: `${180 + Math.min(count, 10) * 20}px`,
    backgroundColor: colors[count % colors.length],
  });
  console.log(`Counter: ${count}`);
}

plusButton.addEventListener("click", () => {
  count += 1;
  renderCounter();
});

controls.appendChild(plusButton);
controls.appendChild(hint);
card.appendChild(title);
card.appendChild(counterLabel);
card.appendChild(counterBar);
card.appendChild(controls);
document.body.appendChild(card);

renderCounter();
