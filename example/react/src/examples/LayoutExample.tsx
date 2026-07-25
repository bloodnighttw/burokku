import { FeatureCard, Label, palette } from "./shared";

const cell = {
  display: "flex" as const,
  alignItems: "center",
  justifyContent: "center",
  minHeight: 42,
  backgroundColor: "#e8efff",
  borderRadius: 8,
  color: palette.ink,
  fontSize: 12,
};

export function LayoutExample() {
  return (
    <FeatureCard title="Grid & flex layout">
      <Label>explicit/implicit tracks · named placement · auto-flow · flex shorthand · order</Label>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "110px minmax(150px, 1fr) 90px",
          gridAutoRows: "44px",
          gridAutoFlow: "row dense",
          gap: 8,
          width: 610,
        }}
      >
        <div style={{ ...cell, gridColumn: "1 / 3", backgroundColor: "#dce8ff" }}>span 2 columns</div>
        <div style={{ ...cell, gridColumn: "3" }}>track 3</div>
        <div style={{ ...cell }}>implicit A</div>
        <div style={{ ...cell }}>implicit B</div>
        <div style={{ ...cell }}>implicit C</div>
      </div>
      <div style={{ display: "flex", gap: 8, width: 610 }}>
        <div style={{ ...cell, flex: "2 1 0px", order: 2, backgroundColor: "#e5dcff" }}>
          flex: 2 · order 2
        </div>
        <div style={{ ...cell, flex: "1 1 0px", order: 1, backgroundColor: "#d9f5e8" }}>
          flex: 1 · order 1
        </div>
      </div>
    </FeatureCard>
  );
}
