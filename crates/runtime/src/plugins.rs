//! Standard plugins shipped with the runtime.

mod console;
mod json;
mod timers;
mod window_events;

pub use console::ConsolePlugin;
pub use json::JsonPlugin;
pub use timers::TimersPlugin;
pub use window_events::{
    InputState, ModifiersState, MouseButton, WindowEventMessage, WindowEventsPlugin,
};
