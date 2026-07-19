import React from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { SnapshotNode } from "./index";
import { createRoot } from "./react";

let commits: SnapshotNode[] = [];

afterEach(() => {
  commits = [];
  globalThis.__burokku_commit = undefined;
});

describe("React renderer", () => {
  it("maps intrinsic elements and text into a Burokku snapshot", () => {
    globalThis.__burokku_commit = (json) => {
      commits.push((JSON.parse(json) as { root: SnapshotNode }).root);
    };
    const root = createRoot();
    root.render(
      <div style={{ display: "flex", backgroundColor: "#102030" }}>
        <span><text>Hello</text></span>
        <button><text>Go</text></button>
      </div>,
    );

    const snapshot = commits.at(-1);
    expect(snapshot?.children?.[0].type).toBe("div");
    expect(snapshot?.children?.[0].style.backgroundColor).toEqual([16, 32, 48, 255]);
    expect(snapshot?.children?.[0].children?.[0].type).toBe("span");
    expect(snapshot?.children?.[0].children?.[0].children?.[0].text).toBe("Hello");
    expect(snapshot?.children?.[0].children?.[1].type).toBe("button");
  });

  it("commits React updates and unmounts", () => {
    globalThis.__burokku_commit = (json) => {
      commits.push((JSON.parse(json) as { root: SnapshotNode }).root);
    };
    const root = createRoot();
    root.render(<text>Before</text>);
    root.render(<text>After</text>);

    expect(commits.at(-1)?.children?.[0].text).toBe("After");

    root.unmount();
    expect(commits.at(-1)?.children).toEqual([]);
  });
});
