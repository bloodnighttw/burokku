/// <reference path="./react-reconciler.d.ts" />
import { createContext, type ReactNode } from "react";
import createReconciler from "react-reconciler";
import { updateProperties, type HostProps } from "@burokku/runtime";

type HostNode = HTMLElement | Text;

let currentUpdatePriority = 32;
const hostContext = {};

const append = (parent: HTMLElement, child: HostNode): void => {
  parent.appendChild(child);
};

const insert = (parent: HTMLElement, child: HostNode, before: HostNode): void => {
  parent.insertBefore(child, before);
};

const remove = (parent: HTMLElement, child: HostNode): void => {
  parent.removeChild(child);
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
  resetAfterCommit: () => undefined,
  createInstance: (type: string, props: HostProps): HTMLElement => {
    const element = document.createElement(type);
    updateProperties(element, {}, props);
    return element;
  },
  createTextInstance: (text: string): Text => document.createTextNode(text),
  appendInitialChild: append,
  appendChild: append,
  appendChildToContainer: append,
  insertBefore: insert,
  insertInContainerBefore: insert,
  removeChild: remove,
  removeChildFromContainer: remove,
  clearContainer: (container: HTMLElement) => container.replaceChildren(),
  finalizeInitialChildren: () => false,
  shouldSetTextContent: () => false,
  commitUpdate: (instance: HTMLElement, _type: string, previous: HostProps, next: HostProps) => {
    updateProperties(instance, previous, next);
  },
  commitTextUpdate: (instance: Text, _previous: string, next: string) => {
    instance.data = next;
  },
  hideInstance: (instance: HTMLElement) => {
    instance.style.display = "none";
  },
  unhideInstance: (instance: HTMLElement, props: HostProps) => {
    instance.style.display = props.style?.display ?? "";
  },
  hideTextInstance: (instance: Text) => {
    instance.data = "";
  },
  unhideTextInstance: (instance: Text, text: string) => {
    instance.data = text;
  },
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

export function createRoot(container: HTMLElement = document.body): Root {
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

export type { BurokkuStyle, HostProps } from "@burokku/runtime";
