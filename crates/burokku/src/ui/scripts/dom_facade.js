(native => {
  "use strict";

  const constructorKey = Symbol("Burokku node constructor");
  const handles = new WeakMap();
  const kinds = new WeakMap();
  const wrappers = new Map();
  let nextCacheGeneration = 1;

  const finalizers = new FinalizationRegistry(({ token, generation }) => {
    const current = wrappers.get(token);
    if (current?.generation === generation) wrappers.delete(token);
    native.releaseWrapper(token);
  });

  function handleOf(value) {
    const handle = handles.get(value);
    if (handle === undefined) {
      throw new TypeError("expected a Burokku Node");
    }
    return handle;
  }

  class Node {
    #listeners = new Map();

    constructor(key, token, kind) {
      if (key !== constructorKey) throw new TypeError("Illegal constructor");
      handles.set(this, token);
      kinds.set(this, kind);
    }

    get parentNode() {
      return wrap(native.parent(handleOf(this)));
    }

    get childNodes() {
      return native.children(handleOf(this)).map(wrap);
    }

    get firstChild() {
      return wrap(native.firstChild(handleOf(this)));
    }

    get lastChild() {
      return wrap(native.lastChild(handleOf(this)));
    }

    get nextSibling() {
      return wrap(native.nextSibling(handleOf(this)));
    }

    get previousSibling() {
      return wrap(native.previousSibling(handleOf(this)));
    }

    get isConnected() {
      return native.isConnected(handleOf(this));
    }

    appendChild(child) {
      native.appendChild(handleOf(this), handleOf(child));
      return child;
    }

    insertBefore(child, reference) {
      native.insertBefore(
        handleOf(this),
        handleOf(child),
        reference == null ? null : handleOf(reference),
      );
      return child;
    }

    removeChild(child) {
      native.removeChild(handleOf(this), handleOf(child));
      return child;
    }

    replaceChild(newChild, oldChild) {
      native.replaceChild(handleOf(this), handleOf(newChild), handleOf(oldChild));
      return oldChild;
    }

    contains(other) {
      return native.contains(handleOf(this), handleOf(other));
    }

    get textContent() {
      return native.textContent(handleOf(this));
    }

    set textContent(value) {
      native.setTextContent(handleOf(this), String(value));
    }

    get nodeValue() {
      if (kinds.get(this) !== "text") return null;
      return native.textContent(handleOf(this));
    }

    set nodeValue(value) {
      if (kinds.get(this) === "text") {
        native.setText(handleOf(this), String(value));
      }
    }

    addEventListener(type, callback) {
      if (typeof callback !== "function") {
        throw new TypeError("event listener must be a function");
      }
      const normalizedType = String(type);
      let callbacks = this.#listeners.get(normalizedType);
      if (callbacks === undefined) {
        callbacks = new Set();
        this.#listeners.set(normalizedType, callbacks);
      }
      callbacks.add(callback);
    }

    removeEventListener(type, callback) {
      if (typeof callback !== "function") return;
      const normalizedType = String(type);
      const callbacks = this.#listeners.get(normalizedType);
      if (callbacks === undefined) return;
      callbacks.delete(callback);
      if (callbacks.size === 0) this.#listeners.delete(normalizedType);
    }
  }

  class AppNode extends Node {
    createElement(tag) {
      return wrap(native.createElement(String(tag)));
    }

    createTextNode(data) {
      return wrap(native.createText(String(data)));
    }
  }

  class TextNode extends Node {
    get data() {
      return native.textContent(handleOf(this));
    }

    set data(value) {
      native.setText(handleOf(this), String(value));
    }
  }

  class BurokkuStyleDeclaration {
    #owner;

    constructor(key, owner) {
      if (key !== constructorKey) throw new TypeError("Illegal constructor");
      this.#owner = owner;
    }

    supportsProperty(name) {
      return native.supportsStyleProperty(handleOf(this.#owner), String(name));
    }

    setProperty(name, value) {
      native.setStyleProperty(
        handleOf(this.#owner),
        String(name),
        String(value),
      );
    }

    removeProperty(name) {
      native.removeStyleProperty(handleOf(this.#owner), String(name));
    }
  }

  class Element extends Node {
    #style;

    constructor(key, token, kind) {
      super(key, token, kind);
      this.#style = new BurokkuStyleDeclaration(constructorKey, this);
    }

    get localName() {
      return native.localName(handleOf(this));
    }

    get style() {
      return this.#style;
    }

    getAttribute(name) {
      const value = native.getAttribute(handleOf(this), String(name));
      return value === undefined ? null : value;
    }

    hasAttribute(name) {
      return native.hasAttribute(handleOf(this), String(name));
    }

    setAttribute(name, value) {
      native.setAttribute(handleOf(this), String(name), String(value));
    }

    removeAttribute(name) {
      native.removeAttribute(handleOf(this), String(name));
    }
  }

  class Window extends Element {}
  class Div extends Element {}
  class Flex extends Element {}
  class Grid extends Element {}
  class TextElement extends Element {}

  function construct(token, kind) {
    switch (kind) {
      case "app":
        return new AppNode(constructorKey, token, kind);
      case "text":
        return new TextNode(constructorKey, token, kind);
      case "window":
        return new Window(constructorKey, token, kind);
      case "div":
        return new Div(constructorKey, token, kind);
      case "flex":
        return new Flex(constructorKey, token, kind);
      case "grid":
        return new Grid(constructorKey, token, kind);
      case "text-element":
        return new TextElement(constructorKey, token, kind);
      default:
        throw new Error(`unknown native node kind: ${kind}`);
    }
  }

  function wrap(token) {
    if (token == null) return null;

    const cached = wrappers.get(token)?.reference.deref();
    if (cached !== undefined) return cached;

    const kind = native.kind(token);
    const wrapper = construct(token, kind);
    const generation = nextCacheGeneration++;
    native.acquireWrapper(token);
    try {
      wrappers.set(token, {
        generation,
        reference: new WeakRef(wrapper),
      });
      finalizers.register(wrapper, { token, generation });
      return wrapper;
    } catch (error) {
      const current = wrappers.get(token);
      if (current?.generation === generation) wrappers.delete(token);
      native.releaseWrapper(token);
      throw error;
    }
  }

  Object.defineProperties(globalThis, {
    Node: { value: Node, writable: false, configurable: false },
    AppNode: { value: AppNode, writable: false, configurable: false },
    TextNode: { value: TextNode, writable: false, configurable: false },
    Element: { value: Element, writable: false, configurable: false },
    Window: { value: Window, writable: false, configurable: false },
    Div: { value: Div, writable: false, configurable: false },
    Flex: { value: Flex, writable: false, configurable: false },
    Grid: { value: Grid, writable: false, configurable: false },
    TextElement: {
      value: TextElement,
      writable: false,
      configurable: false,
    },
    BurokkuStyleDeclaration: {
      value: BurokkuStyleDeclaration,
      writable: false,
      configurable: false,
    },
    app: {
      value: wrap(native.root()),
      writable: false,
      configurable: false,
      enumerable: true,
    },
  });
})
