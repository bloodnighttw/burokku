import React from "react";
import { afterEach, describe, expect, it } from "vitest";
import { createRoot } from "./react";

const calls: Array<[string, ...unknown[]]> = [];

const installBridge = (): void => {
  globalThis.__burokku_create = (...args) => calls.push(["create", ...args]);
  globalThis.__burokku_set_text = (...args) => calls.push(["text", ...args]);
  globalThis.__burokku_set_style_number = (...args) => calls.push(["number", ...args]);
  globalThis.__burokku_set_style_string = (...args) => calls.push(["string", ...args]);
  globalThis.__burokku_set_style_color = (...args) => calls.push(["color", ...args]);
  globalThis.__burokku_clear_style = (...args) => calls.push(["clear", ...args]);
  globalThis.__burokku_insert = (...args) => calls.push(["insert", ...args]);
  globalThis.__burokku_remove = (...args) => calls.push(["remove", ...args]);
  globalThis.__burokku_flush = (...args) => calls.push(["flush", ...args]);
};

afterEach(() => {
  calls.length = 0;
  globalThis.__burokku_create = undefined;
  globalThis.__burokku_set_text = undefined;
  globalThis.__burokku_set_style_number = undefined;
  globalThis.__burokku_set_style_string = undefined;
  globalThis.__burokku_set_style_color = undefined;
  globalThis.__burokku_clear_style = undefined;
  globalThis.__burokku_insert = undefined;
  globalThis.__burokku_remove = undefined;
  globalThis.__burokku_flush = undefined;
});

describe("React renderer", () => {
  it("commits a typed native mutation batch without a JSON snapshot", () => {
    installBridge();
    const root = createRoot();
    root.render(
      <div style={{ display: "flex", width: 300, backgroundColor: "#102030" }}>
        <span><text>Hello</text></span>
        <button><text>Go</text></button>
      </div>,
    );

    expect(calls.filter(([kind]) => kind === "flush")).toHaveLength(1);
    expect(calls.some(([kind, _id, type]) => kind === "create" && type === "div")).toBe(true);
    expect(calls.some(([kind, _id, name, value]) =>
      kind === "string" && name === "display" && value === "flex"
    )).toBe(true);
    expect(calls.some(([kind, _id, name, value]) =>
      kind === "number" && name === "width" && value === 300
    )).toBe(true);
    expect(calls.some(([kind, _id, name, red, green, blue, alpha]) =>
      kind === "color" &&
      name === "backgroundColor" &&
      red === 16 &&
      green === 32 &&
      blue === 48 &&
      alpha === 255
    )).toBe(true);
  });

  it("sends only changed text and style fields on later commits", () => {
    installBridge();
    const root = createRoot();
    root.render(<text style={{ width: 100, height: 20 }}>Before</text>);
    calls.length = 0;

    root.render(<text style={{ width: 120 }}>After</text>);

    expect(calls.filter(([kind]) => kind === "create")).toHaveLength(0);
    expect(calls.some(([kind, _id, text]) => kind === "text" && text === "After")).toBe(true);
    expect(calls.some(([kind, _id, name, value]) =>
      kind === "number" && name === "width" && value === 120
    )).toBe(true);
    expect(calls.some(([kind, _id, name]) => kind === "clear" && name === "height")).toBe(true);
    expect(calls.filter(([kind]) => kind === "flush")).toHaveLength(1);

    calls.length = 0;
    root.unmount();
    expect(calls.some(([kind]) => kind === "remove")).toBe(true);
    expect(calls.filter(([kind]) => kind === "flush")).toHaveLength(1);
  });
});
