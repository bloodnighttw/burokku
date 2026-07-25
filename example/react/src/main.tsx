import { useEffect, useState } from "react";
import { createRoot } from "@burokku/react";

function ScrollablePanel() {
  const colors = ["#e8efff", "#eee9ff", "#e1f7ef", "#fff1dc", "#ffe8ee", "#e8f4ff"];

  return (
    <div
      style={{
        display: "flex",
        width: 386,
        height: 130,
        overflow: "auto",
        backgroundColor: "#eef2f7",
        borderColor: "#c5cfdd",
        borderWidth: 2,
        borderStyle: "solid",
        borderRadius: 12,
      }}
    >
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          width: 520,
          padding: 8,
          gap: 6,
          flexShrink: 0,
        }}
      >
        {colors.map((color, index) => (
          <div
            key={color}
            style={{
              display: "flex",
              width: 500,
              height: 38,
              padding: 10,
              flexShrink: 0,
              backgroundColor: color,
              borderRadius: 8,
            }}
          >
            <span style={{ color: "#263246", fontSize: 14, lineHeight: "18px", fontWeight: 700 }}>
              Scroll item {index + 1} · drag either thumb or use the mouse wheel
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

function BorderPositioningPanel() {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        padding: 14,
        gap: 10,
        backgroundColor: "#ffffff",
        borderColor: "#dce1e8",
        borderWidth: 1,
        borderStyle: "solid",
        borderRadius: 12,
      }}
    >
      <span style={{ color: "#18202b", fontSize: 16, lineHeight: "22px", fontWeight: 700 }}>
        Borders & positioning
      </span>
      <span style={{ color: "#526071", fontSize: 12, lineHeight: "18px" }}>
        Per-side borders, elliptical radii, and all four position modes
      </span>
      <div
        style={{
          height: 72,
          padding: 12,
          backgroundColor: "#f8fbff",
          borderTopWidth: 3,
          borderRightWidth: 7,
          borderBottomWidth: 5,
          borderLeftWidth: 10,
          borderTopColor: "#ef476f",
          borderRightColor: "#118ab2",
          borderBottomColor: "#06d6a0",
          borderLeftColor: "#8338ec",
          borderTopStyle: "dashed",
          borderRightStyle: "double",
          borderBottomStyle: "dotted",
          borderLeftStyle: "solid",
          borderRadius: "28px 10px / 12px 30px",
        }}
      >
        <span style={{ color: "#263246", fontSize: 13, lineHeight: "18px", fontWeight: 700 }}>
          Four independent border sides
        </span>
      </div>
      <div
        style={{
          position: "relative",
          height: 104,
          padding: 12,
          backgroundColor: "#edf2ff",
          borderRadius: 12,
        }}
      >
        <span style={{ position: "static", color: "#526071", fontSize: 12, lineHeight: "18px" }}>
          static flow
        </span>
        <span
          style={{
            position: "relative",
            left: 22,
            top: 12,
            color: "#3158aa",
            fontSize: 14,
            lineHeight: "18px",
            fontWeight: 700,
          }}
        >
          relative offset
        </span>
        <span
          style={{
            position: "absolute",
            right: 10,
            bottom: 8,
            padding: 6,
            backgroundColor: "#3158aa",
            color: "#ffffff",
            borderRadius: 7,
            fontSize: 11,
            lineHeight: "16px",
          }}
        >
          absolute
        </span>
      </div>
      <span
        style={{
          position: "fixed",
          top: 10,
          right: 12,
          padding: "5px 9px",
          backgroundColor: "#172033",
          color: "#ffffff",
          borderRadius: 8,
          fontSize: 11,
          lineHeight: "16px",
        }}
      >
        fixed to viewport
      </span>
    </div>
  );
}

function App() {
  const [remaining, setRemaining] = useState(10);

  useEffect(() => {
    const interval = setInterval(() => {
      setRemaining((current) => {
        if (current <= 1) {
          clearInterval(interval);
          return 0;
        }
        return current - 1;
      });
    }, 1000);

    return () => clearInterval(interval);
  }, []);

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        width: 420,
        padding: 24,
        gap: 16,
        backgroundColor: "#f5f7fa",
        borderColor: "#cbd2dc",
        borderWidth: 1,
        borderStyle: "solid",
        borderRadius: 16,
      }}
    >
      <span style={{ display: "flex", flexDirection: "row", gap: 6 }}>
        <span style={{ color: "#18202b", fontSize: 28, lineHeight: "34px", fontWeight: 700 }}>
          Burokku
        </span>
        <span style={{ color: "#526071", fontSize: 28, lineHeight: "34px" }}>React DOM</span>
      </span>
      <span
        style={{
          display: "flex",
          flexDirection: "column",
          padding: 18,
          backgroundColor: "#ffffff",
          borderColor: "#dce1e8",
          borderWidth: 1,
          borderStyle: "solid",
          borderRadius: 12,
        }}
      >
        <span style={{ color: "#526071", fontSize: 16, lineHeight: "24px" }}>
          Countdown
        </span>
        <span style={{ color: "#18202b", fontSize: 52, lineHeight: "60px", fontWeight: 700 }}>
          {remaining}
        </span>
      </span>
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          padding: 14,
          gap: 10,
          backgroundColor: "#ffffff",
          borderColor: "#dce1e8",
          borderWidth: 1,
          borderStyle: "solid",
          borderRadius: 12,
        }}
      >
        <span style={{ color: "#18202b", fontSize: 16, lineHeight: "22px", fontWeight: 700 }}>
          Usable scroll container
        </span>
        <ScrollablePanel />
      </div>
      <BorderPositioningPanel />
    </div>
  );
}

document.body.style.overflowY = "auto";
document.body.style.overflowX = "auto";

createRoot(document.body).render(<App />);
