import { FeatureCard, Label } from "./shared";

export function ControlsExample() {
  return (
    <FeatureCard title="Native controls">
      <Label>button states · select/option projection · disabled and multiple controls</Label>
      <div style={{ display: "flex", gap: 12, alignItems: "center" }}>
        <button>Native button</button>
        <button aria-pressed="true">Pressed</button>
        <button disabled>Disabled</button>
        <select>
          <option value="blue">Blue option</option>
          <option value="violet" selected>Violet option</option>
          <option value="disabled" disabled>Disabled option</option>
        </select>
        <select multiple>
          <option selected>Multiple A</option>
          <option selected>Multiple B</option>
        </select>
      </div>
    </FeatureCard>
  );
}
