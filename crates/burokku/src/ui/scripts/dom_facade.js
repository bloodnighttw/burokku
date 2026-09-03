((nodeMethods, styleMethods) => {
  "use strict";

  function constructor(name, parent = null) {
    const value = function () {
      throw new TypeError("Illegal constructor");
    };
    Object.defineProperty(value, "name", { value: name });
    if (parent) Object.setPrototypeOf(value, parent);
    value.prototype = Object.create(parent?.prototype ?? Object.prototype);
    Object.defineProperty(value.prototype, "constructor", { value });
    return value;
  }

  function copy(target, source, names) {
    for (const name of names) {
      Object.defineProperty(
        target,
        name,
        Object.getOwnPropertyDescriptor(source, name),
      );
    }
  }

  const Node = constructor("Node");
  const AppNode = constructor("AppNode", Node);
  const TextNode = constructor("TextNode", Node);
  const Element = constructor("Element", Node);
  const Window = constructor("Window", Element);
  const Div = constructor("Div", Element);
  const Flex = constructor("Flex", Element);
  const Grid = constructor("Grid", Element);
  const TextElement = constructor("TextElement", Element);
  const BurokkuStyleDeclaration = constructor("BurokkuStyleDeclaration");

  copy(Node.prototype, nodeMethods, [
    "parentNode",
    "childNodes",
    "firstChild",
    "lastChild",
    "nextSibling",
    "previousSibling",
    "isConnected",
    "appendChild",
    "insertBefore",
    "removeChild",
    "replaceChild",
    "contains",
    "textContent",
    "nodeValue",
  ]);
  copy(AppNode.prototype, nodeMethods, ["createElement", "createTextNode"]);
  copy(TextNode.prototype, nodeMethods, ["data"]);
  copy(Element.prototype, nodeMethods, [
    "localName",
    "getBoundingClientRect",
    "getAttribute",
    "hasAttribute",
    "setAttribute",
    "removeAttribute",
  ]);
  copy(BurokkuStyleDeclaration.prototype, styleMethods, [
    "supportsProperty",
    "setProperty",
    "removeProperty",
  ]);

  const listeners = new WeakMap();
  Object.defineProperties(Node.prototype, {
    addEventListener: {
      value(type, callback) {
        if (typeof callback !== "function") {
          throw new TypeError("event listener must be a function");
        }
        let byType = listeners.get(this);
        if (!byType) listeners.set(this, (byType = new Map()));
        const normalizedType = String(type);
        let callbacks = byType.get(normalizedType);
        if (!callbacks) byType.set(normalizedType, (callbacks = new Set()));
        callbacks.add(callback);
      },
    },
    removeEventListener: {
      value(type, callback) {
        if (typeof callback !== "function") return;
        const byType = listeners.get(this);
        const callbacks = byType?.get(String(type));
        if (!callbacks) return;
        callbacks.delete(callback);
        if (callbacks.size === 0) byType.delete(String(type));
      },
    },
  });

  const wrappers = new Map();
  let nextGeneration = 1;
  const finalizers = new FinalizationRegistry(({ token, generation }) => {
    if (wrappers.get(token)?.generation === generation) wrappers.delete(token);
  });
  const getWrapper = token => wrappers.get(token)?.reference.deref();
  const cacheWrapper = (token, wrapper) => {
    const generation = nextGeneration++;
    wrappers.set(token, { generation, reference: new WeakRef(wrapper) });
    finalizers.register(wrapper, { token, generation });
  };

  Object.defineProperties(nodeMethods, {
    getCachedWrapper: { value: getWrapper },
    cacheWrapper: { value: cacheWrapper },
  });

  Object.defineProperties(globalThis, {
    Node: { value: Node, writable: false, configurable: false },
    AppNode: { value: AppNode, writable: false, configurable: false },
    TextNode: { value: TextNode, writable: false, configurable: false },
    Element: { value: Element, writable: false, configurable: false },
    Window: { value: Window, writable: false, configurable: false },
    Div: { value: Div, writable: false, configurable: false },
    Flex: { value: Flex, writable: false, configurable: false },
    Grid: { value: Grid, writable: false, configurable: false },
    TextElement: { value: TextElement, writable: false, configurable: false },
    BurokkuStyleDeclaration: {
      value: BurokkuStyleDeclaration,
      writable: false,
      configurable: false,
    },
  });
})
