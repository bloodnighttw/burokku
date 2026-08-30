//! Platform backend selection.
//!
//! Public modules depend only on the types re-exported here. A Linux or
//! Windows backend can therefore be added beside `macos` without introducing
//! target-specific conditionals throughout the crate.

use std::time::Instant;

/// Result of one platform-owned main-loop callback.
pub(crate) struct PlatformTick {
    pub(crate) next_deadline: Option<Instant>,
    pub(crate) exit: bool,
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod unsupported;

#[cfg(target_os = "macos")]
pub(crate) use macos::{PlatformEventLoop, PlatformWake, PlatformWindow};
#[cfg(not(target_os = "macos"))]
pub(crate) use unsupported::{PlatformEventLoop, PlatformWake, PlatformWindow};
