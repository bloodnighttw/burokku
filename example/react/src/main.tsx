import { useEffect, useState, type ReactNode } from "react";
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

function FlexExample() {
  const colors = ["#dbe7ff", "#ddd6fe", "#cff4e4"];

  return (
    <div
      style={{
        display: "flex",
        height: 80,
        padding: 8,
        gap: 8,
        backgroundColor: "#eef2f7",
        borderColor: "#c5cfdd",
        borderWidth: 2,
        borderRadius: 10,
      }}
    >
      {colors.map((color, index) => (
        <div
          key={color}
          style={{
            display: "flex",
            width: 0,
            padding: 12,
            flexGrow: index === 1 ? 2 : 1,
            backgroundColor: color,
            borderRadius: 7,
          }}
        >
          <span style={{ color: "#263246", fontSize: 14, lineHeight: "18px", fontWeight: 700 }}>
            {index === 1 ? "2×" : "1×"}
          </span>
        </div>
      ))}
    </div>
  );
}

function GridExample() {
  const items = [
    { label: "span 2", color: "#dbe7ff", column: "span 2" },
    { label: "1", color: "#ddd6fe" },
    { label: "1", color: "#cff4e4" },
    { label: "span 2", color: "#ffdfba", column: "span 2" },
  ];

  return (
    <div
      style={{
        display: "grid",
        height: 88,
        padding: 8,
        gap: 8,
        gridTemplateColumns: "1fr 1fr 1fr",
        gridTemplateRows: "40px 40px",
        backgroundColor: "#eef2f7",
        borderColor: "#c5cfdd",
        borderWidth: 2,
        borderRadius: 10,
      }}
    >
      {items.map((item, index) => (
        <div
          key={`${item.label}-${index}`}
          style={{
            display: "flex",
            padding: 10,
            gridColumn: item.column,
            backgroundColor: item.color,
            borderRadius: 7,
          }}
        >
          <span style={{ color: "#263246", fontSize: 14, lineHeight: "18px", fontWeight: 700 }}>
            {item.label}
          </span>
        </div>
      ))}
    </div>
  );
}

function ExampleCard({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        width: 0,
        padding: 14,
        gap: 10,
        flexGrow: 1,
        backgroundColor: "#ffffff",
        borderColor: "#dce1e8",
        borderWidth: 1,
        borderRadius: 12,
      }}
    >
      <span style={{ color: "#18202b", fontSize: 16, lineHeight: "22px", fontWeight: 700 }}>
        {title}
      </span>
      {children}
    </div>
  );
}

function TypographyExamples() {
  const panelStyle = {
    display: "flex",
    flexDirection: "column",
    width: 0,
    minWidth: 0,
    padding: 14,
    gap: 8,
    flexGrow: 1,
    backgroundColor: "#ffffff",
    borderColor: "#dce1e8",
    borderWidth: 1,
    borderRadius: 12,
  } as const;

  const labelStyle = {
    color: "#526071",
    fontSize: 13,
    lineHeight: "18px",
    fontWeight: 700,
    letterSpacing: 0.8,
  } as const;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <span
        style={{
          color: "#18202b",
          fontSize: 20,
          lineHeight: "26px",
          fontWeight: 700,
          letterSpacing: -0.3,
        }}
      >
        Typography
      </span>
      <div style={{ display: "flex", gap: 12 }}>
        <div style={panelStyle}>
          <span style={labelStyle}>FONT STACK · STYLE · SPACING</span>
          <span
            style={{
              color: "#312e81",
              fontFamily: '"Charter", Georgia, serif',
              fontSize: 22,
              lineHeight: "30px",
              fontStyle: "italic",
              letterSpacing: 1,
              wordSpacing: 5,
            }}
          >
            Fallback fonts stay expressive.
          </span>
          <span
            style={{
              color: "#475569",
              fontFamily: '"JetBrains Mono", monospace',
              fontSize: 13,
              lineHeight: "19px",
            }}
          >
            "JetBrains Mono", monospace
          </span>
        </div>
        <div style={panelStyle}>
          <span style={labelStyle}>ALIGNMENT · DECORATION</span>
          <div
            style={{
              width: "100%",
              color: "#0f766e",
              fontSize: 17,
              lineHeight: "25px",
              fontWeight: 700,
              textAlign: "center",
              textDecoration: "underline overline #14b8a6",
            }}
          >
            Centered, underlined, overlined
          </div>
          <div
            style={{
              width: "100%",
              color: "#7c2d12",
              fontSize: 15,
              lineHeight: "23px",
              textAlign: "right",
              textDecorationLine: "line-through",
              textDecorationColor: "#fb923c",
            }}
          >
            Right aligned with a custom color
          </div>
        </div>
      </div>
      <div style={{ display: "flex", gap: 12 }}>
        <div style={panelStyle}>
          <span style={labelStyle}>WHITE-SPACE: PRE-WRAP</span>
          <span
            style={{
              color: "#334155",
              fontFamily: "monospace",
              fontSize: 14,
              lineHeight: "21px",
              whiteSpace: "pre-wrap",
            }}
          >
            {"spaces   remain\nline breaks remain too"}
          </span>
        </div>
        <div style={panelStyle}>
          <span style={labelStyle}>OVERFLOW-WRAP · WORD-BREAK</span>
          <span
            style={{
              width: "100%",
              color: "#334155",
              fontSize: 14,
              lineHeight: "20px",
              overflowWrap: "anywhere",
            }}
          >
            a-very-long-unbroken-identifier-can-wrap-anywhere
          </span>
          <span
            style={{
              width: "100%",
              color: "#64748b",
              fontSize: 13,
              lineHeight: "18px",
              wordBreak: "break-all",
            }}
          >
            word-break: break-all
          </span>
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
        width: 720,
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
      <div style={{ display: "flex", gap: 16 }}>
        <span
          style={{
            display: "flex",
            flexDirection: "column",
            width: 0,
            padding: 18,
            flexGrow: 1,
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
            Usable scroll container
          </span>
          <ScrollablePanel />
        </div>
      </div>
      <div style={{ display: "flex", gap: 16 }}>
        <ExampleCard title="Flex · proportional growth">
          <FlexExample />
        </ExampleCard>
        <ExampleCard title="Grid · three columns">
          <GridExample />
        </ExampleCard>
      </div>
      <TypographyExamples />
    </div>
  );
}

document.body.style.overflowY = "auto";
document.body.style.overflowX = "auto";

createRoot(document.body).render(<App />);
