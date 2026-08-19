(() => {
  "use strict";

  const native = globalThis.__burokkuDomNative;
  const construct = Symbol("Burokku DOM node");
  const cache = new Map();
  const finalizers = new FinalizationRegistry(handle => {
    // A replacement wrapper can be created after the old WeakRef is cleared
    // but before its finalizer runs. Keep that newer cache entry while still
    // releasing the old wrapper's native lease.
    const reference = cache.get(handle);
    if (!reference || reference.deref() === undefined) cache.delete(handle);
    native.release(handle);
  });

  function requireNode(value, label = "node") {
    if (!(value instanceof Node)) throw new TypeError(`${label} must be a Node`);
    return value;
  }

  function cssName(name) {
    name = String(name);
    if (name.startsWith("--") || name.includes("-")) return name.toLowerCase();
    return name.replace(/[A-Z]/g, letter => `-${letter.toLowerCase()}`).toLowerCase();
  }

  function warnUnsupportedStyle(name) {
    const message = `Unsupported or invalid style property "${String(name)}" was ignored.`;
    if (globalThis.console && typeof globalThis.console.warn === "function") {
      globalThis.console.warn(message);
    }
  }

  class CSSStyleDeclaration {
    constructor(node) {
      this._node = node;
      this._values = new Map();
    }

    getPropertyValue(name) {
      const normalized = cssName(name);
      if (!native.supportsStyle(this._node._handle, normalized)) {
        warnUnsupportedStyle(normalized);
        return "";
      }
      return this._values.get(normalized) ?? "";
    }

    setProperty(name, value) {
      const normalized = cssName(name);
      const stringValue = String(value);
      if (!native.setStyle(this._node._handle, normalized, stringValue)) {
        warnUnsupportedStyle(normalized);
        return;
      }
      this._values.set(normalized, stringValue);
    }

    removeProperty(name) {
      const normalized = cssName(name);
      const previous = this._values.get(normalized) ?? "";
      if (!native.removeStyle(this._node._handle, normalized)) {
        warnUnsupportedStyle(normalized);
        return "";
      }
      this._values.delete(normalized);
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
        target.setProperty(property, value);
        return true;
      }
    });
  }

  class Event {
    constructor(type, init = {}) {
      if (type === undefined || type === null) throw new TypeError("event.type is required");
      Object.assign(this, init);
      this.type = String(type);
      this.bubbles = Boolean(init.bubbles);
      this.cancelable = Boolean(init.cancelable);
      this.defaultPrevented = false;
      this.target = null;
      this.currentTarget = null;
      this._propagationStopped = false;
      this._immediatePropagationStopped = false;
      this._path = [];
    }

    composedPath() {
      return this._path.slice();
    }

    preventDefault() {
      if (this.cancelable) this.defaultPrevented = true;
    }

    stopPropagation() {
      this._propagationStopped = true;
    }

    stopImmediatePropagation() {
      this._propagationStopped = true;
      this._immediatePropagationStopped = true;
    }
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
      if (event.target === undefined || event.target === null) {
        Object.defineProperty(event, "target", { value: this, configurable: true });
      }
      const path = [this];
      if (event.bubbles) {
        for (let current = this.parentNode; current; current = current.parentNode) path.push(current);
        path.push(document);
      }
      event._path = path;
      for (const current of path) {
        Object.defineProperty(event, "currentTarget", { value: current, configurable: true });
        for (const listener of current._listeners.get(event.type.toLowerCase()) ?? []) {
          listener.call(current, event);
          if (event._immediatePropagationStopped) break;
        }
        if (event._propagationStopped) break;
      }
      Object.defineProperty(event, "currentTarget", { value: null, configurable: true });
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
      return native.getAttribute(this._handle, String(name).toLowerCase());
    }

    hasAttribute(name) {
      return this.getAttribute(name) !== null;
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
    let node = cache.get(handle)?.deref();
    if (node) return node;
    node = native.nodeType(handle) === Node.TEXT_NODE
      ? new Text(construct, handle)
      : new Element(construct, handle);
    native.retain(handle);
    cache.set(handle, new WeakRef(node));
    finalizers.register(node, handle);
    return node;
  }

  class Document {
    constructor() {
      this._listeners = new Map();
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
      if (event.target === undefined || event.target === null) {
        Object.defineProperty(event, "target", { value: this, configurable: true });
      }
      event._path = [this];
      Object.defineProperty(event, "currentTarget", { value: this, configurable: true });
      for (const listener of this._listeners.get(event.type.toLowerCase()) ?? []) {
        listener.call(this, event);
        if (event._immediatePropagationStopped) break;
      }
      Object.defineProperty(event, "currentTarget", { value: null, configurable: true });
      return !event.defaultPrevented;
    }

    get nodeType() { return Node.DOCUMENT_NODE; }
    get nodeName() { return "#document"; }
    get documentElement() { return wrap(native.root()); }
    get body() { return wrap(native.body()); }
    get defaultView() { return globalThis; }
    createElement(name) { return wrap(native.createElement(String(name).toLowerCase())); }
    createTextNode(value) { return wrap(native.createTextNode(String(value))); }
  }

  Object.defineProperties(globalThis, {
    Event: { value: Event, configurable: true, writable: true },
    Node: { value: Node, configurable: true, writable: true },
    Element: { value: Element, configurable: true, writable: true },
    HTMLElement: { value: Element, configurable: true, writable: true },
    Text: { value: Text, configurable: true, writable: true },
    CSSStyleDeclaration: { value: CSSStyleDeclaration, configurable: true, writable: true },
    Document: { value: Document, configurable: true, writable: true },
    document: { value: new Document(), configurable: true, writable: true }
  });
  Object.defineProperty(globalThis, "__burokkuDispatchNativeEvent", {
    configurable: true,
    value(handle, init) {
      // A target selected from the presented revision may be gone by the time
      // BTS reaches this macrotask. Generation validation makes that a quiet
      // dropped event rather than dispatching to a reused or deleted node.
      if (!native.contains(handle)) return false;
      return wrap(handle).dispatchEvent(new Event(init.type, init));
    }
  });

  if (globalThis.window === undefined) globalThis.window = globalThis;
  delete globalThis.__burokkuDomNative;
})();
