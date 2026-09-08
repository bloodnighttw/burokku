# `UiDomState` refactor plan

## Decision

Do not split `UiDomState` into several wrappers only to make the field list shorter. Refactor ownership where it is currently wrong, then rename the remaining shared state to describe its actual role.

The main defect is the pointer interaction state split between `ApplicationHost` and `UiDomState`. The DOM arena, JavaScript wrapper roots, task queue, and last-presented layout all support the JavaScript DOM binding and may remain behind one UI-thread shared handle.

## Target responsibilities

### `Dom`

Keep `Dom` as the authoritative document model:

- nodes and tree relationships;
- attributes and styles;
- structural validation;
- document revision.

It must not own JavaScript, input, layout, window, or renderer state.

### DOM binding state

Rename `UiDomState` and `SharedUiDom` to `DomBindingState` and `SharedDomBindings` after the ownership changes are complete.

The binding state may own:

- `Dom`;
- JavaScript wrapper roots needed to retain and reclaim DOM nodes;
- the JavaScript task queue used to dispatch DOM events;
- the last successfully presented layout exposed by geometry APIs;
- one dispatch-time pointer state machine.

These fields are cohesive because they implement the live JavaScript DOM contract. The new name must not imply that the structure is only the DOM model.

### `ApplicationHost`

Keep native/platform concerns in `ApplicationHost`:

- winit window events and native window lifecycle;
- cursor coordinates and physical button facts reported by winit;
- hit testing against the last successfully presented scene;
- layout, rendering, GPU, and presentation lifecycle;
- runtime shutdown and fatal-error policy.

The host must not decide DOM-level hover transitions, pointer capture routing, click recognition, or whether a pointer interaction is active.

## Migration phases

### 1. Lock down pointer transition behavior

Before moving state, add compact regression tests for:

1. down → capture in JavaScript → move before the queue drains;
2. down → capture → release over another target;
3. down → resize/window loss → cancellation;
4. down → full task queue → guaranteed cancellation;
5. chorded button transitions;
6. capture target detached before the next input.

Prefer transition assertions over repeating every event payload field.

### 2. Remove accidental retained state

Change `reclaim_detached` to return `ReclaimReport` directly. Delete `UiDomState::last_reclaim`; callers and tests should inspect the returned report.

This field records the previous maintenance operation rather than durable binding state, so retaining it makes ownership less clear.

### 3. Queue native pointer facts

Replace host-produced DOM event decisions with one input record containing only facts available when winit delivers input:

- physical/logical position;
- changed button and current button mask;
- movement, button, wheel, exit, or cancellation input kind;
- hit-test candidate and presented revision, where required to preserve presented-frame semantics;
- wheel deltas.

Do not put `event_type`, `click_target`, hover transitions, or capture-adjusted targets in this record.

### 4. Consolidate pointer authority at dispatch time

Move these host fields and decisions into the binding-side pointer state machine:

- `pressed_target`;
- `active_pointer` semantics needed for cancellation;
- click recognition;
- hover path;
- pointer active state;
- requested and announced pointer capture.

When a queued input is dispatched, resolve in this order:

1. validate live nodes and presented input data;
2. apply pending capture transitions;
3. choose the effective target;
4. derive hover boundary events;
5. derive pointer/wheel events;
6. recognize click from dispatch-time down/up targets;
7. clear terminal interaction and capture state.

A queue failure must not advance only one copy of pointer state. Terminal release/cancellation must be retried or represented by a guaranteed queued cancellation.

### 5. Rename the remaining shared state

After pointer authority is singular, rename:

- `UiDomState` → `DomBindingState`;
- `SharedUiDom` → `SharedDomBindings`;
- `ApplicationHost::dom` → `dom_bindings`.

Update module comments to state that this is UI-thread-only JavaScript binding state around one `Dom`, not the document model itself.

Do not add interfaces, factories, or separate `Rc<RefCell<_>>` handles unless an actual borrow conflict requires independent borrowing.

### 6. Simplify obsolete host code

Delete host helpers and fields that construct derived DOM events or duplicate binding state. Expected removals include:

- `recognize_primary_click`;
- `pressed_target`;
- host-side `active_pointer` bookkeeping once cancellation is binding-owned;
- preconstructed mouse-event batches;
- duplicated capture/click fallback branches.

Keep hit testing in the host because `ScenePlan` and physical presentation state are host-owned.

## Validation

Run after each phase:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Also confirm:

- JavaScript DOM mutations remain synchronous;
- geometry APIs expose only the last successfully presented layout;
- detached wrapper reclamation still preserves live detached components;
- pointer behavior is determined by queue order, not enqueue-time guesses;
- `ApplicationHost` no longer owns DOM-level pointer decisions.

## Non-goals

- redesigning `Dom`;
- introducing multi-pointer or touch support;
- changing the public JavaScript event contract;
- splitting every binding concern into its own allocation;
- refactoring rendering, layout, or native window management.

Finish this refactor before adding multi-pointer support, because multiple pointers would multiply the current split-authority problem.
