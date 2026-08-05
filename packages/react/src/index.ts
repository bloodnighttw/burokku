/// <reference path="./react-reconciler.d.ts" />
import { createContext, type ReactNode } from "react";
import createReconciler from "react-reconciler";
import {
  commitHostRoot,
  createHostElement,
  createHostRoot,
  createHostText,
  insertHostNode,
  removeHostNode,
  setHostText,
  updateHostProperties,
  type ElementName,
  type HostElement,
  type HostNode,
  type HostParent,
  type HostProps,
  type HostRoot,
  type HostText,
} from "@burokku/runtime";

let currentUpdatePriority = 32;
const hostContext = {};

const append = (parent: HostParent, child: HostNode): void => {
  insertHostNode(parent, child);
};

const insert = (parent: HostParent, child: HostNode, before: HostNode): void => {
  insertHostNode(parent, child, before);
};

const remove = (parent: HostParent, child: HostNode): void => {
  removeHostNode(parent, child);
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
  scheduleMicrotask: (callback: () => void) => void Promise.resolve().then(callback),
  supportsMicrotasks: true,
  getRootHostContext: () => hostContext,
  getChildHostContext: () => hostContext,
  getPublicInstance: (instance: HostNode) => instance,
  prepareForCommit: () => null,
  resetAfterCommit: (container: HostRoot) => commitHostRoot(container),
  createInstance: (type: string, props: HostProps): HostElement => {
    if (!isElementName(type)) throw new Error(`Unsupported Burokku element <${type}>`);
    const element = createHostElement(type);
    updateHostProperties(element, {}, props);
    return element;
  },
  createTextInstance: (text: string): HostText => createHostText(text),
  appendInitialChild: append,
  appendChild: append,
  appendChildToContainer: append,
  insertBefore: insert,
  insertInContainerBefore: insert,
  removeChild: remove,
  removeChildFromContainer: remove,
  clearContainer: (container: HostRoot) => {
    for (const child of [...container.children]) removeHostNode(container, child);
  },
  finalizeInitialChildren: () => false,
  shouldSetTextContent: () => false,
  commitUpdate: (instance: HostElement, _type: string, previous: HostProps, next: HostProps) => {
    updateHostProperties(instance, previous, next);
  },
  commitTextUpdate: (instance: HostText, _previous: string, next: string) => {
    setHostText(instance, next);
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
  resolveEventTimeStamp: Date.now,
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
  const container = createHostRoot();
  let pendingError: unknown;
  const captureError = (error: unknown): void => {
    pendingError ??= error;
  };
  const root = reconciler.createContainer(
    container,
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
      reconciler.updateContainerSync(node, root, null);
      reconciler.flushSyncWork?.();
    } else {
      reconciler.updateContainer(node, root, null);
    }
    if (pendingError !== undefined) {
      const error = pendingError;
      pendingError = undefined;
      throw error;
    }
  };
  return { render: update, unmount: () => update(null) };
}

function isElementName(name: string): name is ElementName {
  return name === "window" || name === "div" || name === "flex" || name === "grid" || name === "text";
}

export type {
  BackgroundImage,
  BurokkuStyle,
  ContentAlignment,
  DivStyle,
  ElementName,
  FlexStyle,
  GradientStop,
  GridStyle,
  HostProps,
  ItemAlignment,
  SharedPaintStyle,
  TextStyle,
} from "@burokku/runtime";
