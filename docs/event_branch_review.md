# Event branch review

Comparison: `feat/event` (`1e778bd39`) against `main` (`ed54619b6`).

## Conclusion

The branch is overdesigned in where pointer state is managed, not in supporting events. Pointer decisions happen on both sides of an asynchronous JavaScript task queue, which causes most of the correctness problems below.

## Overdesign

### Split pointer state machine

Relevant code:

- `crates/burokku/src/ui/host.rs:83-167`
- `crates/burokku/src/ui/host.rs:500-726`
- `crates/burokku/src/ui/dom_plugin/classes.rs:1000-1077`

The host resolves hit targets, hover transitions, clicks, and sampled capture before enqueueing work. The DOM dispatcher later resolves capture again while executing the queued task.

Queue raw native input instead. Resolve hit target, capture, hover, click, and cancellation once, using dispatch-time state.

### Duplicated pointer authority

`ApplicationHost` owns:

- `pressed_target`
- `active_pointer`
- `hover_path`

`UiDomState` separately owns:

- `pointer_active`
- `pointer_capture`
- `announced_pointer_capture`

These fields describe one interaction but can advance independently. Keep pointer lifecycle and capture state in one authoritative component. The host should retain only native position/buttons and the presented layout needed for hit testing.

### Preconstructed event batches

`NativeMouseEvent` batches use string event types and optional fields such as `wheel_delta` and `pointer_id`. The host constructs derived events before JavaScript runs, allowing queued events to contain stale capture and hover decisions.

Queue one small native-input record and construct JavaScript events during dispatch.

### Test volume is aimed at payloads instead of transitions

The large event tests in `crates/burokku/src/ui/host.rs:1882-2201` repeat many payload assertions while missing the sequences that currently fail.

Prefer compact state-table tests for:

1. down → capture → move before queue drain;
2. down → capture → resize → release;
3. down → queue full → cancel;
4. left down → right down/up → left up;
5. captured release over another target.

### Trivial cleanup

`crates/burokku/src/ui/dom_plugin.rs:150-151` contains duplicate `#[cfg(test)]` attributes.

Estimated simplification: roughly 200 production lines after consolidating pointer dispatch.

## Real issues to fix now

### P1: Pointer capture races queued JavaScript dispatch

Relevant code:

- `crates/burokku/src/ui/host.rs:529-540`
- `crates/burokku/src/ui/host.rs:703-724`
- `crates/burokku/src/ui/dom_plugin/classes.rs:1014-1019`

The host samples capture before a queued `pointerdown` handler can establish it. A following native movement can therefore be dropped or can enqueue `pointerleave`/`pointerenter` events that fire after capture is active.

Add a test that queues down and movement outside the hit region without awaiting between them. Capture and boundary decisions must use dispatch-time state.

### P1: Terminal input loss leaves pointer capture active

Relevant code:

- `crates/burokku/src/ui/host.rs:523-527`
- `crates/burokku/src/ui/host.rs:551-588`
- `crates/burokku/src/ui/host.rs:662-686`
- `crates/burokku/src/ui/host.rs:1321-1328`
- `crates/burokku/src/ui/dom_plugin/classes.rs:1023-1029`

A release during resize or another period without a usable presented frame clears host state without dispatching `pointerup` or `pointercancel`. A full bounded task queue can similarly reject cancellation after `active_pointer` has already been consumed.

The DOM then retains `pointer_active` and pointer capture indefinitely. Critical release/cancel transitions must be retained, retried, or converted into a guaranteed cancellation. Host bookkeeping must not commit until enqueue succeeds.

### P1: Chorded mouse-button transitions disappear

Relevant code:

- `crates/burokku/src/ui/host.rs:113-166`
- `crates/burokku/src/ui/host.rs:2044-2045`

Pressing or releasing another button while one remains held suppresses `pointerdown`/`pointerup`, but no replacement event is emitted. Applications cannot observe the changed `button` or `buttons` state.

Emit `pointermove` for chorded button transitions and update the existing test, which currently expects no event.

