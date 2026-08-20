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
- `AppNode` represents `NodeKind::App` and extends `Node` directly.
- `TextNode` represents `NodeKind::Text` and adds text data operations.
- `Element` represents `NodeKind::Element` and provides tag, attribute, style,
  and element-specific behavior.
- `TextElement` is a styled element and is distinct from `TextNode`, which holds
  raw text data.

The facade uses inheritance and polymorphism, while the Rust implementation can
continue using enums and composition.

## Application mount root

The host exposes the existing app root as:

```ts
declare global {
  var app: AppNode;
}
```

`globalThis.app` is permanent and host-created. It is the mount target for UI
frameworks:

```tsx
render(() => (
  <window>
    <div>Application content</div>
  </window>
), globalThis.app);
```

The app node:

- is not an element;
- has no element tag, attributes, layout style, or paint style;
- cannot be created by script;
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
