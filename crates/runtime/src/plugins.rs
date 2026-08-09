//! Standard plugins shipped with the runtime.

mod console;
mod json;
mod timers;

pub use console::ConsolePlugin;
pub use json::JsonPlugin;
pub use timers::TimersPlugin;