### P1: Control-modified keys are mistranslated

Relevant code:

- `crates/winit/src/platform/macos.rs:582-589`
- `crates/burokku/src/ui/host.rs:202-220`

macOS forwards post-modifier `characters()`. Control+C can therefore provide U+0003, which is mapped to `"Enter"`; Control+H can become `"Backspace"`.

Carry modifier-independent logical key information separately from text input.

### P1: Command-modified key releases are missing on macOS

Relevant code:

- `crates/winit/src/platform/macos.rs:374-382`

AppKit normally does not deliver Command-modified key releases through the view's ordinary `keyUp:` path. Upstream winit handles this at `NSApplication.sendEvent:`.

Add application-level forwarding and an integration test for Command+letter down/up.

### P1: Left/right modifier releases can become key presses

Relevant code:

- `crates/winit/src/platform/macos.rs:384-396`
- `crates/winit/src/platform/macos.rs:654-667`

Modifier state is inferred from aggregate flags. If both Shift keys are held and one is released, the aggregate Shift flag remains set, so the released key is reported as `Pressed`.

Track pressed modifier key codes and derive each transition from that set.

## Contract decisions

These are not immediate correctness blockers if Burokku intentionally defines a smaller, non-browser-compatible event contract. They should be settled before the API is frozen.

### Pointer `button` value

Relevant code:

- `crates/burokku/src/ui/dom_plugin.rs:28`
- `crates/burokku/src/ui/host.rs:139-143`
- `packages/runtime/src/index.ts:20`

Ordinary movement and hover report `button = 0`. Pointer Events uses `-1` when no button changed, and the current unsigned Rust field cannot represent it.

### Click targeting

Relevant code:

- `crates/burokku/src/ui/host.rs:83-101`
- `crates/burokku/src/ui/host.rs:113-165`
- `crates/burokku/src/ui/dom_plugin/classes.rs:1001-1007`

Current click recognition requires identical raw hit targets and does not account for pointer capture. Browser Pointer Events instead target active capture, or otherwise the nearest common inclusive ancestor of the down/up targets.

Even without full browser compatibility, the current behavior should be documented because captured `pointerup` and `click` can disagree.

### Capture loss after disconnection

Relevant code:

- `crates/burokku/src/ui/dom_plugin/classes.rs:1042-1074`
- `crates/burokku/src/ui/dom_plugin/classes.rs:1089-1091`

Capture is cleared when its target disconnects, but the resulting `lostpointercapture` is dispatched to the disconnected target and dropped. A browser-compatible implementation would notify the connected document/app fallback.

### Event surface

The current contract intentionally omits compatibility mouse events. Also decide whether it should eventually include:

- `pointerover` and `pointerout`;
- `auxclick` and `contextmenu`;
- pointer modifier fields;
- richer keyboard fields such as logical `code` and location.

Absence alone is not a bug while the pointer-only subset is explicitly documented.

## Missing validation

`crates/winit/tests/external_wake_macos.rs` exercises mouse movement/buttons and ordinary key selectors, but not `scrollWheel:` or `flagsChanged:`. Add selector-level tests for wheel conversion and modifier chords.

The repository's `pnpm --filter './example/*'` commands currently match no projects. The counter JavaScript is still exercised by the Rust test `llrt_counter_example_updates_from_an_interval`, but the root pnpm scripts give a misleading impression that examples were built.

## Complexity worth keeping

The following code addresses real ownership and event-dispatch requirements and should not be removed merely to reduce line count:

- WeakRef and listener wrapper rooting;
- propagation-path snapshots;
- checks for listeners removed during dispatch;
- listener exception isolation;
- the bounded JavaScript task queue;
- typed TypeScript event maps;
- native macOS selector implementations.

## Validation performed

The reviewed branch passed:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test --workspace`;
- runtime TypeScript build and typecheck;
- `git diff --check main...HEAD`.

Passing tests do not cover the transition and queue-ordering failures listed above.
