import { createSignal, onCleanup, type JSX } from "solid-js";
import { render } from "@burokku/solid";

function ScrollablePanel() {
  const colors = ["#e8efff", "#eee9ff", "#e1f7ef", "#fff1dc", "#ffe8ee", "#e8f4ff"];

  return (
    <div
      style={{
        display: "flex",
        width: "386px",
        height: "130px",
        overflow: "auto",
        "background-color": "#eef2f7",
        "border-color": "#c5cfdd",
        "border-width": "2px",
        "border-radius": "12px",
      }}
    >
      <div
        style={{
          display: "flex",
          "flex-direction": "column",
          width: "520px",
          padding: "8px",
          gap: "6px",
          "flex-shrink": 0,
        }}
      >
        {colors.map((color, index) => (
          <div
            style={{
              display: "flex",
              width: "500px",
              height: "38px",
              padding: "10px",
              "flex-shrink": 0,
              "background-color": color,
              "border-radius": "8px",
            }}
          >
            <text
              style={{
                color: "#263246",
                "font-size": "14px",
                "line-height": "18px",
                "font-weight": 700,
                "white-space": "nowrap",
              }}
            >
              Scroll item {index + 1} · drag either thumb or use the mouse wheel
            </text>
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
        height: "80px",
        padding: "8px",
        gap: "8px",
        "background-color": "#eef2f7",
        "border-color": "#c5cfdd",
        "border-width": "2px",
        "border-radius": "10px",
      }}
    >
      {colors.map((color, index) => (
        <div
          style={{
            display: "flex",
            width: "0px",
            padding: "12px",
            "flex-grow": index === 1 ? 2 : 1,
            "background-color": color,
            "border-radius": "7px",
          }}
        >
          <text style={{ color: "#263246", "font-size": "14px", "line-height": "18px", "font-weight": 700 }}>
            {index === 1 ? "2×" : "1×"}
          </text>
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
        height: "88px",
        padding: "8px",
        gap: "8px",
        "grid-template-columns": "1fr 1fr 1fr",
        "grid-template-rows": "40px 40px",
        "background-color": "#eef2f7",
        "border-color": "#c5cfdd",
        "border-width": "2px",
        "border-radius": "10px",
      }}
    >
      {items.map((item) => (
        <div
          style={{
            display: "flex",
            padding: "10px",
            "grid-column": item.column,
            "background-color": item.color,
            "border-radius": "7px",
          }}
        >
          <text style={{ color: "#263246", "font-size": "14px", "line-height": "18px", "font-weight": 700 }}>
            {item.label}
          </text>
        </div>
      ))}
    </div>
  );
}

function PositionExamples() {
  const stageStyle = {
    position: "relative",
    height: "112px",
    padding: "10px",
    "background-color": "#eef2f7",
    "border-color": "#c5cfdd",
    "border-width": "2px",
    "border-radius": "10px",
  } as const;

  const badgeStyle = {
    display: "flex",
    height: "24px",
    padding: "6px",
    color: "#ffffff",
    "font-size": "12px",
    "line-height": "16px",
    "font-weight": 700,
    "border-radius": "6px",
  } as const;

  return (
    <div style={{ display: "flex", "flex-direction": "column", gap: "12px" }}>
      <text
        style={{
          color: "#18202b",
          "font-size": "20px",
          "line-height": "26px",
          "font-weight": 700,
          "letter-spacing": "-0.3px",
        }}
      >
        Positioning
      </text>
      <div style={{ display: "flex", gap: "12px" }}>
        <ExampleCard title="Static ignores inset · relative keeps flow">
          <div style={stageStyle}>
            <div
              style={{
                ...badgeStyle,
                position: "static",
                left: "180px",
                top: "50px",
                width: "112px",
                "background-color": "#475569",
              }}
            >
              <text>STATIC · left ignored</text>
            </div>
            <div
              style={{
                ...badgeStyle,
                position: "relative",
                left: "128px",
                top: "6px",
                width: "116px",
                "background-color": "#7c3aed",
              }}
            >
              <text>RELATIVE · shifted</text>
            </div>
            <div
              style={{
                ...badgeStyle,
                width: "120px",
                "background-color": "#0f766e",
              }}
            >
              <text>SIBLING · flow intact</text>
            </div>
          </div>
        </ExampleCard>
        <ExampleCard title="Absolute skips static wrappers">
          <div style={stageStyle}>
            <div
              style={{
                width: "118px",
                height: "74px",
                "margin-left": "24px",
                padding: "8px",
                "background-color": "#dbeafe",
                "border-color": "#60a5fa",
                "border-width": "1px",
                "border-radius": "7px",
              }}
            >
              <text style={{ color: "#1e40af", "font-size": "12px", "line-height": "16px", "font-weight": 700 }}>
                STATIC WRAPPER
              </text>
              <div
                style={{
                  ...badgeStyle,
                  position: "absolute",
                  right: "10px",
                  top: "10px",
                  width: "104px",
                  "background-color": "#dc2626",
                }}
              >
                <text>ABS · outer right</text>
              </div>
            </div>
          </div>
        </ExampleCard>
      </div>
      <div style={{ display: "flex", gap: "12px" }}>
        <ExampleCard title="Absolute retains DOM scroll + clip">
          <div style={stageStyle}>
            <div
              style={{
                width: "100%",
                height: "88px",
                "overflow-y": "scroll",
                "background-color": "#dbeafe",
                "border-radius": "7px",
              }}
            >
              <div style={{ height: "180px", padding: "8px", "background-color": "#eeeeee", }}>
                <text style={{ color: "#1e40af", "font-size": "12px", "line-height": "16px", "font-weight": 700 }}>
                  Scroll this panel
                </text>
              </div>
              <div style={{
                  height: "400px",
                  width: "240px",
                  "background-color": "#ea580c",
              }}>

                <div
                  style={{
                    ...badgeStyle,
                    position: "absolute",
                    left: "32px",
                    top: "76px",
                    width: "142px",
                    "background-color": "#ea580c",
                  }}
                >
                  <text>ABsssS · moves + clips</text>
                </div>
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
                width: "120px",
                height: "58px",
                padding: "8px",
                "background-color": "#dcfce7",
                "border-radius": "7px",
              }}
            >
              <text style={{ color: "#166534", "font-size": "12px", "line-height": "16px", "font-weight": 700 }}>
                STATIC DESCENDANT
              </text>
              <div
                style={{
                  ...badgeStyle,
                  position: "fixed",
                  right: "10px",
                  bottom: "10px",
                  width: "126px",
                  "background-color": "#059669",
                }}
              >
                <text>FIXED · transformed CB</text>
              </div>
            </div>
          </div>
        </ExampleCard>
      </div>
    </div>
  );
}

