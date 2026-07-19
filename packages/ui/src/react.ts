/// <reference path="./react-reconciler.d.ts" />
import { createContext, type ReactNode } from "react";
import createReconciler from "react-reconciler";
import type { ElementName, ElementProps, Style } from "./index";

interface HostRoot {
  id: 0;
  children: HostNode[];
}

interface HostNode {
  id: number;
  type: ElementName;
  props: ElementProps;
  text?: string;
  children: HostNode[];
  parent?: HostRoot | HostNode;
  nativeCreated: boolean;
}

type HostParent = HostRoot | HostNode;
type StyleName = keyof Style;

type NativeMutation =
  | { kind: "create"; id: number; type: ElementName }
  | { kind: "text"; id: number; text: string }
  | { kind: "style-number"; id: number; name: StyleName; value: number }
  | { kind: "style-string"; id: number; name: StyleName; value: string }
  | {
      kind: "style-color";
      id: number;
      name: StyleName;
      value: [number, number, number, number];
    }
  | { kind: "style-clear"; id: number; name: StyleName }
  | { kind: "insert"; parent: number; child: number; before: number }
  | { kind: "remove"; parent: number; child: number };

let nextId = 1;
let nextCommitId = 1;
let currentUpdatePriority = 32;
let pendingMutations: NativeMutation[] = [];

const elementNames = new Set<ElementName>(["div", "button", "span", "text"]);
const colorNames = new Set<StyleName>([
  "backgroundColor",
  "color",
  "borderColor",
  "outlineColor",
]);
const stringNames = new Set<StyleName>(["display", "flexDirection", "fontFamily"]);
const numberNames = new Set<StyleName>([
  "width",
  "height",
  "minWidth",
  "minHeight",
  "maxWidth",
  "maxHeight",
  "flexGrow",
  "flexShrink",
  "gap",
  "padding",
  "margin",
  "borderWidth",
  "borderRadius",
  "outlineWidth",
  "outlineOffset",
  "fontSize",
  "lineHeight",
  "fontWeight",
]);
const hostContext = {};

const now = (): number => globalThis.__burokku_now?.() ?? Date.now();

const required = <T>(name: string, callback: T | undefined): T => {
  if (callback === undefined) throw new Error(`Burokku host function ${name} is not installed`);
  return callback;
};

const assertElementName = (type: string): ElementName => {
  if (!elementNames.has(type as ElementName)) {
    throw new Error(`Unsupported Burokku element '${type}'`);
  }
  return type as ElementName;
};

const color = (value: string): [number, number, number, number] => {
  const hex = value.startsWith("#") ? value.slice(1) : value;
  if (hex.length === 3 || hex.length === 4) {
    const channels = [...hex].map((digit) => Number.parseInt(digit + digit, 16));
    if (channels.some(Number.isNaN)) throw new Error(`Invalid color '${value}'`);
    return [channels[0], channels[1], channels[2], channels[3] ?? 255];
  }
  if (hex.length === 6 || hex.length === 8) {
    const channels: [number, number, number, number] = [
      Number.parseInt(hex.slice(0, 2), 16),
      Number.parseInt(hex.slice(2, 4), 16),
      Number.parseInt(hex.slice(4, 6), 16),
      hex.length === 8 ? Number.parseInt(hex.slice(6, 8), 16) : 255,
    ];
    if (channels.some(Number.isNaN)) throw new Error(`Invalid color '${value}'`);
    return channels;
  }
  throw new Error(`Unsupported color '${value}'; use #rgb, #rgba, #rrggbb, or #rrggbbaa`);
};

