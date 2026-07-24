import { createSignal, onCleanup } from "solid-js";
import { render } from "@burokku/solid";

function OverflowBar(props: { mode: "hidden" | "clip" }) {
  const colors =
    props.mode === "hidden"
      ? ["#2850dc", "#4f72e6", "#7e99ef", "#a9baf5"]
      : ["#7c3aed", "#965cef", "#af7cf4", "#c7a5f8"];

  return (
    <div style={{ display: "flex", "flex-direction": "column", width: "184px", gap: "7px" }}>
      <span style={{ color: "#526071", "font-size": "13px", "line-height": "18px", "font-weight": 700 }}>
        overflow: {props.mode}
      </span>
      <div
        style={{
          display: "flex",
          width: "180px",
          height: "24px",
          overflow: props.mode,
          "background-color": "#dce3ee",
          "border-color": "#c5cfdd",
          "border-width": "2px",
          "border-radius": "12px",
        }}
      >
        <div style={{ display: "flex", width: "260px", height: "24px", "flex-shrink": 0 }}>
          {colors.map(color => (
            <div style={{ width: "65px", height: "24px", "flex-shrink": 0, "background-color": color }} />
          ))}
        </div>
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
        width: "420px",
        padding: "24px",
        gap: "16px",
        "background-color": "#f5f7fa",
        "border-color": "#cbd2dc",
        "border-width": "1px",
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
          "border-radius": "12px",
        }}
      >
        <span style={{ color: "#18202b", "font-size": "16px", "line-height": "22px", "font-weight": 700 }}>
          Descendant clipping
        </span>
        <div style={{ display: "flex", "flex-direction": "row", gap: "10px" }}>
          <OverflowBar mode="hidden" />
          <OverflowBar mode="clip" />
        </div>
      </div>
    </div>
  );
}

render(() => <App />, document.body);
