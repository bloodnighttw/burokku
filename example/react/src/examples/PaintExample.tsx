import { FeatureCard, Label, palette } from "./shared";

const png =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAADklEQVR4nGP4z8AAQv8BD/kD/YURmXYAAAAASUVORK5CYII=";

export function PaintExample() {
  return (
    <FeatureCard title="Paint effects & colors">
      <Label>group opacity · affine transform · shadows · gradients/raster · functional/named colors</Label>
      <div style={{ display: "flex", gap: 16, alignItems: "center", padding: 12 }}>
        <div
          style={{
            display: "flex",
            width: 170,
            height: 92,
            alignItems: "center",
            justifyContent: "center",
            opacity: "78%",
            transform: "rotate(-4deg) skewX(4deg)",
            backgroundImage: "linear-gradient(120deg, #ef476f 0%, #ffd166 45%, #06d6a0 75%, #118ab2 100%)",
            borderRadius: 16,
            boxShadow: "0 8px 18px rgba(23, 32, 51, .22), inset 0 0 0 3px rgba(255,255,255,.45)",
          }}
        >
          <span
            style={{
              color: "white",
              fontSize: 15,
              fontWeight: 700,
              textShadow: "1px 2px 3px rgba(0,0,0,.45), -1px 0 1px rebeccapurple",
            }}
          >
            transformed group
          </span>
        </div>
        <div
          style={{
            width: 126,
            height: 92,
            backgroundColor: "hsl(215 65% 92%)",
            backgroundImage: `url(${png})`,
            borderRadius: 14,
            boxShadow: "0 5px 12px rgba(53, 104, 212, .22)",
          }}
        />
        <div
          style={{
            width: 190,
            height: 92,
            backgroundImage: "radial-gradient(white 0%, rgba(116, 74, 205, .55) 45%, rebeccapurple 100%)",
            borderRadius: 14,
          }}
        />
      </div>
      <span style={{ color: palette.muted, fontSize: 11 }}>
        PNG data URL · multi-stop linear/radial gradients · rgb/hsl/rgba/named colors
      </span>
    </FeatureCard>
  );
}