const queueStyle = (id: number, name: StyleName, value: Style[StyleName]): void => {
  if (value === undefined) {
    pendingMutations.push({ kind: "style-clear", id, name });
  } else if (colorNames.has(name) && typeof value === "string") {
    pendingMutations.push({ kind: "style-color", id, name, value: color(value) });
  } else if (stringNames.has(name) && typeof value === "string") {
    pendingMutations.push({ kind: "style-string", id, name, value });
  } else if (numberNames.has(name) && typeof value === "number" && Number.isFinite(value)) {
    pendingMutations.push({ kind: "style-number", id, name, value });
  } else {
    throw new Error(`Invalid value for Burokku style '${name}': ${String(value)}`);
  }
};

const queueStyleChanges = (id: number, previous: Style | undefined, next: Style | undefined): void => {
  const names = new Set<StyleName>([
    ...(Object.keys(previous ?? {}) as StyleName[]),
    ...(Object.keys(next ?? {}) as StyleName[]),
  ]);
  for (const name of names) {
    const previousValue = previous?.[name];
    const nextValue = next?.[name];
    if (previousValue !== nextValue) queueStyle(id, name, nextValue);
  }
};

const textContent = (node: HostNode): string =>
  node.text ?? node.children.map((child) => textContent(child)).join("");

const isHostNode = (parent: HostParent): parent is HostNode => "type" in parent;

const syncTextContainer = (parent: HostParent | undefined): void => {
  if (
    parent &&
    isHostNode(parent) &&
    parent.type === "text" &&
    parent.text === undefined &&
    parent.nativeCreated
  ) {
    pendingMutations.push({ kind: "text", id: parent.id, text: textContent(parent) });
  }
};

const syncTextAncestors = (node: HostNode): void => {
  let parent = node.parent;
  while (parent && isHostNode(parent)) {
    syncTextContainer(parent);
    parent = parent.parent;
  }
};

const detachLocal = (child: HostNode): HostParent | undefined => {
  const parent = child.parent;
  if (!parent) return undefined;
  const index = parent.children.indexOf(child);
  if (index >= 0) parent.children.splice(index, 1);
  child.parent = undefined;
  return parent;
};

const appendLocal = (parent: HostParent, child: HostNode): HostParent | undefined => {
  const previousParent = detachLocal(child);
  parent.children.push(child);
  child.parent = parent;
  return previousParent;
};

const insertLocal = (
  parent: HostParent,
  child: HostNode,
  before: HostNode,
): HostParent | undefined => {
  if (child === before) return child.parent;
  const previousParent = detachLocal(child);
  const index = parent.children.indexOf(before);
  if (index < 0) throw new Error("React attempted to insert before a node with another parent");
  parent.children.splice(index, 0, child);
  child.parent = parent;
  return previousParent;
};

const materialize = (node: HostNode): void => {
  if (node.nativeCreated) return;
  node.nativeCreated = true;
  pendingMutations.push({ kind: "create", id: node.id, type: node.type });
  queueStyleChanges(node.id, undefined, node.props.style);
  if (node.text !== undefined) {
    pendingMutations.push({ kind: "text", id: node.id, text: node.text });
  }
  for (const child of node.children) {
    materialize(child);
    pendingMutations.push({ kind: "insert", parent: node.id, child: child.id, before: -1 });
  }
  syncTextContainer(node);
};

const appendNative = (parent: HostParent, child: HostNode): void => {
  const previousParent = appendLocal(parent, child);
  materialize(child);
  pendingMutations.push({ kind: "insert", parent: parent.id, child: child.id, before: -1 });
  syncTextContainer(previousParent);
  syncTextContainer(parent);
};

const insertNative = (parent: HostParent, child: HostNode, before: HostNode): void => {
  if (child === before) return;
  const previousParent = insertLocal(parent, child, before);
  materialize(child);
  pendingMutations.push({ kind: "insert", parent: parent.id, child: child.id, before: before.id });
  syncTextContainer(previousParent);
  syncTextContainer(parent);
};

const removeNative = (parent: HostParent, child: HostNode): void => {
  if (child.parent !== parent) throw new Error("React attempted to remove a node from another parent");
  detachLocal(child);
  if (child.nativeCreated) {
    pendingMutations.push({ kind: "remove", parent: parent.id, child: child.id });
  }
  syncTextContainer(parent);
};

