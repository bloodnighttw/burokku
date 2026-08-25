# DOM node model

## Rust representation

The authoritative DOM model is defined in
`crates/burokku/src/ui/elements.rs`:

```rust
pub enum NodeKind {
    App,
    Element(Element),
    Text(String),
}
```

`App` is a node kind, not an element. The element variants are:

```rust
pub enum Element {
    Window { /* ... */ },
    Div { /* ... */ },
    Flex { /* ... */ },
    Grid { /* ... */ },
    Text { /* ... */ },
}
```

## JavaScript facade

The JavaScript API exposes an object-oriented facade over the Rust node kinds:

```text
Node
├── AppNode
├── TextNode
└── Element
    ├── Window
    ├── Div
    ├── Flex
    ├── Grid
    └── TextElement
```

- `Node` provides behavior shared by every node, including identity, parent and
  child traversal, insertion, and removal.
- `AppNode` represents `NodeKind::App`, extends `Node` directly, and owns the
  node factory methods `createElement` and `createTextNode`.
- `TextNode` represents `NodeKind::Text` and adds text data operations.
- `Element` represents `NodeKind::Element` and provides tag, attribute, style,
  and element-specific behavior.
- `TextElement` is a styled element and is distinct from `TextNode`, which holds
  raw text data.

The facade uses inheritance and polymorphism, while the Rust implementation can
continue using enums and composition.

## Child relationship contract

Raw strings are represented by `TextNode`, and an attached `TextNode` is valid
only beneath a `TextElement`. Text elements may nest recursively to introduce
inherited styled runs.

```text
AppNode
└── Window
    ├── Div | Flex | Grid
    │   └── Div | Flex | Grid | TextElement
    └── TextElement
        ├── TextNode
        └── TextElement
            ├── TextNode
            └── TextElement ...
```

More precisely:

- `AppNode` accepts one `Window`;
- `Window`, `Div`, `Flex`, and `Grid` accept `Div`, `Flex`, `Grid`, and
  `TextElement`, but never `TextNode`;
- `TextElement` accepts `TextNode` and nested `TextElement` children only;
- `TextNode` is a leaf.

`app.createTextNode(...)` may return a detached text node. Attempting to insert
it beneath a non-text parent must fail without changing either parent. The host
must not silently wrap it in a text element because that would change observable
node identity and structure.

Therefore `<div><text>Application content</text></div>` is valid, while
`<div>Application content</div>` is not.

## Application mount root

The host exposes the existing app root as:

```ts
interface AppNode extends Node {
  createElement(tag: BurokkuTagName): Element;
  createTextNode(data: string): TextNode;
}

declare global {
  var app: AppNode;
}
```

`globalThis.app` is permanent and host-created. It is the mount target for UI
frameworks:

```tsx
render(() => (
  <window>
    <div><text>Application content</text></div>
  </window>
), globalThis.app);
```

The app node:

- is not an element;
- has no element tag, attributes, layout style, or paint style;
- cannot be created by script;
- is the sole factory for script-created elements and text nodes through
  `app.createElement(...)` and `app.createTextNode(...)`;
- returns newly created nodes detached from the tree;
- accepts only `Window` element children;
- currently permits only one attached window;
- uses the normal inherited `Node` mutation and traversal operations, with its
  child constraints enforced by the native DOM.

Adding a window under the app root associates that DOM node with a native
window. The native integration maintains the mapping internally:

```text
DOM NodeId -> native WindowId
```

User-provided attributes do not determine native identity. Framework-specific
keys are reconciliation hints and are separate from the DOM `NodeId`.
