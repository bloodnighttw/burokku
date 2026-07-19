import { useEffect, useState } from "react";
import { createRoot } from "@burokku/ui/react";

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
        <text style={{ color: "#18202b", fontSize: 28, lineHeight: 34, fontWeight: 700 }}>
          Burokku
        </text>
        <text style={{ color: "#526071", fontSize: 28, lineHeight: 34 }}>React</text>
      </span>
      <span
        style={{
          display: "flex",
          flexDirection: "column",
          padding: 24,
          backgroundColor: "#ffffff",
          borderColor: "#dce1e8",
          borderWidth: 1,
          borderRadius: 12,
        }}
      >
        <text style={{ color: "#526071", fontSize: 16, lineHeight: 24 }}>
          Countdown
        </text>
        <text style={{ color: "#18202b", fontSize: 72, lineHeight: 82, fontWeight: 700 }}>
          {remaining}
        </text>
      </span>
      <button
        style={{
          display: "flex",
          padding: 12,
          backgroundColor: "#2850dc",
          borderRadius: 10,
        }}
      >
        <text style={{ color: "#ffffff", fontSize: 16, lineHeight: 20, fontWeight: 700 }}>
          {remaining === 0 ? "Finished" : "Counting down…"}
        </text>
      </button>
    </div>
  );
}

createRoot().render(<App />);