const sendMutation = (mutation: NativeMutation): void => {
  switch (mutation.kind) {
    case "create":
      required("__burokku_create", globalThis.__burokku_create)(mutation.id, mutation.type);
      break;
    case "text":
      required("__burokku_set_text", globalThis.__burokku_set_text)(mutation.id, mutation.text);
      break;
    case "style-number":
      required("__burokku_set_style_number", globalThis.__burokku_set_style_number)(
        mutation.id,
        mutation.name,
        mutation.value,
      );
      break;
    case "style-string":
      required("__burokku_set_style_string", globalThis.__burokku_set_style_string)(
        mutation.id,
        mutation.name,
        mutation.value,
      );
      break;
    case "style-color":
      required("__burokku_set_style_color", globalThis.__burokku_set_style_color)(
        mutation.id,
        mutation.name,
        ...mutation.value,
      );
      break;
    case "style-clear":
      required("__burokku_clear_style", globalThis.__burokku_clear_style)(
        mutation.id,
        mutation.name,
      );
      break;
    case "insert":
      required("__burokku_insert", globalThis.__burokku_insert)(
        mutation.parent,
        mutation.child,
        mutation.before,
      );
      break;
    case "remove":
      required("__burokku_remove", globalThis.__burokku_remove)(
        mutation.parent,
        mutation.child,
      );
      break;
  }
};

const commit = (): void => {
  const commitId = nextCommitId++;
  const mutations = pendingMutations;
  pendingMutations = [];
  const startedAt = now();
  for (const mutation of mutations) sendMutation(mutation);
  required("__burokku_flush", globalThis.__burokku_flush)(commitId);
  console.log(
    `[Burokku perf] React commit #${commitId}: bridge ${(now() - startedAt).toFixed(3)} ms (${mutations.length} mutations)`,
  );
};

const hostConfig: Record<string, unknown> = {
  supportsMutation: true,
  supportsPersistence: false,
  supportsHydration: false,
  isPrimaryRenderer: true,
  noTimeout: -1,
  NotPendingTransition: null,
  HostTransitionContext: createContext(null),
  now,
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
  resetAfterCommit: commit,
  createInstance: (type: string, props: ElementProps): HostNode => ({
    id: nextId++,
    type: assertElementName(type),
    props,
    children: [],
    nativeCreated: false,
  }),
  createTextInstance: (text: string): HostNode => ({
    id: nextId++,
    type: "text",
    props: {},
    text,
    children: [],
    nativeCreated: false,
  }),
  appendInitialChild: appendLocal,
  appendChild: appendNative,
  appendChildToContainer: appendNative,
  insertBefore: insertNative,
  insertInContainerBefore: insertNative,
  removeChild: removeNative,
  removeChildFromContainer: removeNative,
  clearContainer: (root: HostRoot) => {
    for (const child of [...root.children]) removeNative(root, child);
  },
  finalizeInitialChildren: () => false,
  shouldSetTextContent: () => false,
  commitUpdate: (instance: HostNode, _type: ElementName, previous: ElementProps, next: ElementProps) => {
    if (instance.nativeCreated) queueStyleChanges(instance.id, previous.style, next.style);
    instance.props = next;
  },
  commitTextUpdate: (instance: HostNode, _old: string, next: string) => {
    instance.text = next;
    if (instance.nativeCreated) {
      pendingMutations.push({ kind: "text", id: instance.id, text: next });
      syncTextAncestors(instance);
    }
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
  const root: HostRoot = { id: 0, children: [] };
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
    const startedAt = now();
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
    console.log(
      `[Burokku perf] React root render: ${(now() - startedAt).toFixed(3)} ms (reconcile + commit)`,
    );
  };
  return {
    render: update,
    unmount: () => update(null),
  };
}
