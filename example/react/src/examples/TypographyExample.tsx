import { FeatureCard, Label, palette } from "./shared";

export function TypographyExample() {
  return (
    <FeatureCard title="Typography & text metrics">
      <Label>baseline · normal line-height · alignment · style/spacing/decoration · wrapping · fallbacks</Label>
      <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
        <span style={{ color: palette.ink, fontSize: 30, lineHeight: "normal", fontWeight: 700 }}>
          Baseline
        </span>
        <span style={{ color: palette.muted, fontSize: 14, lineHeight: "normal" }}>
          aligned through Glyphon metrics
        </span>
      </div>
      <span
        style={{
          width: 610,
          color: "#243b6b",
          fontSize: 16,
          lineHeight: "24px",
          fontFamily: "\"Definitely Missing\", serif, sans-serif",
          fontStyle: "italic",
          textAlign: "center",
          letterSpacing: "1px",
          wordSpacing: "5px",
          textDecoration: "underline line-through",
          textShadow: "1px 1px 2px rgba(53, 104, 212, .25)",
        }}
      >
        Styled, centered, spaced and decorated text with font fallbacks
      </span>
      <span
        style={{
          width: 610,
          height: 54,
          overflow: "hidden",
          padding: 8,
          backgroundColor: "#f2f5fb",
          borderRadius: 8,
          color: palette.ink,
          fontSize: 12,
          lineHeight: "18px",
          whiteSpace: "pre-wrap",
          overflowWrap: "anywhere",
          wordBreak: "normal",
        }}
      >
        {"pre-wrap preserves this line\nwhile anywhere can break: supercalifragilisticexpialidocious"}
      </span>
    </FeatureCard>
  );
}
