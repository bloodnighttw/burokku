(() => {
  "use strict";

  const camelToKebab = name => name.replace(/[A-Z]/g, letter => `-${letter.toLowerCase()}`);
  const listeners = new WeakMap();

  class EventTarget {
    addEventListener(type, callback) {
      if (typeof callback !== "function" && typeof callback?.handleEvent !== "function") return;
      let targetListeners = listeners.get(this);
      if (!targetListeners) listeners.set(this, targetListeners = new Map());
      let callbacks = targetListeners.get(type);
      if (!callbacks) targetListeners.set(type, callbacks = new Set());
      callbacks.add(callback);
    }

    removeEventListener(type, callback) {
      listeners.get(this)?.get(type)?.delete(callback);
    }

    dispatchEvent(event) {
      event.target ??= this;
      event.currentTarget = this;
      for (const callback of listeners.get(this)?.get(event.type) ?? []) {
        if (typeof callback === "function") callback.call(this, event);
        else callback.handleEvent(event);
      }
      return !event.defaultPrevented;
    }
  }

  class Node extends EventTarget {
    constructor(id, nodeType, nodeName) {
      super();
      this.__burokkuId = id;
      this.nodeType = nodeType;
      this.nodeName = nodeName;
      this.parentNode = null;
      this.childNodes = [];
      this.ownerDocument = null;
    }

    get firstChild() { return this.childNodes[0] ?? null; }
    get lastChild() { return this.childNodes[this.childNodes.length - 1] ?? null; }
    get parentElement() { return this.parentNode?.nodeType === 1 ? this.parentNode : null; }
    get nextSibling() {
      if (!this.parentNode) return null;
      const index = this.parentNode.childNodes.indexOf(this);
      return this.parentNode.childNodes[index + 1] ?? null;
    }
    get previousSibling() {
      if (!this.parentNode) return null;
      const index = this.parentNode.childNodes.indexOf(this);
      return index > 0 ? this.parentNode.childNodes[index - 1] : null;
    }
    get isConnected() { return this === document.body || Boolean(this.parentNode?.isConnected); }

    appendChild(child) { return this.insertBefore(child, null); }

    insertBefore(child, before) {
      if (!(child instanceof Node)) throw new TypeError("child must be a Node");
      if (child.nodeType === 11) {
        for (const nested of [...child.childNodes]) this.insertBefore(nested, before);
        return child;
      }
      if (before !== null && before.parentNode !== this) {
        throw new Error("The reference node is not a child of this node");
      }
      if (child === before) return child;
      if (child.parentNode) {
        const oldParent = child.parentNode;
        oldParent.childNodes.splice(oldParent.childNodes.indexOf(child), 1);
      }
      const index = before === null ? this.childNodes.length : this.childNodes.indexOf(before);
      this.childNodes.splice(index, 0, child);
      child.parentNode = this;
      if (this.__burokkuId !== null && child.__burokkuId !== null) {
        __burokku_dom_insert(this.__burokkuId, child.__burokkuId, before?.__burokkuId ?? -1);
      }
      return child;
    }

    removeChild(child) {
      const index = this.childNodes.indexOf(child);
      if (index < 0) throw new Error("The node to remove is not a child of this node");
      this.childNodes.splice(index, 1);
      child.parentNode = null;
      if (this.__burokkuId !== null && child.__burokkuId !== null) {
        __burokku_dom_remove(this.__burokkuId, child.__burokkuId);
      }
      return child;
    }

    replaceChild(next, previous) {
      this.insertBefore(next, previous);
      this.removeChild(previous);
      return previous;
    }

    append(...children) {
      for (const child of children) {
        this.appendChild(child instanceof Node ? child : document.createTextNode(String(child)));
      }
    }

    prepend(...children) {
      for (const child of [...children].reverse()) {
        this.insertBefore(child instanceof Node ? child : document.createTextNode(String(child)), this.firstChild);
      }
    }

    replaceChildren(...children) {
      for (const child of [...this.childNodes]) this.removeChild(child);
      this.append(...children);
    }

    remove() { this.parentNode?.removeChild(this); }
    contains(node) {
      for (let current = node; current; current = current.parentNode) if (current === this) return true;
      return false;
    }
    getRootNode() { let node = this; while (node.parentNode) node = node.parentNode; return node; }

    get textContent() {
      return this.nodeType === 3 || this.nodeType === 8
        ? this.data
        : this.childNodes.map(child => child.textContent).join("");
    }
    set textContent(value) {
      if (this.nodeType === 3 || this.nodeType === 8) {
        this.data = String(value ?? "");
      } else {
        this.replaceChildren();
        if (value !== null && value !== undefined && String(value) !== "") {
          this.appendChild(document.createTextNode(String(value)));
        }
      }
    }

    cloneNode(deep = false) {
      let clone;
      if (this.nodeType === 1) {
        clone = document.createElement(this.localName);
        for (const [name, value] of this.__attributes) clone.setAttribute(name, value);
        for (const [name, value] of this.__styleValues) clone.style.setProperty(name, value);
      } else if (this.nodeType === 3) clone = document.createTextNode(this.data);
      else if (this.nodeType === 8) clone = document.createComment(this.data);
      else clone = document.createDocumentFragment();
      if (deep) for (const child of this.childNodes) clone.appendChild(child.cloneNode(true));
      return clone;
    }
  }
  Node.ELEMENT_NODE = 1;
  Node.TEXT_NODE = 3;
  Node.COMMENT_NODE = 8;
  Node.DOCUMENT_NODE = 9;
  Node.DOCUMENT_FRAGMENT_NODE = 11;

  const styleMethods = values => ({
    setProperty(name, value) {
      name = camelToKebab(String(name));
      value = String(value ?? "");
      if (value === "") return this.removeProperty(name);
      values.set(name, value);
      __burokku_dom_set_style(this.__elementId, name, value);
    },
    removeProperty(name) {
      name = camelToKebab(String(name));
      const previous = values.get(name) ?? "";
      values.delete(name);
      __burokku_dom_set_style(this.__elementId, name, null);
      return previous;
    },
    getPropertyValue(name) { return values.get(camelToKebab(String(name))) ?? ""; },
  });

  const createStyle = (id, values) => new Proxy(styleMethods(values), {
    get(target, name) {
      if (name in target) return target[name];
      if (name === "cssText") return [...values].map(([key, value]) => `${key}: ${value};`).join(" ");
      if (name === "__elementId") return id;
      return values.get(camelToKebab(String(name))) ?? "";
    },
    set(target, name, value) {
      if (name === "cssText") {
        for (const key of [...values.keys()]) target.removeProperty.call({ __elementId: id }, key);
        for (const declaration of String(value).split(";")) {
          const colon = declaration.indexOf(":");
          if (colon > 0) target.setProperty.call({ __elementId: id }, declaration.slice(0, colon).trim(), declaration.slice(colon + 1).trim());
        }
      } else {
        target.setProperty.call({ __elementId: id }, camelToKebab(String(name)), value);
      }
      return true;
    },
    has: () => true,
  });

  class Element extends Node {
    constructor(id, name) {
      super(id, 1, name.toUpperCase());
      this.localName = name.toLowerCase();
      this.tagName = this.nodeName;
      this.namespaceURI = "http://www.w3.org/1999/xhtml";
      this.__attributes = new Map();
      this.__styleValues = new Map();
      this.style = createStyle(id, this.__styleValues);
    }
    setAttribute(name, value) {
      name = String(name).toLowerCase();
      value = String(value);
      this.__attributes.set(name, value);
      if (name === "style") this.style.cssText = value;
      if (this.__burokkuId !== null) __burokku_dom_set_attribute(this.__burokkuId, name, value);
    }
    getAttribute(name) { return this.__attributes.get(String(name).toLowerCase()) ?? null; }
    hasAttribute(name) { return this.__attributes.has(String(name).toLowerCase()); }
    removeAttribute(name) {
      name = String(name).toLowerCase();
      this.__attributes.delete(name);
      if (name === "style") this.style.cssText = "";
      if (this.__burokkuId !== null) __burokku_dom_set_attribute(this.__burokkuId, name, null);
    }
    setAttributeNS(_namespace, name, value) { this.setAttribute(name, value); }
    removeAttributeNS(_namespace, name) { this.removeAttribute(name); }
    get children() { return this.childNodes.filter(child => child.nodeType === 1); }
    get firstElementChild() { return this.children[0] ?? null; }
    get lastElementChild() { return this.children[this.children.length - 1] ?? null; }
    get className() { return this.getAttribute("class") ?? ""; }
    set className(value) { this.setAttribute("class", value); }
    get id() { return this.getAttribute("id") ?? ""; }
    set id(value) { this.setAttribute("id", value); }
    get innerHTML() { return this.textContent; }
    set innerHTML(value) { this.textContent = value; }
    focus() {
      if (this.disabled) return;
      if (document.activeElement === this) return;
      document.activeElement?.removeAttribute?.("data-burokku-focused");
      document.activeElement = this;
      this.setAttribute("data-burokku-focused", "");
    }
    blur() {
      if (document.activeElement !== this) return;
      this.removeAttribute("data-burokku-focused");
      document.activeElement = document.body;
    }
    querySelector() { return null; }
  }

  class HTMLElement extends Element {}
  class HTMLButtonElement extends HTMLElement {
    get disabled() { return this.hasAttribute("disabled"); }
    set disabled(value) {
      if (value) {
        this.blur();
        this.setAttribute("disabled", "");
      } else {
        this.removeAttribute("disabled");
      }
    }
  }
  class HTMLOptionElement extends HTMLElement {
    get disabled() { return this.hasAttribute("disabled"); }
    set disabled(value) { value ? this.setAttribute("disabled", "") : this.removeAttribute("disabled"); }
    get selected() {
      const select = this.parentElement;
      if (select instanceof HTMLSelectElement) return select.__selectedOptions().includes(this);
      if (this.hasAttribute("data-burokku-option-explicit")) {
        return this.hasAttribute("data-burokku-selected");
      }
      return this.hasAttribute("selected");
    }
    set selected(value) {
      const select = this.parentElement;
      if (select instanceof HTMLSelectElement) {
        if (select.multiple) {
          select.__markSelectionExplicit();
          value
            ? this.setAttribute("data-burokku-selected", "")
            : this.removeAttribute("data-burokku-selected");
        } else if (value) {
          select.__setSelection(this);
        } else if (this.selected) {
          select.__setSelection(null);
        }
      } else {
        this.setAttribute("data-burokku-option-explicit", "");
        value
          ? this.setAttribute("data-burokku-selected", "")
          : this.removeAttribute("data-burokku-selected");
      }
    }
    get value() { return this.getAttribute("value") ?? this.textContent; }
    set value(value) { this.setAttribute("value", value); }
  }
  class HTMLSelectElement extends HTMLElement {
    get disabled() { return this.hasAttribute("disabled"); }
    set disabled(value) {
      if (value) {
        this.blur();
        this.setAttribute("disabled", "");
      } else {
        this.removeAttribute("disabled");
      }
    }
    get multiple() { return this.hasAttribute("multiple"); }
    set multiple(value) {
      if (value) {
        this.setAttribute("multiple", "");
      } else {
        const selected = this.__selectedOptions().at(-1) ?? null;
        this.removeAttribute("multiple");
        if (this.hasAttribute("data-burokku-selection-explicit")) this.__setSelection(selected);
      }
    }
    get options() { return this.children.filter(child => child instanceof HTMLOptionElement); }
    __markSelectionExplicit() {
      this.setAttribute("data-burokku-selection-explicit", "");
    }
    __selectedOptions() {
      if (this.hasAttribute("data-burokku-selection-explicit")) {
        const selected = this.options.filter(option => option.hasAttribute("data-burokku-selected"));
        return this.multiple || selected.length === 0 ? selected : [selected[selected.length - 1]];
      }
      const selected = this.options.filter(option => option.hasAttribute("data-burokku-selected")
        || (!option.hasAttribute("data-burokku-option-explicit") && option.hasAttribute("selected")));
      if (this.multiple) return selected;
      if (selected.length) return [selected[selected.length - 1]];
      const fallback = this.options.find(option => !option.disabled) ?? this.options[0];
      return fallback ? [fallback] : [];
    }
    __setSelection(selected) {
      this.__markSelectionExplicit();
      for (const option of this.options) {
        if (option === selected) option.setAttribute("data-burokku-selected", "");
        else option.removeAttribute("data-burokku-selected");
      }
    }
    get selectedIndex() {
      const selected = this.__selectedOptions()[0];
      return selected ? this.options.indexOf(selected) : -1;
    }
    set selectedIndex(index) {
      const selected = this.options[Number(index)] ?? null;
      this.__setSelection(selected);
    }
    get value() { return this.__selectedOptions()[0]?.value ?? ""; }
    set value(value) {
      const selected = this.options.find(option => option.value === String(value)) ?? null;
      this.__setSelection(selected);
    }
  }
  class Text extends Node {
    constructor(id, data) {
      super(id, 3, "#text");
      this._data = String(data);
    }
    get data() { return this._data; }
    set data(value) {
      this._data = String(value);
      __burokku_dom_set_text(this.__burokkuId, this._data);
    }
    get nodeValue() { return this.data; }
    set nodeValue(value) { this.data = value ?? ""; }
  }
  class Comment extends Text {
    constructor(id, data) { super(id, data); this.nodeType = 8; this.nodeName = "#comment"; }
  }
  class DocumentFragment extends Node {
    constructor() { super(null, 11, "#document-fragment"); }
  }

  class Document extends EventTarget {
    constructor() {
      super();
      this.nodeType = 9;
      this.nodeName = "#document";
      this.defaultView = globalThis;
      this.documentElement = new HTMLElement(null, "html");
      this.body = new HTMLElement(0, "body");
      this.body.ownerDocument = this;
      this.documentElement.ownerDocument = this;
      this.documentElement.childNodes.push(this.body);
      this.body.parentNode = this.documentElement;
      this.activeElement = this.body;
      this.readyState = "complete";
    }
    createElement(name) {
      name = String(name).toLowerCase();
      const ElementClass = name === "button" ? HTMLButtonElement
        : name === "select" ? HTMLSelectElement
        : name === "option" ? HTMLOptionElement
        : HTMLElement;
      const element = new ElementClass(__burokku_dom_create("element", name), name);
      element.ownerDocument = this;
      return element;
    }
    createElementNS(_namespace, name) { return this.createElement(name); }
    createTextNode(data) {
      const node = new Text(__burokku_dom_create("text", ""), data);
      node.ownerDocument = this;
      __burokku_dom_set_text(node.__burokkuId, node.data);
      return node;
    }
    createComment(data) {
      const node = new Comment(__burokku_dom_create("comment", ""), data);
      node.ownerDocument = this;
      __burokku_dom_set_text(node.__burokkuId, node.data);
      return node;
    }
    createDocumentFragment() { const node = new DocumentFragment(); node.ownerDocument = this; return node; }
    getElementById(id) {
      const visit = node => node.id === id ? node : node.childNodes.map(visit).find(Boolean);
      return visit(this.documentElement) ?? null;
    }
    querySelector(selector) {
      if (selector === "body") return this.body;
      if (selector === "html") return this.documentElement;
      if (selector.startsWith("#")) return this.getElementById(selector.slice(1));
      return null;
    }
  }

  const document = new Document();
  Object.assign(globalThis, {
    window: globalThis,
    self: globalThis,
    document,
    EventTarget,
    Node,
    Element,
    HTMLElement,
    HTMLButtonElement,
    HTMLSelectElement,
    HTMLOptionElement,
    HTMLIFrameElement: class HTMLIFrameElement extends HTMLElement {},
    SVGElement: class SVGElement extends Element {},
    Text,
    Comment,
    DocumentFragment,
  });
  globalThis.navigator ??= { userAgent: "Burokku" };
  globalThis.addEventListener = EventTarget.prototype.addEventListener;
  globalThis.removeEventListener = EventTarget.prototype.removeEventListener;
  globalThis.dispatchEvent = EventTarget.prototype.dispatchEvent;

  const previousDispatch = globalThis.__burokku_dispatch_event;
  globalThis.__burokku_dispatch_event = event => {
    previousDispatch?.(event);
    globalThis.dispatchEvent(event);
  };
})();
