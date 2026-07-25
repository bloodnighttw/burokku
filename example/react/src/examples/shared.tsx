import type { ReactNode } from "react";

export const palette = {
  ink: "#172033",
  muted: "#61708a",
  line: "#d7deea",
  panel: "#ffffff",
  canvas: "#eef2f8",
  blue: "#3568d4",
};

export function FeatureCard({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 12,
        padding: 16,
        backgroundColor: palette.panel,
        borderColor: palette.line,
        borderStyle: "solid",
        borderWidth: 1,
        borderRadius: 14,
      }}
    >
      <span style={{ color: palette.ink, fontSize: 18, lineHeight: "24px", fontWeight: 700 }}>
        {title}
      </span>
      {children}
    </div>
  );
}

export function Label({ children }: { children: ReactNode }) {
  return (
    <span style={{ color: palette.muted, fontSize: 12, lineHeight: "16px", fontWeight: 700 }}>
      {children}
    </span>
  );
}
