// Build and run with: pnpm --filter @burokku/example-counter dev
//
// Click the purple plus button. Each click increments the counter, updates the
// label, grows the colored counter bar, and cycles its color.

const colors = ["#22c55e", "#3b82f6", "#f59e0b", "#ef4444", "#a855f7"];
let count = 0;

document.body.style.padding = "32px";
document.body.style.backgroundColor = "#111827";

const card = document.createElement("flex");
card.style.flexDirection = "column";
card.style.width = "440px";
card.style.padding = "24px";
card.style.gap = "20px";
card.style.backgroundColor = "#1f2937";

const title = document.createElement("text");
title.textContent = "Click counter";
title.style.color = "#f9fafb";
title.style.fontSize = "28px";

const counterLabel = document.createElement("text");
counterLabel.style.color = "#ffffff";
counterLabel.style.fontSize = "24px";

// The colored bar makes counter changes visible even while text rendering is
// still minimal: it changes color and grows after every click.
const counterBar = document.createElement("div");
counterBar.style.height = "88px";
counterBar.style.backgroundColor = colors[0];

const controls = document.createElement("flex");
controls.style.height = "96px";
controls.style.gap = "18px";
controls.style.alignItems = "center";

const plusButton = document.createElement("flex");
plusButton.setAttribute("role", "button");
plusButton.setAttribute("aria-label", "Increment counter");
plusButton.style.flexDirection = "column";
plusButton.style.width = "72px";
plusButton.style.height = "72px";
plusButton.style.padding = "12px";
plusButton.style.gap = "2px";
plusButton.style.backgroundColor = "#7c3aed";

// Draw a visible plus from colored div cells, so the example does not depend
// on glyph rendering for the button icon.
for (const rowPattern of [
  [false, true, false],
  [true, true, true],
  [false, true, false],
]) {
  const iconRow = document.createElement("flex");
  iconRow.style.width = "48px";
  iconRow.style.height = "14px";
  iconRow.style.gap = "2px";

  for (const filled of rowPattern) {
    const cell = document.createElement("div");
    cell.style.width = "14px";
    cell.style.height = "14px";
    cell.style.backgroundColor = filled ? "#ffffff" : "#7c3aed";
    iconRow.appendChild(cell);
  }
  plusButton.appendChild(iconRow);
}

const hint = document.createElement("text");
hint.textContent = "Press + to increment";
hint.style.color = "#c4b5fd";
hint.style.fontSize = "18px";

function renderCounter(): void {
  counterLabel.textContent = `Count: ${count}`;
  counterBar.style.width = `${180 + Math.min(count, 10) * 20}px`;
  counterBar.style.backgroundColor = colors[count % colors.length];
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
