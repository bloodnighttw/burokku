//! JavaScript wrapper identity and DOM node lifetime ownership rules.
//!
//! 1. The app tree retains every connected descendant, even without JavaScript wrappers.
//! 2. A live canonical JavaScript wrapper retains its entire detached component so parent
//!    and sibling traversal remains valid.
//! 3. `NodeId` values held by layout, rendering, or other derived state are non-owning.
//! 4. Dropping the final wrapper only makes a detached component eligible for reclamation;
//!    cleanup happens after QuickJS GC at the next host maintenance point (in about_to_wait).
//! 5. Every use of a retained `NodeId` must validate its generation because reclamation can
//!    make non-owning handles stale.

use std::{cell::RefCell, collections::HashSet, rc::Rc};

use rquickjs::{
    class::Trace, object::Property, prelude::This, Class, Constructor, Ctx, Function, JsLifetime,
    Object, Result,
};
use slotmap::Key;

use super::{
    borrow, borrow_mut, errors, events::listeners::ListenerRegistry, node::NativeNode,
    style::NativeStyleDeclaration, DomBindingState, SharedDomBindings,
};
use crate::ui::elements::{DomError, ElementTag, NodeId, NodeKind, ReclaimReport};

const WRAPPER_CACHE: &str = "__burokkuWrapperCache";

pub(super) type SharedWrapperRoots = Rc<RefCell<WrapperRoots>>;

#[derive(Debug, Default)]
pub(super) struct WrapperRoots {
    nodes: HashSet<NodeId>,
    released: Vec<NodeId>,
}

pub(super) struct WrapperRoot {
    id: NodeId,
    roots: SharedWrapperRoots,
}

impl Drop for WrapperRoot {
    fn drop(&mut self) {
        self.roots.borrow_mut().release(self.id);
    }
}

#[derive(Trace, JsLifetime)]
struct WrapperEntry<'js> {
    #[qjs(skip_trace)]
    id: NodeId,
    reference: Object<'js>,
}

#[derive(Trace, JsLifetime)]
struct ListenerRoot<'js> {
    #[qjs(skip_trace)]
    id: NodeId,
    wrapper: Object<'js>,
}

#[derive(Trace, JsLifetime)]
#[rquickjs::class]
struct WrapperCache<'js> {
    // ponytail: linear lookup; add a traced index only if large DOMs make this measurable.
    entries: Vec<WrapperEntry<'js>>,
    listener_roots: Vec<ListenerRoot<'js>>,
    weak_ref: Constructor<'js>,
    weak_ref_deref: Function<'js>,
}

impl<'js> WrapperCache<'js> {
    fn reference(&self, id: NodeId) -> Option<Object<'js>> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.reference.clone())
    }

    fn remove(&mut self, id: NodeId) {
        self.entries.retain(|entry| entry.id != id);
        self.listener_roots.retain(|entry| entry.id != id);
    }
}

pub(super) fn install<'js>(context: &Ctx<'js>) -> Result<()> {
    let weak_ref: Constructor = context.globals().get("WeakRef")?;
    let weak_ref_deref: Function = weak_ref.get::<_, Object>("prototype")?.get("deref")?;
    let node_methods = Class::<NativeNode<'js>>::prototype(context)?
        .expect("macro-backed Node class has a prototype");
    node_methods.prop(
        WRAPPER_CACHE,
        Class::instance(
            context.clone(),
            WrapperCache {
                entries: Vec::new(),
                listener_roots: Vec::new(),
                weak_ref,
                weak_ref_deref,
            },
        )?,
    )?;
    Ok(())
}

fn wrapper_cache<'js>(context: &Ctx<'js>) -> Result<Class<'js, WrapperCache<'js>>> {
    Class::<NativeNode<'js>>::prototype(context)?
        .expect("macro-backed Node class has a prototype")
        .get(WRAPPER_CACHE)
}

pub(super) fn sync_connected_listener_roots<'js>(
    context: &Ctx<'js>,
    state: &SharedDomBindings,
) -> Result<()> {
    // ponytail: linear scan; index listener-bearing wrappers only if mutations make this measurable.
    // ponytail: detached descendants survive only while their wrappers are live; root detached
    // component groups if browser-compatible subtree retention becomes necessary.
    let cache = wrapper_cache(context)?;
    borrow_mut(context, state)?.clear_disconnected_pointer_capture();
    let (candidates, deref) = {
        let cache = cache.borrow();
        (
            cache
                .entries
                .iter()
                .map(|entry| (entry.id, entry.reference.clone()))
                .collect::<Vec<_>>(),
            cache.weak_ref_deref.clone(),
        )
    };
    let candidates: Vec<_> = {
        let state = borrow(context, state)?;
        candidates
            .into_iter()
            .filter(|(id, _)| matches!(state.dom.is_connected(*id), Ok(true)))
            .collect()
    };

    let mut roots = Vec::new();
    for (id, reference) in candidates {
        let Some(wrapper) = deref.call::<_, Option<Object>>((This(reference),))? else {
            continue;
        };
        let node =
            Class::<NativeNode>::from_object(&wrapper).expect("wrapped nodes use NativeNode");
        if !node.borrow().listeners.is_empty() {
            roots.push(ListenerRoot { id, wrapper });
        }
    }
    cache.borrow_mut().listener_roots = roots;
    Ok(())
}

