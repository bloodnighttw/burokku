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
            <span
              style={{
                color: "#263246",
                fontSize: 14,
                lineHeight: "18px",
                fontWeight: 700,
                whiteSpace: "nowrap",
              }}
            >
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

function PositionExamples() {
  const stageStyle = {
    position: "relative",
    height: 112,
    padding: 10,
    backgroundColor: "#eef2f7",
    borderColor: "#c5cfdd",
    borderWidth: 2,
    borderRadius: 10,
  } as const;

  const badgeStyle = {
    display: "flex",
    height: 24,
    padding: 6,
    color: "#ffffff",
    fontSize: 12,
    lineHeight: "16px",
    fontWeight: 700,
    borderRadius: 6,
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
        Positioning
      </span>
      <div style={{ display: "flex", gap: 12 }}>
        <ExampleCard title="Static ignores inset · relative keeps flow">
          <div style={stageStyle}>
            <div
              style={{
                ...badgeStyle,
                position: "static",
                left: 180,
                top: 50,
                width: 112,
                backgroundColor: "#475569",
              }}
            >
              STATIC · left ignored
            </div>
            <div
              style={{
                ...badgeStyle,
                position: "relative",
                left: 128,
                top: 6,
                width: 116,
                backgroundColor: "#7c3aed",
              }}
            >
              RELATIVE · shifted
            </div>
            <div
              style={{
                ...badgeStyle,
                width: 120,
                backgroundColor: "#0f766e",
              }}
            >
              SIBLING · flow intact
            </div>
          </div>
        </ExampleCard>
        <ExampleCard title="Absolute skips static wrappers">
          <div style={stageStyle}>
            <div
              style={{
                width: 118,
                height: 74,
                marginLeft: 24,
                padding: 8,
                backgroundColor: "#dbeafe",
                borderColor: "#60a5fa",
                borderWidth: 1,
                borderRadius: 7,
              }}
            >
              <span style={{ color: "#1e40af", fontSize: 12, lineHeight: "16px", fontWeight: 700 }}>
                STATIC WRAPPER
              </span>
              <div
                style={{
                  ...badgeStyle,
                  position: "absolute",
                  right: 10,
                  top: 10,
                  width: 104,
                  backgroundColor: "#dc2626",
                }}
              >
                ABS · outer right
              </div>
            </div>
          </div>
        </ExampleCard>
      </div>
      <div style={{ display: "flex", gap: 12 }}>
        <ExampleCard title="Absolute retains DOM scroll + clip">
          <div style={stageStyle}>
            <div
              style={{
                width: "100%",
                height: 88,
                overflowY: "scroll",
                backgroundColor: "#dbeafe",
                borderRadius: 7,
              }}
            >
              <div style={{ height: 180, padding: 8 }}>
                <span style={{ color: "#1e40af", fontSize: 12, lineHeight: "16px", fontWeight: 700 }}>
                  Scroll this panel
                </span>
              </div>
              <div
                style={{
                  ...badgeStyle,
                  position: "absolute",
                  left: 32,
                  top: 76,
                  width: 142,
                  backgroundColor: "#ea580c",
                }}
              >
                ABS · moves + clips
              </div>
            </div>
          </div>
        </ExampleCard>
        <ExampleCard title="Transform contains fixed">
          <div
            style={{
              ...stageStyle,
              overflow: "hidden",
              transform: "translateX(0px)",
            }}
          >
            <div
              style={{
                width: 120,
                height: 58,
                padding: 8,
                backgroundColor: "#dcfce7",
                borderRadius: 7,
              }}
            >
              <span style={{ color: "#166534", fontSize: 12, lineHeight: "16px", fontWeight: 700 }}>
                STATIC DESCENDANT
              </span>
              <div
                style={{
                  ...badgeStyle,
                  position: "fixed",
                  right: 10,
                  bottom: 10,
                  width: 126,
                  backgroundColor: "#059669",
                }}
              >
                FIXED · transformed CB
              </div>
            </div>
          </div>
        </ExampleCard>
      </div>
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

function TypographyExamples({ remaining }: { remaining: number }) {
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
          <span style={labelStyle}>INLINE SPANS · REACTIVE TEXT</span>
          <span
            style={{
              width: 190,
              color: "#334155",
              fontSize: 14,
              lineHeight: "20px",
              overflowWrap: "anywhere",
            }}
          >
            build/
            <span style={{ color: "#7c3aed", fontWeight: 700 }}>
              a-very-long-styled-identifier
            </span>
            /
            <span
              style={{
                color: remaining === 0 ? "#059669" : "#ea580c",
                fontWeight: 700,
                textDecoration: "underline",
              }}
            >
              {remaining}
            </span>
          </span>
          <span
            style={{
              width: 150,
              color: "#64748b",
              fontSize: 13,
              lineHeight: "18px",
              wordBreak: "break-all",
            }}
          >
            mode:<span style={{ color: "#0369a1" }}>break-all-in-one-flow</span>
          </span>
        </div>
      </div>
    </div>
  );
}

function PaintExamples() {
  const rasterBackground =
    'url("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAADklEQVR4nGP4z8AAQv8BD/kD/YURmXYAAAAASUVORK5CYII=")';

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
        Paint
      </span>
      <div style={{ display: "flex", gap: 12 }}>
        <ExampleCard title="Linear gradient · box shadow">
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              height: 104,
              padding: 16,
              gap: 5,
              backgroundImage:
                "linear-gradient(125deg, hsl(252 88% 67%) 0%, rgb(37 99 235) 52%, rgb(6 182 212) 100%)",
              borderRadius: 12,
              boxShadow: "0px 12px 24px -10px rgba(37, 99, 235, 0.75)",
            }}
          >
            <span
              style={{
                color: "white",
                fontSize: 21,
                lineHeight: "27px",
                fontWeight: 700,
                textShadow: "0px 2px 5px rgba(15, 23, 42, 0.45)",
              }}
            >
              Aurora
            </span>
            <span style={{ color: "rgba(255, 255, 255, 0.82)", fontSize: 13, lineHeight: "18px" }}>
              HSL + RGB color stops
            </span>
          </div>
        </ExampleCard>
        <ExampleCard title="Radial gradient · inset shadow">
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              height: 104,
              padding: 16,
              gap: 5,
              backgroundColor: "rebeccapurple",
              backgroundImage:
                "radial-gradient(hsla(48, 100%, 88%, 0.95) 0%, rgba(244, 114, 182, 0.82) 42%, rebeccapurple 100%)",
              borderRadius: 12,
              boxShadow: "inset 0px 0px 22px rgba(49, 46, 129, 0.55)",
            }}
          >
            <span
              style={{
                color: "lightgoldenrodyellow",
                fontSize: 21,
                lineHeight: "27px",
                fontWeight: 700,
                textShadow: "1px 2px 4px navy, -1px 0px 3px rgba(255, 255, 255, 0.4)",
              }}
            >
              Solar bloom
            </span>
            <span style={{ color: "rgb(255 255 255 / 78%)", fontSize: 13, lineHeight: "18px" }}>
              HSLA + expanded named colors
            </span>
          </div>
        </ExampleCard>
      </div>
      <div style={{ display: "flex", gap: 12 }}>
        <ExampleCard title="Opacity · transform group">
          <div
            style={{
              display: "flex",
              height: 92,
              padding: 16,
              backgroundColor: "hsl(220 33% 96%)",
              borderRadius: 12,
            }}
          >
            <div
              style={{
                display: "flex",
                flexDirection: "column",
                width: 215,
                padding: 14,
                gap: 3,
                opacity: 0.72,
                transform: "translate(22px, 1px) rotate(-4deg) scale(1.04)",
                backgroundColor: "rgb(16 185 129)",
                borderRadius: 10,
                boxShadow: "0px 8px 15px rgba(6, 95, 70, 0.4)",
              }}
            >
              <span style={{ color: "white", fontSize: 17, lineHeight: "22px", fontWeight: 700 }}>
                Composited card
              </span>
              <span style={{ color: "rgba(255, 255, 255, 0.9)", fontSize: 12, lineHeight: "17px" }}>
                72% opacity · rotate · scale
              </span>
            </div>
          </div>
        </ExampleCard>
        <ExampleCard title="PNG data URL · text shadow">
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              height: 92,
              padding: 16,
              gap: 4,
              backgroundImage: rasterBackground,
              borderRadius: 12,
              boxShadow: "0px 8px 18px -8px rgba(15, 23, 42, 0.65)",
            }}
          >
            <span
              style={{
                color: "white",
                fontSize: 19,
                lineHeight: "25px",
                fontWeight: 700,
                textShadow: "0px 2px 4px black",
              }}
            >
              Raster fill
            </span>
            <span
              style={{
                color: "white",
                fontSize: 12,
                lineHeight: "17px",
                textShadow: "0px 1px 3px black",
              }}
            >
              Decoded from an embedded PNG
            </span>
          </div>
        </ExampleCard>
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
        fontFamily: '"Helvetica Neue", Arial, sans-serif',
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
      <PositionExamples />
      <TypographyExamples remaining={remaining} />
      <PaintExamples />
      <div
        style={{
          display: "flex",
          position: "fixed",
          right: 12,
          bottom: 12,
          width: 154,
          padding: 10,
          color: "#ffffff",
          fontSize: 12,
          lineHeight: "16px",
          fontWeight: 700,
          backgroundColor: "#0f172a",
          borderRadius: 8,
          boxShadow: "0px 8px 18px rgba(15, 23, 42, 0.35)",
        }}
      >
        FIXED · viewport corner
      </div>
    </div>
  );
}

document.body.style.padding = "12px";
document.body.style.borderWidth = "4px";
document.body.style.borderColor = "#94a3b8";
document.body.style.overflowY = "auto";
document.body.style.overflowX = "scroll";

createRoot(document.body).render(<App />);