function ExampleCard(props: { title: string; children: JSX.Element }) {
  return (
    <div
      style={{
        display: "flex",
        "flex-direction": "column",
        width: "0px",
        padding: "14px",
        gap: "10px",
        "flex-grow": 1,
        "background-color": "#ffffff",
        "border-color": "#dce1e8",
        "border-width": "1px",
        "border-radius": "12px",
      }}
    >
      <text style={{ color: "#18202b", "font-size": "16px", "line-height": "22px", "font-weight": 700 }}>
        {props.title}
      </text>
      {props.children}
    </div>
  );
}

function TypographyExamples(props: { remaining: number }) {
  const panelStyle = {
    display: "flex",
    "flex-direction": "column",
    width: "0px",
    "min-width": "0px",
    padding: "14px",
    gap: "8px",
    "flex-grow": 1,
    "background-color": "#ffffff",
    "border-color": "#dce1e8",
    "border-width": "1px",
    "border-radius": "12px",
  } as const;

  const labelStyle = {
    color: "#526071",
    "font-size": "13px",
    "line-height": "18px",
    "font-weight": 700,
    "letter-spacing": "0.8px",
  } as const;

  return (
    <div style={{ display: "flex", "flex-direction": "column", gap: "12px" }}>
      <text
        style={{
          color: "#18202b",
          "font-size": "20px",
          "line-height": "26px",
          "font-weight": 700,
          "letter-spacing": "-0.3px",
        }}
      >
        Typography
      </text>
      <div style={{ display: "flex", gap: "12px" }}>
        <div style={panelStyle}>
          <text style={labelStyle}>FONT STACK · STYLE · SPACING</text>
          <text
            style={{
              color: "#312e81",
              "font-family": '"Charter", Georgia, serif',
              "font-size": "22px",
              "line-height": "30px",
              "font-style": "italic",
              "letter-spacing": "1px",
              "word-spacing": "5px",
            }}
          >
            Fallback fonts stay expressive.
          </text>
          <text
            style={{
              color: "#475569",
              "font-family": '"JetBrains Mono", monospace',
              "font-size": "13px",
              "line-height": "19px",
            }}
          >
            "JetBrains Mono", monospace
          </text>
        </div>
        <div style={panelStyle}>
          <text style={labelStyle}>ALIGNMENT · DECORATION</text>
          <text
            style={{
              width: "100%",
              color: "#0f766e",
              "font-size": "17px",
              "line-height": "25px",
              "font-weight": 700,
              "text-align": "center",
              "text-decoration": "underline overline #14b8a6",
            }}
          >
            Centered, underlined, overlined
          </text>
          <text
            style={{
              width: "100%",
              color: "#7c2d12",
              "font-size": "15px",
              "line-height": "23px",
              "text-align": "right",
              "text-decoration-line": "line-through",
              "text-decoration-color": "#fb923c",
            }}
          >
            Right aligned with a custom color
          </text>
        </div>
      </div>
      <div style={{ display: "flex", gap: "12px" }}>
        <div style={panelStyle}>
          <text style={labelStyle}>WHITE-SPACE: PRE-WRAP</text>
          <text
            style={{
              color: "#334155",
              "font-family": "monospace",
              "font-size": "14px",
              "line-height": "21px",
              "white-space": "pre-wrap",
            }}
          >
            {"spaces   remain\nline breaks remain too"}
          </text>
        </div>
        <div style={panelStyle}>
          <text style={labelStyle}>NESTED TEXT · REACTIVE STYLES</text>
          <text
            style={{
              width: "190px",
              color: "#334155",
              "font-size": "14px",
              "line-height": "20px",
              "overflow-wrap": "anywhere",
            }}
          >
            build/
            <text style={{ color: "#7c3aed", "font-weight": 700 }}>
              a-very-long-styled-identifier
            </text>
            /
            <text
              style={{
                color: props.remaining === 0 ? "#059669" : "#ea580c",
                "font-weight": 700,
                "text-decoration": "underline",
              }}
            >
              {props.remaining}
            </text>
          </text>
          <text
            style={{
              width: "150px",
              color: "#64748b",
              "font-size": "13px",
              "line-height": "18px",
              "word-break": "break-all",
            }}
          >
            mode:<text style={{ color: "#0369a1" }}>break-all-in-one-flow</text>
          </text>
        </div>
      </div>
    </div>
  );
}

