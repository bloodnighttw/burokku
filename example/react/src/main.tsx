import { useEffect, useState, type ReactNode } from "react";
import { createRoot } from "@burokku/react";

function Card({ title, children }: { title: string; children: ReactNode }) {
  return (
    <flex
      style={{
        flexDirection: "column",
        flexBasis: 0,
        flexGrow: 1,
        gap: 6,
        backgroundColor: "#ffffff",
        borderColor: "#d8dee8",
        borderWidth: 1,
        borderRadius: 10,
      }}
    >
      <text
        style={{
          color: "#526071",
          fontSize: 12,
          lineHeight: 16,
          fontWeight: 700,
          letterSpacing: 0.7,
        }}
      >
        {title}
      </text>
      {children}
    </flex>
  );
}

function GridDemo() {
  const cells = [
    ["A", "#dbeafe"],
    ["B", "#ede9fe"],
    ["C", "#d1fae5"],
    ["D", "#ffedd5"],
  ] as const;

  return (
    <grid
      style={{
        gridTemplateColumns: "1fr 1fr",
        gridTemplateRows: "auto auto",
        gap: 6,
        backgroundColor: "#f1f5f9",
        borderRadius: 8,
      }}
    >
      {cells.map(([label, backgroundColor]) => (
        <div key={label} style={{ backgroundColor, borderRadius: 6 }}>
          <text style={{ color: "#263246", fontSize: 15, lineHeight: 20, fontWeight: 700 }}>
            Grid {label}
          </text>
        </div>
      ))}
    </grid>
  );
}

function App() {
  const [remaining, setRemaining] = useState(100);

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
    <window>
      <flex
        style={{
          flexDirection: "column",
          gap: 14,
          backgroundColor: "#f5f7fa",
          backgroundImage: {
            type: "linear-gradient",
            direction: [1, 1],
            stops: [
              { color: "#f8fafc", position: 0 },
              { color: "#e8efff", position: 1 },
            ],
          },
          borderColor: "#cbd2dc",
          borderWidth: 1,
          borderRadius: 14,
        }}
      >
        <flex style={{ gap: 7, alignItems: "baseline" }}>
          <text style={{ color: "#18202b", fontSize: 28, lineHeight: 34, fontWeight: 700 }}>
            Burokku
          </text>
          <text style={{ color: "#526071", fontSize: 20, lineHeight: 28 }}>React Elements</text>
        </flex>

        <flex style={{ gap: 10, alignItems: "stretch" }}>
          <Card title="REACTIVE TEXT">
            <text style={{ color: "#18202b", fontSize: 42, lineHeight: 48, fontWeight: 700 }}>
              {remaining}
            </text>
            <text
              style={{
                color: remaining === 0 ? "#047857" : "#c2410c",
                fontSize: 13,
                lineHeight: 18,
                textDecorationLine: "underline",
              }}
            >
              {remaining === 0 ? "Complete" : "Counting down"}
            </text>
          </Card>

          <Card title="FLEX GROWTH">
            <flex style={{ gap: 5 }}>
              {[1, 2, 1].map((grow, index) => (
                <flex
                  key={`${grow}-${index}`}
                  style={{
                    flexBasis: 0,
                    flexGrow: grow,
                    backgroundColor: index === 1 ? "#ddd6fe" : "#dbeafe",
                    borderRadius: 5,
                  }}
                >
                  <text style={{ color: "#312e81", fontSize: 14, lineHeight: 19, fontWeight: 700 }}>
                    {grow}x
                  </text>
                </flex>
              ))}
            </flex>
          </Card>
        </flex>

        <flex style={{ gap: 10, alignItems: "stretch" }}>
          <Card title="GRID LAYOUT">
            <GridDemo />
          </Card>

          <Card title="RICH TEXT">
            <text
              style={{
                color: "#334155",
                fontFamily: "sans-serif",
                fontSize: 15,
                lineHeight: 22,
                whiteSpace: "pre-wrap",
                overflowWrap: "anywhere",
              }}
            >
              React keeps one inline flow with <text style={{ color: "#7c3aed", fontWeight: 700 }}>nested styles</text>
              {"\nand reactive updates."}
            </text>
          </Card>
        </flex>
      </flex>
    </window>
  );
}

createRoot().render(<App />);