fn cached_wrapper<'js>(
    cache: &Class<'js, WrapperCache<'js>>,
    id: NodeId,
) -> Result<Option<Object<'js>>> {
    let (reference, deref) = {
        let cache = cache.borrow();
        (cache.reference(id), cache.weak_ref_deref.clone())
    };
    let Some(reference) = reference else {
        return Ok(None);
    };
    deref.call((This(reference),))
}

fn cache_wrapper<'js>(
    cache: &Class<'js, WrapperCache<'js>>,
    id: NodeId,
    wrapper: &Object<'js>,
) -> Result<()> {
    let weak_ref = cache.borrow().weak_ref.clone();
    let reference = weak_ref.construct((wrapper.clone(),))?;
    cache
        .borrow_mut()
        .entries
        .push(WrapperEntry { id, reference });
    Ok(())
}

pub(super) fn wrap_node<'js>(
    context: &Ctx<'js>,
    state: &SharedDomBindings,
    id: NodeId,
) -> Result<Object<'js>> {
    let cache = wrapper_cache(context)?;
    let released = borrow(context, state)?.take_released_wrappers();
    if !released.is_empty() {
        let mut cache = cache.borrow_mut();
        for id in released {
            cache.remove(id);
        }
    }
    if let Some(cached) = cached_wrapper(&cache, id)? {
        return Ok(cached);
    }

    let (constructor_name, is_element) = {
        let state = borrow(context, state)?;
        let kind = state.dom.kind(id).ok_or(DomError::NodeNotFound(id));
        let kind = errors::map_dom(context, "wrap node", kind)?;
        match kind {
            NodeKind::App => ("AppNode", false),
            NodeKind::Text(_) => ("TextNode", false),
            NodeKind::Element(element) => match element.tag() {
                ElementTag::Window => ("Window", true),
                ElementTag::Div => ("Div", true),
                ElementTag::Flex => ("Flex", true),
                ElementTag::Grid => ("Grid", true),
                ElementTag::Text => ("TextElement", true),
            },
        }
    };
    let constructor: Object = context.globals().get(constructor_name)?;
    let prototype: Object = constructor.get("prototype")?;
    let style_prototype = if is_element {
        let constructor: Object = context.globals().get("BurokkuStyleDeclaration")?;
        Some(constructor.get("prototype")?)
    } else {
        None
    };
    let wrapper_root = {
        let state = borrow(context, state)?;
        errors::map_dom(context, "acquire node wrapper", state.acquire_wrapper(id))?
    };
    let node = Class::instance_proto(
        NativeNode {
            state: state.clone(),
            id,
            _wrapper_root: wrapper_root,
            listeners: ListenerRegistry::default(),
        },
        prototype,
    )?;

    if let Some(prototype) = style_prototype {
        let style = Class::instance_proto(
            NativeStyleDeclaration {
                state: state.clone(),
                id,
            },
            prototype,
        )?;
        node.prop("style", Property::from(style))?;
    }

    cache_wrapper(
        &cache,
        id,
        node.as_value()
            .as_object()
            .expect("class instance is an object"),
    )?;
    Ok(node.into_inner())
}

impl WrapperRoots {
    fn acquire(&mut self, id: NodeId) {
        assert!(
            self.nodes.insert(id),
            "canonical NodeId wrappers cannot overlap"
        );
    }

    pub(super) fn release(&mut self, id: NodeId) {
        let removed = self.nodes.remove(&id);
        debug_assert!(removed, "wrapper root must be registered");
        if removed {
            self.released.push(id);
        }
    }
}

pub(super) fn encode_node_id(id: NodeId) -> String {
    format!("{:016x}", id.data().as_ffi())
}

impl DomBindingState {
    pub(super) fn acquire_wrapper(&self, id: NodeId) -> std::result::Result<WrapperRoot, DomError> {
        self.dom
            .contains(id)
            .then_some(())
            .ok_or(DomError::NodeNotFound(id))?;
        self.wrapper_roots.borrow_mut().acquire(id);
        Ok(WrapperRoot {
            id,
            roots: Rc::clone(&self.wrapper_roots),
        })
    }

    pub(super) fn take_released_wrappers(&self) -> Vec<NodeId> {
        std::mem::take(&mut self.wrapper_roots.borrow_mut().released)
    }

    pub(crate) fn reclaim_detached(&mut self) -> runtime::Result<ReclaimReport> {
        let live = self
            .wrapper_roots
            .borrow()
            .nodes
            .iter()
            .copied()
            .collect::<Vec<_>>();
        self.dom
            .reclaim_unreachable_detached(live)
            .map_err(|error| {
                runtime::Error::new_from_js_message(
                    "DOM wrapper roots",
                    "live NodeId values",
                    error.to_string(),
                )
            })
    }

    #[cfg(test)]
    pub(super) fn live_wrapper_count(&self) -> usize {
        self.wrapper_roots.borrow().nodes.len()
    }

    #[cfg(test)]
    pub(super) fn has_wrapper(&self, id: NodeId) -> bool {
        self.wrapper_roots.borrow().nodes.contains(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_tokens_preserve_full_node_id_precision() {
        let mut dom = crate::ui::elements::Dom::new();
        let token = encode_node_id(dom.create_text("token"));

        assert_eq!(token.len(), 16);
        assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}
