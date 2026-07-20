import { createSignal, onCleanup } from "solid-js";
import { render } from "@burokku/solid";

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
      <span style={{ color: "#18202b", "font-size": "28px", "line-height": "34px", "font-weight": 700 }}>
        Burokku Solid
      </span>
      <div
        style={{
          display: "flex",
          "flex-direction": "column",
          padding: "24px",
          "background-color": "#ffffff",
          "border-color": "#dce1e8",
          "border-width": "1px",
          "border-radius": "12px",
        }}
      >
        <span style={{ color: "#526071", "font-size": "16px", "line-height": "24px" }}>
          Countdown
        </span>
        <span style={{ color: "#18202b", "font-size": "72px", "line-height": "82px", "font-weight": 700 }}>
          {remaining()}
        </span>
      </div>
    </div>
  );
}

render(() => <App />, document.body);
