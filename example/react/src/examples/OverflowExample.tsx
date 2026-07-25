import { FeatureCard, Label, palette } from "./shared";

export function OverflowExample() {
  const colors = ["#e6efff", "#eee8ff", "#dcf7ea", "#fff0d8", "#ffe4eb"];

  return (
    <FeatureCard title="Overflow, clipping & stacking">
      <Label>overflow: auto · rounded clip · z-index/isolation</Label>
      <div
        style={{
          display: "flex",
          width: 610,
          height: 116,
          overflow: "auto",
          backgroundColor: palette.canvas,
          borderColor: "#aab7cb",
          borderStyle: "solid",
          borderWidth: 2,
          borderRadius: "18px 8px / 10px 22px",
        }}
      >
        <div style={{ display: "flex", width: 760, gap: 10, padding: 12, flexShrink: 0 }}>
          {colors.map((color, index) => (
            <div
              key={color}
              style={{
                display: "flex",
                width: 138,
                height: 74,
                flexShrink: 0,
                alignItems: "center",
                justifyContent: "center",
                backgroundColor: color,
                borderRadius: 10,
                position: "relative",
                zIndex: index === 2 ? 2 : "auto",
                isolation: index === 2 ? "isolate" : "auto",
              }}
            >
              <span style={{ color: palette.ink, fontSize: 13, lineHeight: "18px" }}>
                Scroll card {index + 1}
              </span>
            </div>
          ))}
        </div>
      </div>
    </FeatureCard>
  );
}
