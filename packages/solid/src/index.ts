import { createRenderer } from "solid-js/universal";
import type { JSX } from "solid-js";
import { setProperty as setHostProperty } from "@burokku/runtime";

const renderer = createRenderer<Node>({
  createElement(name) {
    return document.createElement(name);
  },
  createTextNode(value) {
    return document.createTextNode(value);
  },
  replaceText(textNode, value) {
    (textNode as Text).data = value;
  },
  setProperty(node, name, value, previous) {
    setHostProperty(node as HTMLElement, name, value, previous);
  },
  insertNode(parent, node, anchor) {
    parent.insertBefore(node, anchor ?? null);
  },
  isTextNode(node) {
    return node.nodeType === Node.TEXT_NODE;
  },
  removeNode(parent, node) {
    parent.removeChild(node);
  },
  getParentNode(node) {
    return node.parentNode ?? undefined;
  },
  getFirstChild(node) {
    return node.firstChild ?? undefined;
  },
  getNextSibling(node) {
    return node.nextSibling ?? undefined;
  },
});

export const render = renderer.render as unknown as (
  code: () => JSX.Element,
  element: Node,
) => () => void;

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

export type { BurokkuStyle, HostProps } from "@burokku/runtime";
