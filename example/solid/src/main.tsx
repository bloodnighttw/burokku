import { createSignal, onCleanup } from "solid-js";
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
        "border-style": "solid",
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
            <span style={{ color: "#263246", "font-size": "14px", "line-height": "18px", "font-weight": 700 }}>
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
        "flex-direction": "column",
        padding: "14px",
        gap: "10px",
        "background-color": "#ffffff",
        "border-color": "#dce1e8",
        "border-width": "1px",
        "border-style": "solid",
        "border-radius": "12px",
      }}
    >
      <span style={{ color: "#18202b", "font-size": "16px", "line-height": "22px", "font-weight": 700 }}>
        Borders & positioning
      </span>
      <span style={{ color: "#526071", "font-size": "12px", "line-height": "18px" }}>
        Per-side borders, elliptical radii, and all four position modes
      </span>
      <div
        style={{
          height: "72px",
          padding: "12px",
          "background-color": "#f8fbff",
          "border-top-width": "3px",
          "border-right-width": "7px",
          "border-bottom-width": "5px",
          "border-left-width": "10px",
          "border-top-color": "#ef476f",
          "border-right-color": "#118ab2",
          "border-bottom-color": "#06d6a0",
          "border-left-color": "#8338ec",
          "border-top-style": "dashed",
          "border-right-style": "double",
          "border-bottom-style": "dotted",
          "border-left-style": "solid",
          "border-radius": "28px 10px / 12px 30px",
        }}
      >
        <span style={{ color: "#263246", "font-size": "13px", "line-height": "18px", "font-weight": 700 }}>
          Four independent border sides
        </span>
      </div>
      <div
        style={{
          position: "relative",
          height: "104px",
          padding: "12px",
          "background-color": "#edf2ff",
          "border-radius": "12px",
        }}
      >
        <span style={{ position: "static", color: "#526071", "font-size": "12px", "line-height": "18px" }}>
          static flow
        </span>
        <span
          style={{
            position: "relative",
            left: "22px",
            top: "12px",
            color: "#3158aa",
            "font-size": "14px",
            "line-height": "18px",
            "font-weight": 700,
          }}
        >
          relative offset
        </span>
        <span
          style={{
            position: "absolute",
            right: "10px",
            bottom: "8px",
            padding: "6px",
            "background-color": "#3158aa",
            color: "#ffffff",
            "border-radius": "7px",
            "font-size": "11px",
            "line-height": "16px",
          }}
        >
          absolute
        </span>
      </div>
      <span
        style={{
          position: "fixed",
          top: "10px",
          right: "12px",
          padding: "5px 9px",
          "background-color": "#172033",
          color: "#ffffff",
          "border-radius": "8px",
          "font-size": "11px",
          "line-height": "16px",
        }}
      >
        fixed to viewport
      </span>
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
        width: "420px",
        padding: "24px",
        gap: "16px",
        "background-color": "#f5f7fa",
        "border-color": "#cbd2dc",
        "border-width": "1px",
        "border-style": "solid",
        "border-radius": "16px",
      }}
    >
      <span style={{ display: "flex", "flex-direction": "row", gap: "6px" }}>
        <span style={{ color: "#18202b", "font-size": "28px", "line-height": "34px", "font-weight": 700 }}>
          Burokku
        </span>
        <span style={{ color: "#526071", "font-size": "28px", "line-height": "34px" }}>Solid DOM</span>
      </span>
      <div
        style={{
          display: "flex",
          "flex-direction": "column",
          padding: "18px",
          "background-color": "#ffffff",
          "border-color": "#dce1e8",
          "border-width": "1px",
          "border-style": "solid",
          "border-radius": "12px",
        }}
      >
        <span style={{ color: "#526071", "font-size": "16px", "line-height": "24px" }}>
          Countdown
        </span>
        <span style={{ color: "#18202b", "font-size": "52px", "line-height": "60px", "font-weight": 700 }}>
          {remaining()}
        </span>
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
          "border-style": "solid",
          "border-radius": "12px",
        }}
      >
        <span style={{ color: "#18202b", "font-size": "16px", "line-height": "22px", "font-weight": 700 }}>
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

render(() => <App />, document.body);
