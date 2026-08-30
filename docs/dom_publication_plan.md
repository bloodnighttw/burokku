# Superseded: DOM Publication Plan

The immutable snapshot and `ArcSwap` publication design has been removed. The application runtime, DOM, layout, and scene construction now share the UI thread and use controlled `Rc<RefCell<UiDomState>>` borrowing.

See [`dom_layout_colocation_plan.md`](dom_layout_colocation_plan.md) for the implemented architecture and migration record.
