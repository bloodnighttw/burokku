use slotmap::{Key, KeyData};

use super::DomPluginState;
use crate::ui::elements::{DomError, NodeId};

pub(super) fn encode_node_id(id: NodeId) -> String {
    format!("{:016x}", id.data().as_ffi())
}

pub(super) fn decode_node_id(token: &str) -> Result<NodeId, InvalidNodeToken> {
    if token.len() != 16 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(InvalidNodeToken);
    }
    let raw = u64::from_str_radix(token, 16).map_err(|_| InvalidNodeToken)?;
    Ok(NodeId::from(KeyData::from_ffi(raw)))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct InvalidNodeToken;

impl DomPluginState {

    // insert the dom wrapper reference
    pub(super) fn acquire_wrapper(&mut self, id: NodeId) -> Result<(), DomError> {
        self.staging
            .contains(id)
            .then_some(())
            .ok_or(DomError::NodeNotFound(id))?;
        let count = self.live_wrappers.entry(id).or_default();
        *count = count
            .checked_add(1)
            .expect("a native node cannot have usize::MAX live wrappers");
        Ok(())
    }

    // remove the dom wrapper reference
    pub(super) fn release_wrapper(&mut self, id: NodeId) {
        let Some(count) = self.live_wrappers.get_mut(&id) else {
            return;
        };
        if *count > 1 {
            *count -= 1;
        } else {
            self.live_wrappers.remove(&id);
        }
    }

    // remove detached nodes that has no live wrappers
    pub(super) fn reclaim_detached(&mut self) -> runtime::Result<()> {
        let live = self.live_wrappers.keys().copied().collect::<Vec<_>>();
        self.last_reclaim = self
            .staging
            .reclaim_unreachable_detached(live)
            .map_err(|error| {
                runtime::Error::new_from_js_message(
                    "DOM wrapper roots",
                    "live NodeId values",
                    error.to_string(),
                )
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_tokens_round_trip_without_number_precision_loss() {
        let mut dom = crate::ui::elements::Dom::new();
        let id = dom.create_text("token");
        let token = encode_node_id(id);

        assert_eq!(token.len(), 16);
        assert_eq!(decode_node_id(&token), Ok(id));
        assert_eq!(decode_node_id("1"), Err(InvalidNodeToken));
        assert_eq!(decode_node_id("not-a-node-token"), Err(InvalidNodeToken));
    }
}
