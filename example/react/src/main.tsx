import { createRoot } from "@burokku/react";
import { BorderPositionExample } from "./examples/BorderPositionExample";
import { ControlsExample } from "./examples/ControlsExample";
import { LayoutExample } from "./examples/LayoutExample";
import { OverflowExample } from "./examples/OverflowExample";
import { PaintExample } from "./examples/PaintExample";
import { palette } from "./examples/shared";
import { TypographyExample } from "./examples/TypographyExample";

function App() {
  return (
    <div
      style={{
        display: "flex",
        width: 720,
        height: 620,
        overflow: "auto",
        padding: 18,
        backgroundColor: palette.canvas,
      }}
    >
      <div style={{ display: "flex", flexDirection: "column", width: 666, gap: 14 }}>
        <div style={{ display: "flex", flexDirection: "column", gap: 4, padding: "4px 2px 10px" }}>
          <span style={{ color: palette.ink, fontSize: 30, lineHeight: "36px", fontWeight: 700 }}>
            Burokku React feature gallery
          </span>
          <span style={{ color: palette.muted, fontSize: 14, lineHeight: "20px" }}>
            Runnable examples for every active feature completed after overflow.
          </span>
        </div>
        <OverflowExample />
        <BorderPositionExample />
        <LayoutExample />
        <TypographyExample />
        <PaintExample />
        <ControlsExample />
      </div>
    </div>
  );
}

createRoot(document.body).render(<App />);
