import { FeatureCard, Label, palette } from "./shared";

export function BorderPositionExample() {
  return (
    <FeatureCard title="Borders, radii & positioning">
      <Label>per-side width/color/style · elliptical radii · static/relative/absolute/fixed</Label>
      <div style={{ display: "flex", gap: 28, alignItems: "center" }}>
        <div
          style={{
            width: 190,
            height: 86,
            padding: 12,
            backgroundColor: "#f8fbff",
            borderTopWidth: 3,
            borderRightWidth: 7,
            borderBottomWidth: 5,
            borderLeftWidth: 10,
            borderTopColor: "#ef476f",
            borderRightColor: "#118ab2",
            borderBottomColor: "#06d6a0",
            borderLeftColor: "#8338ec",
            borderTopStyle: "dashed",
            borderRightStyle: "double",
            borderBottomStyle: "dotted",
            borderLeftStyle: "solid",
            borderRadius: "28px 10px / 12px 30px",
          }}
        >
          <span style={{ color: palette.ink, fontSize: 13, lineHeight: "18px" }}>
            Four independent border sides
          </span>
        </div>
        <div
          style={{
            position: "relative",
            width: 250,
            height: 104,
            backgroundColor: "#edf2ff",
            borderRadius: 12,
          }}
        >
          <span style={{ position: "static", color: palette.muted, fontSize: 12 }}>
            static flow
          </span>
          <span
            style={{
              position: "relative",
              left: 22,
              top: 12,
              color: "#3158aa",
              fontSize: 14,
              fontWeight: 700,
            }}
          >
            relative offset
          </span>
          <span
            style={{
              position: "absolute",
              right: 10,
              bottom: 8,
              padding: 6,
              backgroundColor: "#3158aa",
              color: "white",
              borderRadius: 7,
              fontSize: 11,
            }}
          >
            absolute
          </span>
        </div>
      </div>
      <span
        style={{
          position: "fixed",
          top: 10,
          right: 12,
          padding: "5px 9px",
          backgroundColor: "#172033",
          color: "white",
          borderRadius: 8,
          fontSize: 11,
          zIndex: 20,
        }}
      >
        fixed to viewport
      </span>
    </FeatureCard>
  );
}
