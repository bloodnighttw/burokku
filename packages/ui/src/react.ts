/// <reference path="./react-reconciler.d.ts" />
import { createContext, type ReactNode } from "react";
import createReconciler from "react-reconciler";
import type { ElementName, ElementProps, SnapshotNode, Style } from "./index";

interface HostNode {
  id: number;
  type: ElementName;
  props: ElementProps;
  text?: string;
  children: HostNode[];
}

interface HostRoot {
  children: HostNode[];
}

let nextId = 1;
let currentUpdatePriority = 32;
const elementNames = new Set<ElementName>(["div", "button", "span", "text"]);
const hostContext = {};

const assertElementName = (type: string): ElementName => {
  if (!elementNames.has(type as ElementName)) {
    throw new Error(`Unsupported Burokku element '${type}'`);
  }
  return type as ElementName;
};

const append = (parent: HostRoot | HostNode, child: HostNode): void => {
  const existing = parent.children.indexOf(child);
  if (existing >= 0) parent.children.splice(existing, 1);
  parent.children.push(child);
};

const insertBefore = (parent: HostRoot | HostNode, child: HostNode, before: HostNode): void => {
  const existing = parent.children.indexOf(child);
  if (existing >= 0) parent.children.splice(existing, 1);
  const index = parent.children.indexOf(before);
  parent.children.splice(index < 0 ? parent.children.length : index, 0, child);
};

const remove = (parent: HostRoot | HostNode, child: HostNode): void => {
  const index = parent.children.indexOf(child);
  if (index >= 0) parent.children.splice(index, 1);
};

const color = (value: string): [number, number, number, number] => {
  const hex = value.startsWith("#") ? value.slice(1) : value;
  if (hex.length === 3 || hex.length === 4) {
    const channels = [...hex].map((digit) => Number.parseInt(digit + digit, 16));
    return [channels[0], channels[1], channels[2], channels[3] ?? 255];
  }
  if (hex.length === 6 || hex.length === 8) {
    return [
      Number.parseInt(hex.slice(0, 2), 16),
      Number.parseInt(hex.slice(2, 4), 16),
      Number.parseInt(hex.slice(4, 6), 16),
      hex.length === 8 ? Number.parseInt(hex.slice(6, 8), 16) : 255,
    ];
  }
  throw new Error(`Unsupported color '${value}'; use #rgb, #rgba, #rrggbb, or #rrggbbaa`);
};

const snapshotStyle = (style: Style | undefined): Record<string, unknown> => {
  if (!style) return {};
  const result: Record<string, unknown> = { ...style };
  for (const key of ["backgroundColor", "color", "borderColor", "outlineColor"] as const) {
    const value = style[key];
    if (value) result[key] = color(value);
  }
  return result;
};

const textContent = (node: HostNode): string =>
  node.text ?? node.children.map((child) => textContent(child)).join("");

const snapshotNode = (node: HostNode): SnapshotNode => {
  if (node.type === "text") {
    return {
      id: node.id,
      type: "text",
      style: snapshotStyle(node.props.style),
      text: textContent(node),
    };
  }
  return {
    id: node.id,
    type: node.type,
    style: snapshotStyle(node.props.style),
    children: node.children.map(snapshotNode),
  };
};

const commit = (root: HostRoot): void => {
  const syntheticRoot: SnapshotNode = {
    id: 0,
    type: "div",
    style: { display: "flex", flexDirection: "column" },
    children: root.children.map(snapshotNode),
  };
  globalThis.__burokku_commit?.(JSON.stringify({ root: syntheticRoot }));
};

const hostConfig: Record<string, unknown> = {
  supportsMutation: true,
  supportsPersistence: false,
  supportsHydration: false,
  isPrimaryRenderer: true,
  noTimeout: -1,
  NotPendingTransition: null,
  HostTransitionContext: createContext(null),
  now: Date.now,
  scheduleTimeout: setTimeout,
  cancelTimeout: clearTimeout,
  scheduleMicrotask: (callback: () => void) => {
    void Promise.resolve().then(callback);
  },
  supportsMicrotasks: true,
  getRootHostContext: () => hostContext,
  getChildHostContext: () => hostContext,
  getPublicInstance: (instance: HostNode) => instance,
  prepareForCommit: () => null,
  resetAfterCommit: (root: HostRoot) => commit(root),
  createInstance: (type: string, props: ElementProps): HostNode => ({
    id: nextId++,
    type: assertElementName(type),
    props,
    children: [],
  }),
  createTextInstance: (text: string): HostNode => ({
    id: nextId++,
    type: "text",
    props: {},
    text,
    children: [],
  }),
  appendInitialChild: append,
  appendChild: append,
  appendChildToContainer: append,
  insertBefore,
  insertInContainerBefore: insertBefore,
  removeChild: remove,
  removeChildFromContainer: remove,
  clearContainer: (root: HostRoot) => {
    root.children.length = 0;
  },
  finalizeInitialChildren: () => false,
  shouldSetTextContent: () => false,
  commitUpdate: (instance: HostNode, _type: ElementName, _old: ElementProps, next: ElementProps) => {
    instance.props = next;
  },
  commitTextUpdate: (instance: HostNode, _old: string, next: string) => {
    instance.text = next;
  },
  hideInstance: () => undefined,
  unhideInstance: () => undefined,
  hideTextInstance: () => undefined,
  unhideTextInstance: () => undefined,
  getCurrentEventPriority: () => 32,
  getCurrentUpdatePriority: () => currentUpdatePriority,
  setCurrentUpdatePriority: (priority: number) => {
    currentUpdatePriority = priority;
  },
  resolveUpdatePriority: () => currentUpdatePriority || 32,
  resolveEventTimeStamp: () => Date.now(),
  resolveEventType: () => null,
  trackSchedulerEvent: () => undefined,
  shouldAttemptEagerTransition: () => false,
  maySuspendCommit: () => false,
  maySuspendCommitOnUpdate: () => false,
  maySuspendCommitInSyncRender: () => false,
  preloadInstance: () => true,
  startSuspendingCommit: () => undefined,
  suspendInstance: () => undefined,
  waitForCommitToBeReady: () => null,
  getSuspendedCommitReason: () => null,
  requestPostPaintCallback: (callback: (time: number) => void) => callback(Date.now()),
  resetFormInstance: () => undefined,
  resetTextContent: () => undefined,
  commitMount: () => undefined,
  detachDeletedInstance: () => undefined,
  getInstanceFromNode: () => null,
  preparePortalMount: () => undefined,
  bindToConsole: (_method: string, args: unknown[]) => console.log(...args),
};

const reconciler = createReconciler(hostConfig);

export interface Root {
  render(node: ReactNode): void;
  unmount(): void;
}

export function createRoot(): Root {
  const root: HostRoot = { children: [] };
  let pendingError: unknown;
  const captureError = (error: unknown): void => {
    pendingError ??= error;
  };
  const container = reconciler.createContainer(
    root,
    1,
    null,
    false,
    null,
    "burokku",
    captureError,
    captureError,
    captureError,
    null,
  );
  const update = (node: ReactNode): void => {
    if (reconciler.updateContainerSync) {
      reconciler.updateContainerSync(node, container, null);
      reconciler.flushSyncWork?.();
    } else {
      reconciler.updateContainer(node, container, null);
    }
    if (pendingError !== undefined) {
      const error = pendingError;
      pendingError = undefined;
      throw error;
    }
  };
  return {
    render: update,
    unmount: () => update(null),
  };
}
