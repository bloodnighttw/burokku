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
            <span style={{ color: "#263246", "font-size": "14px", "line-height": "18px", "font-weight": 700 }}>
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
          <span style={{ color: "#263246", "font-size": "14px", "line-height": "18px", "font-weight": 700 }}>
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
          <span style={{ color: "#263246", "font-size": "14px", "line-height": "18px", "font-weight": 700 }}>
            {item.label}
          </span>
        </div>
      ))}
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
      <span style={{ color: "#18202b", "font-size": "16px", "line-height": "22px", "font-weight": 700 }}>
        {props.title}
      </span>
      {props.children}
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
            Usable scroll container
          </span>
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
    </div>
  );
}

document.body.style.overflowY = "auto";
document.body.style.overflowX = "auto";

render(() => <App />, document.body);
