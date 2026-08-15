(() => {
  "use strict";

  const native = globalThis.__burokkuDomNative;
  const construct = Symbol("Burokku DOM node");
  const cache = new Map();

  function cssName(name) {
    name = String(name);
    if (name.startsWith("--") || name.includes("-")) return name.toLowerCase();
    return name.replace(/[A-Z]/g, letter => `-${letter.toLowerCase()}`).toLowerCase();
  }

  function requireNode(value, label = "node") {
    if (!(value instanceof Node)) throw new TypeError(`${label} must be a Node`);
    return value;
  }

  class CSSStyleDeclaration {
    constructor(node) {
      this._node = node;
    }

    getPropertyValue(name) {
      return native.getStyle(this._node._handle, cssName(name)) ?? "";
    }

    setProperty(name, value) {
      native.setStyle(this._node._handle, cssName(name), String(value));
    }

    removeProperty(name) {
      const normalized = cssName(name);
      const previous = native.getStyle(this._node._handle, normalized) ?? "";
      native.removeStyle(this._node._handle, normalized);
      return previous;
    }
  }

  function styleFor(node) {
    const declaration = new CSSStyleDeclaration(node);
    return new Proxy(declaration, {
      get(target, property, receiver) {
        if (typeof property !== "string" || property in target) {
          const value = Reflect.get(target, property, receiver);
          return typeof value === "function" ? value.bind(target) : value;
        }
        return target.getPropertyValue(property);
      },
      set(target, property, value, receiver) {
        if (typeof property !== "string" || property in target) {
          return Reflect.set(target, property, value, receiver);
        }
        if (value === undefined || value === null) target.removeProperty(property);
        else target.setProperty(property, value);
        return true;
      }
    });
  }

  class Node {
    constructor(token, handle) {
      if (token !== construct) throw new TypeError("DOM nodes cannot be constructed directly");
      Object.defineProperty(this, "_handle", { value: handle });
      this._listeners = new Map();
    }

    get nodeType() { return native.nodeType(this._handle); }
    get nodeName() { return native.nodeName(this._handle); }
    get parentNode() { return wrap(native.parent(this._handle)); }
    get firstChild() { return wrap(native.firstChild(this._handle)); }
    get nextSibling() { return wrap(native.nextSibling(this._handle)); }
    get lastChild() {
      const children = native.children(this._handle);
      return children.length === 0 ? null : wrap(children[children.length - 1]);
    }
    get childNodes() { return native.children(this._handle).map(wrap); }
    get children() { return this.childNodes.filter(node => node.nodeType === Node.ELEMENT_NODE); }
    get firstElementChild() { return this.children[0] ?? null; }
    get isConnected() {
      let current = this;
      while (current.parentNode) current = current.parentNode;
      return current === document.documentElement;
    }
    get textContent() { return native.textContent(this._handle); }
    set textContent(value) { native.setTextContent(this._handle, value == null ? "" : String(value)); }

    appendChild(node) {
      requireNode(node);
      native.append(this._handle, node._handle);
      return node;
    }

    insertBefore(node, before) {
      requireNode(node);
      if (before !== null && before !== undefined) requireNode(before, "before");
      native.insertBefore(this._handle, node._handle, before?._handle);
      return node;
    }

    removeChild(node) {
      requireNode(node);
      native.removeChild(this._handle, node._handle);
      return node;
    }

    replaceChildren(...nodes) {
      for (const node of nodes) requireNode(node);
      native.replaceChildren(this._handle, nodes.map(node => node._handle));
    }

    remove() {
      const parent = this.parentNode;
      if (parent) parent.removeChild(this);
    }

    contains(node) {
      if (!(node instanceof Node)) return false;
      for (let current = node; current; current = current.parentNode) {
        if (current === this) return true;
      }
      return false;
    }

    addEventListener(type, listener) {
      if (typeof listener !== "function") return;
      const name = String(type).toLowerCase();
      let listeners = this._listeners.get(name);
      if (!listeners) this._listeners.set(name, listeners = new Set());
      listeners.add(listener);
    }

    removeEventListener(type, listener) {
      this._listeners.get(String(type).toLowerCase())?.delete(listener);
    }

    dispatchEvent(event) {
      if (!event || typeof event.type !== "string") throw new TypeError("event.type is required");
      if (event.target === undefined) Object.defineProperty(event, "target", { value: this });
      for (const listener of this._listeners.get(event.type.toLowerCase()) ?? []) {
        listener.call(this, event);
      }
      return !event.defaultPrevented;
    }
  }

  Object.defineProperties(Node, {
    ELEMENT_NODE: { value: 1 },
    TEXT_NODE: { value: 3 },
    DOCUMENT_NODE: { value: 9 }
  });
  Object.defineProperties(Node.prototype, {
    ELEMENT_NODE: { value: 1 },
    TEXT_NODE: { value: 3 },
    DOCUMENT_NODE: { value: 9 }
  });

  class Element extends Node {
    constructor(token, handle) {
      super(token, handle);
      this._style = styleFor(this);
    }

    get tagName() { return native.nodeName(this._handle); }
    get localName() { return this.tagName.toLowerCase(); }
    get style() { return this._style; }
    get className() { return this.getAttribute("class") ?? ""; }
    set className(value) { this.setAttribute("class", value); }

    getAttribute(name) {
      return native.getAttribute(this._handle, String(name).toLowerCase()) ?? null;
    }

    hasAttribute(name) {
      return native.getAttribute(this._handle, String(name).toLowerCase()) !== undefined;
    }

    setAttribute(name, value) {
      native.setAttribute(this._handle, String(name).toLowerCase(), String(value));
    }

    removeAttribute(name) {
      native.removeAttribute(this._handle, String(name).toLowerCase());
    }
  }

  class Text extends Node {
    get data() { return native.textContent(this._handle); }
    set data(value) { native.setTextContent(this._handle, String(value)); }
    get nodeValue() { return this.data; }
    set nodeValue(value) { this.data = value ?? ""; }
  }

  function wrap(handle) {
    if (handle === undefined || handle === null) return null;
    let node = cache.get(handle);
    if (node) return node;
    node = native.nodeType(handle) === Node.TEXT_NODE
      ? new Text(construct, handle)
      : new Element(construct, handle);
    cache.set(handle, node);
    return node;
  }

  class Document {
    get nodeType() { return Node.DOCUMENT_NODE; }
    get nodeName() { return "#document"; }
    get documentElement() { return wrap(native.root()); }
    get body() { return wrap(native.body()); }
    get defaultView() { return globalThis; }
    createElement(name) { return wrap(native.createElement(String(name).toLowerCase())); }
    createTextNode(value) { return wrap(native.createTextNode(String(value))); }
  }

  Object.defineProperties(globalThis, {
    Node: { value: Node, configurable: true, writable: true },
    Element: { value: Element, configurable: true, writable: true },
    HTMLElement: { value: Element, configurable: true, writable: true },
    Text: { value: Text, configurable: true, writable: true },
    CSSStyleDeclaration: { value: CSSStyleDeclaration, configurable: true, writable: true },
    Document: { value: Document, configurable: true, writable: true },
    document: { value: new Document(), configurable: true, writable: true }
  });
  if (globalThis.window === undefined) globalThis.window = globalThis;
  delete globalThis.__burokkuDomNative;
})();
