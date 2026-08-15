use runtime::{rquickjs::Ctx, Plugin, Result as RuntimeResult, RuntimeRole};

use super::elements::{BtsDom, SharedDom};

/// Owns BTS staging state and publishes it at the runtime checkpoint.
#[derive(Debug)]
pub struct DomPlugin {
    owner: BtsDom,
}

impl DomPlugin {
    pub fn new(shared: SharedDom) -> Self {
        Self {
            owner: BtsDom::new(shared),
        }
    }

    pub fn with_new_dom() -> (Self, SharedDom) {
        let shared = SharedDom::new();
        (Self::new(shared.clone()), shared)
    }
}

impl Plugin for DomPlugin {
    fn install<'js>(&self, context: &Ctx<'js>) -> RuntimeResult<()> {
        if RuntimeRole::from_context(context) != Some(RuntimeRole::Background) {
            return Err(runtime::rquickjs::Error::Unknown);
        }
        Ok(())
    }

    fn checkpoint<'js>(&mut self, _context: &Ctx<'js>) -> RuntimeResult<()> {
        self.owner
            .checkpoint()
            .map_err(|_| runtime::rquickjs::Error::Unknown)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::elements::Elements;
    use runtime::Runtime;

    #[tokio::test(flavor = "current_thread")]
    async fn failed_javascript_task_commits_successful_dom_mutations() {
        let (mut dom_plugin, shared) = DomPlugin::with_new_dom();
        dom_plugin.owner.mutate().create(Elements::Div);
        let runtime = Runtime::builder()
            .role(RuntimeRole::Background)
            .plugin(dom_plugin)
            .build()
            .await
            .unwrap();

        assert!(runtime
            .eval::<()>("throw new Error('render failed')")
            .await
            .is_err());

        assert_eq!(shared.load().revision(), 1);
        assert_eq!(shared.load().dom().revision(), 1);
    }
}
