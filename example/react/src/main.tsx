import { useEffect, useState } from "react";
import { createRoot } from "@burokku/react";

function OverflowBar({ mode }: { mode: "hidden" | "clip" }) {
  const colors =
    mode === "hidden"
      ? ["#2850dc", "#4f72e6", "#7e99ef", "#a9baf5"]
      : ["#7c3aed", "#965cef", "#af7cf4", "#c7a5f8"];

  return (
    <div style={{ display: "flex", flexDirection: "column", width: 184, gap: 7 }}>
      <span style={{ color: "#526071", fontSize: 13, lineHeight: "18px", fontWeight: 700 }}>
        overflow: {mode}
      </span>
      <div
        style={{
          display: "flex",
          width: 180,
          height: 24,
          overflow: mode,
          backgroundColor: "#dce3ee",
          borderColor: "#c5cfdd",
          borderWidth: 2,
          borderRadius: 12,
        }}
      >
        <div style={{ display: "flex", width: 260, height: 24, flexShrink: 0 }}>
          {colors.map((color) => (
            <div key={color} style={{ width: 65, height: 24, flexShrink: 0, backgroundColor: color }} />
          ))}
        </div>
      </div>
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
          borderRadius: 12,
        }}
      >
        <span style={{ color: "#18202b", fontSize: 16, lineHeight: "22px", fontWeight: 700 }}>
          Descendant clipping
        </span>
        <div style={{ display: "flex", flexDirection: "row", gap: 10 }}>
          <OverflowBar mode="hidden" />
          <OverflowBar mode="clip" />
        </div>
      </div>
    </div>
  );
}

createRoot(document.body).render(<App />);
