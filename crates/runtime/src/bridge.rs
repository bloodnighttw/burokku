//! Bounded, typed channels for communication between isolated runtimes.

use tokio::sync::mpsc;

/// One side of a bidirectional bridge between two JavaScript runtimes.
///
/// Messages are owned Rust values; JavaScript values must be converted before
/// they cross an isolate boundary. The channel is bounded so high-frequency UI
/// producers cannot grow memory without limit.
pub struct BridgeEndpoint<Outgoing, Incoming> {
    sender: mpsc::Sender<Outgoing>,
    receiver: mpsc::Receiver<Incoming>,
}

impl<Outgoing, Incoming> BridgeEndpoint<Outgoing, Incoming> {
    /// Clone the sending half for installation into host callbacks or plugins.
    pub fn sender(&self) -> mpsc::Sender<Outgoing> {
        self.sender.clone()
    }

    /// Send one message, waiting for bounded capacity when necessary.
    pub async fn send(
        &self,
        message: Outgoing,
    ) -> std::result::Result<(), mpsc::error::SendError<Outgoing>> {
        self.sender.send(message).await
    }

    /// Attempt to send one message without waiting.
    pub fn try_send(
        &self,
        message: Outgoing,
    ) -> std::result::Result<(), mpsc::error::TrySendError<Outgoing>> {
        self.sender.try_send(message)
    }

    /// Receive the next message from the other runtime.
    pub async fn recv(&mut self) -> Option<Incoming> {
        self.receiver.recv().await
    }
}

impl<Outgoing, Incoming> std::fmt::Debug for BridgeEndpoint<Outgoing, Incoming> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BridgeEndpoint")
            .finish_non_exhaustive()
    }
}

/// Create a bidirectional, bounded bridge.
///
/// The first endpoint is conventionally installed on the main runtime and the
/// second on the background runtime.
pub fn bridge_channel<MainToBackground, BackgroundToMain>(
    capacity: usize,
) -> (
    BridgeEndpoint<MainToBackground, BackgroundToMain>,
    BridgeEndpoint<BackgroundToMain, MainToBackground>,
) {
    assert!(capacity > 0, "runtime bridge capacity must be non-zero");
    let (main_sender, background_receiver) = mpsc::channel(capacity);
    let (background_sender, main_receiver) = mpsc::channel(capacity);

    (
        BridgeEndpoint {
            sender: main_sender,
            receiver: main_receiver,
        },
        BridgeEndpoint {
            sender: background_sender,
            receiver: background_receiver,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn routes_typed_messages_in_both_directions() {
        let (mut main, mut background) = bridge_channel::<String, u32>(2);

        main.send("mutation".into()).await.unwrap();
        assert_eq!(background.recv().await.as_deref(), Some("mutation"));

        background.send(42).await.unwrap();
        assert_eq!(main.recv().await, Some(42));
    }

    #[tokio::test]
    async fn applies_backpressure_at_the_configured_capacity() {
        let (main, _background) = bridge_channel::<u32, ()>(1);

        main.try_send(1).unwrap();
        assert!(matches!(
            main.try_send(2),
            Err(mpsc::error::TrySendError::Full(2))
        ));
    }
}
