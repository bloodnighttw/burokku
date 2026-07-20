import { useEffect, useState } from "react";
import { createRoot } from "@burokku/react";

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
          padding: 24,
          backgroundColor: "#ffffff",
          borderColor: "#dce1e8",
          borderWidth: 1,
          borderRadius: 12,
        }}
      >
        <span style={{ color: "#526071", fontSize: 16, lineHeight: "24px" }}>
          Countdown
        </span>
        <span style={{ color: "#18202b", fontSize: 72, lineHeight: "82px", fontWeight: 700 }}>
          {remaining}
        </span>
      </span>
      <button
        style={{
          display: "flex",
          padding: 12,
          backgroundColor: "#2850dc",
          borderRadius: 10,
        }}
      >
        <span style={{ color: "#ffffff", fontSize: 16, lineHeight: "20px", fontWeight: 700 }}>
          {remaining === 0 ? "Finished" : "Counting down…"}
        </span>
      </button>
    </div>
  );
}

createRoot(document.body).render(<App />);