function PaintExamples() {
  const rasterBackground =
    'url("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAADklEQVR4nGP4z8AAQv8BD/kD/YURmXYAAAAASUVORK5CYII=")';

  return (
    <div style={{ display: "flex", "flex-direction": "column", gap: "12px" }}>
      <text
        style={{
          color: "#18202b",
          "font-size": "20px",
          "line-height": "26px",
          "font-weight": 700,
          "letter-spacing": "-0.3px",
        }}
      >
        Paint
      </text>
      <div style={{ display: "flex", gap: "12px" }}>
        <ExampleCard title="Linear gradient · box shadow">
          <div
            style={{
              display: "flex",
              "flex-direction": "column",
              height: "104px",
              padding: "16px",
              gap: "5px",
              "background-image":
                "linear-gradient(125deg, hsl(252 88% 67%) 0%, rgb(37 99 235) 52%, rgb(6 182 212) 100%)",
              "border-radius": "12px",
              "box-shadow": "0px 12px 24px -10px rgba(37, 99, 235, 0.75)",
            }}
          >
            <text
              style={{
                color: "white",
                "font-size": "21px",
                "line-height": "27px",
                "font-weight": 700,
              }}
            >
              Aurora
            </text>
            <text style={{ color: "rgba(255, 255, 255, 0.82)", "font-size": "13px", "line-height": "18px" }}>
              HSL + RGB color stops
            </text>
          </div>
        </ExampleCard>
        <ExampleCard title="Radial gradient · inset shadow">
          <div
            style={{
              display: "flex",
              "flex-direction": "column",
              height: "104px",
              padding: "16px",
              gap: "5px",
              "background-color": "rebeccapurple",
              "background-image":
                "radial-gradient(hsla(48, 100%, 88%, 0.95) 0%, rgba(244, 114, 182, 0.82) 42%, rebeccapurple 100%)",
              "border-radius": "12px",
              "box-shadow": "inset 0px 0px 22px rgba(49, 46, 129, 0.55)",
            }}
          >
            <text
              style={{
                color: "lightgoldenrodyellow",
                "font-size": "21px",
                "line-height": "27px",
                "font-weight": 700,
              }}
            >
              Solar bloom
            </text>
            <text style={{ color: "rgb(255 255 255 / 78%)", "font-size": "13px", "line-height": "18px" }}>
              HSLA + expanded named colors
            </text>
          </div>
        </ExampleCard>
      </div>
      <div style={{ display: "flex", gap: "12px" }}>
        <ExampleCard title="Opacity · transform group">
          <div
            style={{
              display: "flex",
              height: "92px",
              padding: "16px",
              "background-color": "hsl(220 33% 96%)",
              "border-radius": "12px",
            }}
          >
            <div
              style={{
                display: "flex",
                "flex-direction": "column",
                width: "215px",
                padding: "14px",
                gap: "3px",
                opacity: 0.72,
                transform: "translate(22px, 1px) rotate(-4deg) scale(1.04)",
                "background-color": "rgb(16 185 129)",
                "border-radius": "10px",
                "box-shadow": "0px 8px 15px rgba(6, 95, 70, 0.4)",
              }}
            >
              <text style={{ color: "white", "font-size": "17px", "line-height": "22px", "font-weight": 700 }}>
                Composited card
              </text>
              <text style={{ color: "rgba(255, 255, 255, 0.9)", "font-size": "12px", "line-height": "17px" }}>
                72% opacity · rotate · scale
              </text>
            </div>
          </div>
        </ExampleCard>
        <ExampleCard title="PNG data URL · raster fill">
          <div
            style={{
              display: "flex",
              "flex-direction": "column",
              height: "92px",
              padding: "16px",
              gap: "4px",
              "background-image": rasterBackground,
              "border-radius": "12px",
              "box-shadow": "0px 8px 18px -8px rgba(15, 23, 42, 0.65)",
            }}
          >
            <text
              style={{
                color: "white",
                "font-size": "19px",
                "line-height": "25px",
                "font-weight": 700,
              }}
            >
              Raster fill
            </text>
            <text
              style={{
                color: "white",
                "font-size": "12px",
                "line-height": "17px",
              }}
            >
              Decoded from an embedded PNG
            </text>
          </div>
        </ExampleCard>
      </div>
    </div>
  );
}

