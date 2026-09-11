//! JavaScript-facing style declaration bindings.

use rquickjs::{class::Trace, Coerced, Ctx, JsLifetime, Result};

use super::{borrow, borrow_mut, errors, SharedDomBindings};
use crate::ui::elements::NodeId;

#[derive(Trace, JsLifetime)]
#[rquickjs::class(rename = "NativeStyleDeclaration", frozen)]
pub(super) struct NativeStyleDeclaration {
    #[qjs(skip_trace)]
    pub(super) state: SharedDomBindings,
    #[qjs(skip_trace)]
    pub(super) id: NodeId,
}

#[rquickjs::methods]
impl NativeStyleDeclaration {
    #[qjs(rename = "supportsProperty")]
    fn supports_property(&self, context: Ctx<'_>, name: Coerced<String>) -> Result<bool> {
        let result = borrow(&context, &self.state)?
            .dom
            .supports_style_property(self.id, &name.0);
        errors::map_dom(&context, "check style property", result)
    }

    #[qjs(rename = "setProperty")]
    fn set_property(
        &self,
        context: Ctx<'_>,
        name: Coerced<String>,
        value: Coerced<String>,
    ) -> Result<()> {
        let result = borrow_mut(&context, &self.state)?
            .dom
            .set_style_property(self.id, &name.0, &value.0);
        errors::map_style(&context, "set style property", result).map(|_| ())
    }

    #[qjs(rename = "removeProperty")]
    fn remove_property(&self, context: Ctx<'_>, name: Coerced<String>) -> Result<()> {
        let result = borrow_mut(&context, &self.state)?
            .dom
            .remove_style_property(self.id, &name.0);
        errors::map_style(&context, "remove style property", result).map(|_| ())
    }
}
