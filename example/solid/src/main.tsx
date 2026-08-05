import { createSignal, onCleanup } from "solid-js";
import { render } from "@burokku/solid";

function Demo() {
  const [remaining, setRemaining] = createSignal(5);
  const timer = setInterval(() => {
    setRemaining(current => {
      if (current <= 1) {
        clearInterval(timer);
        return 0;
      }
      return current - 1;
    });
  }, 1000);
  onCleanup(() => clearInterval(timer));

  const cards = [
    { label: "semantic elements", color: "#dbeafe" },
    { label: "typed styles", color: "#ede9fe" },
    { label: "reactive text", color: "#d1fae5" },
  ] as const;

  return (
    <window>
      <flex
        style={{
          flexDirection: "column",
          gap: 16,
          backgroundColor: "#f5f7fa",
          borderColor: "#cbd2dc",
          borderWidth: 1,
          borderRadius: 16,
        }}
      >
        <text
          style={{
            color: "#18202b",
            fontFamily: '"Helvetica Neue", Arial, sans-serif',
            fontSize: 28,
            lineHeight: 34,
            fontWeight: 700,
          }}
        >
          Burokku · Solid
        </text>

        <text style={{ color: "#526071", fontSize: 16, lineHeight: 24 }}>
          A semantic host tree rendered without browser DOM nodes.
        </text>

        <grid
          style={{
            gridTemplateColumns: "1fr 1fr 1fr",
            gap: 8,
            backgroundColor: "#ffffff",
            borderColor: "#dce1e8",
            borderWidth: 1,
            borderRadius: 12,
          }}
        >
          {cards.map(card => (
            <div
              style={{
                backgroundColor: card.color,
                borderColor: "#cbd5e1",
                borderWidth: 1,
                borderRadius: 8,
              }}
            >
              <text style={{ color: "#263246", fontSize: 14, fontWeight: 700 }}>
                {card.label}
              </text>
            </div>
          ))}
        </grid>

        <text
          style={{
            color: remaining() === 0 ? "#059669" : "#7c3aed",
            fontSize: 18,
            lineHeight: 24,
            fontWeight: 700,
          }}
        >
          Reactive countdown: {remaining()}
        </text>
      </flex>
    </window>
  );
}

render(() => <Demo />);