function App() {
  const [remaining, setRemaining] = createSignal(10);
  const interval = setInterval(() => {
    setRemaining(current => {
      if (current <= 1) {
        clearInterval(interval);
        return 0;
      }
      return current - 1;
    });
  }, 1000);
  onCleanup(() => clearInterval(interval));

  return (
    <div
      style={{
        display: "flex",
        "flex-direction": "column",
        width: "720px",
        padding: "24px",
        gap: "16px",
        "font-family": '"Helvetica Neue", Arial, sans-serif',
        "background-color": "#f5f7fa",
        "border-color": "#cbd2dc",
        "border-width": "1px",
        "border-radius": "16px",
      }}
    >
      <div style={{ display: "flex", "flex-direction": "row", gap: "6px" }}>
        <text style={{ color: "#18202b", "font-size": "28px", "line-height": "34px", "font-weight": 700 }}>
          Burokku
        </text>
        <text style={{ color: "#526071", "font-size": "28px", "line-height": "34px" }}>Solid DOM</text>
      </div>
      <div style={{ display: "flex", gap: "16px" }}>
        <div
          style={{
            display: "flex",
            "flex-direction": "column",
            width: "0px",
            padding: "18px",
            "flex-grow": 1,
            "background-color": "#ffffff",
            "border-color": "#dce1e8",
            "border-width": "1px",
            "border-radius": "12px",
          }}
        >
          <text style={{ color: "#526071", "font-size": "16px", "line-height": "24px" }}>
            Countdown
          </text>
          <text style={{ color: "#18202b", "font-size": "52px", "line-height": "60px", "font-weight": 700 }}>
            {remaining()}
          </text>
        </div>
        <div
          style={{
            display: "flex",
            "flex-direction": "column",
            padding: "14px",
            gap: "10px",
            "background-color": "#ffffff",
            "border-color": "#dce1e8",
            "border-width": "1px",
            "border-radius": "12px",
          }}
        >
          <text style={{ color: "#18202b", "font-size": "16px", "line-height": "22px", "font-weight": 700 }}>
            Usable scroll container
          </text>
          <ScrollablePanel />
        </div>
      </div>
      <div style={{ display: "flex", gap: "16px" }}>
        <ExampleCard title="Flex · proportional growth">
          <FlexExample />
        </ExampleCard>
        <ExampleCard title="Grid · three columns">
          <GridExample />
        </ExampleCard>
      </div>
      <PositionExamples />
      <TypographyExamples remaining={remaining()} />
      <PaintExamples />
      <div
        style={{
          display: "flex",
          position: "fixed",
          right: "12px",
          bottom: "12px",
          width: "154px",
          padding: "10px",
          color: "#ffffff",
          "font-size": "12px",
          "line-height": "16px",
          "font-weight": 700,
          "background-color": "#0f172a",
          "border-radius": "8px",
          "box-shadow": "0px 8px 18px rgba(15, 23, 42, 0.35)",
        }}
      >
        <text>FIXED · viewport corner</text>
      </div>
    </div>
  );
}

document.body.style.padding = "12px";
document.body.style.borderWidth = "4px";
document.body.style.borderColor = "#94a3b8";
document.body.style.overflowY = "auto";
document.body.style.overflowX = "scroll";

render(() => <App />, document.body);
