import { createRenderer } from "solid-js/universal";
import type { JSX as SolidJSX } from "solid-js";
import {
  commitHostRoot,
  createHostElement,
  createHostRoot,
  createHostText,
  getHostFirstChild,
  getHostNextSibling,
  getHostParentNode,
  insertHostNode,
  removeHostNode,
  setHostProperty,
  setHostText,
  type ElementName,
  type HostNode,
  type HostParent,
} from "@burokku/runtime";

type SolidHostNode = HostNode | HostParent;
const elementNames = new Set<ElementName>(["window", "div", "flex", "grid", "text"]);

function isElementName(name: string): name is ElementName {
  return elementNames.has(name as ElementName);
}

const renderer = createRenderer<SolidHostNode>({
  createElement(name) {
    if (!isElementName(name)) throw new TypeError(`unsupported host element '${name}'`);
    return createHostElement(name);
  },
  createTextNode(value) {
    return createHostText(value);
  },
  replaceText(textNode, value) {
    if (textNode.type !== "string") throw new TypeError("expected a host text node");
    setHostText(textNode, value);
  },
  setProperty(node, name, value, previous) {
    if (node.type === "app" || node.type === "string") {
      throw new TypeError("expected a host element");
    }
    setHostProperty(node, name, value, previous);
  },
  insertNode(parent, node, anchor) {
    if (parent.type === "string" || node.type === "app" || anchor?.type === "app") {
      throw new TypeError("invalid host tree insertion");
    }
    insertHostNode(parent, node, anchor);
  },
  isTextNode(node) {
    return node.type === "string";
  },
  removeNode(parent, node) {
    if (parent.type === "string" || node.type === "app") {
      throw new TypeError("invalid host tree removal");
    }
    removeHostNode(parent, node);
  },
  getParentNode(node) {
    return node.type === "app" ? undefined : getHostParentNode(node);
  },
  getFirstChild(node) {
    return node.type === "string" ? undefined : getHostFirstChild(node);
  },
  getNextSibling(node) {
    return node.type === "app" ? undefined : getHostNextSibling(node);
  },
});

export function render(code: () => SolidJSX.Element): () => void {
  const root = createHostRoot(true);
  const dispose = renderer.render(code as unknown as () => SolidHostNode, root);
  commitHostRoot(root);

  return () => {
    dispose();
    let child = getHostFirstChild(root);
    while (child) {
      removeHostNode(root, child);
      child = getHostFirstChild(root);
    }
    commitHostRoot(root);
  };
}

export const {
  effect,
  memo,
  createComponent,
  createElement,
  createTextNode,
  insertNode,
  insert,
  spread,
  setProp,
  mergeProps,
} = renderer;

export type {
  BurokkuStyle,
  DivStyle,
  ElementName,
  FlexStyle,
  GridStyle,
  HostNode,
  HostParent,
  HostProps,
  TextStyle,
} from "@burokku/runtime";
export type { JSX } from "./jsx-runtime";
