//! Standard plugins shipped with the runtime.

mod console;
mod timers;
mod window_events;

pub use console::ConsolePlugin;
pub use timers::TimersPlugin;
pub use window_events::{
    InputState, ModifiersState, MouseButton, WindowEventMessage, WindowEventsPlugin,
};

pub(crate) use window_events::enqueue;
