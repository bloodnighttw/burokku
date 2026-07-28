//! Platform backend selection.
//!
//! Public modules depend only on the two types re-exported here. A Linux or
//! Windows backend can therefore be added beside `macos` without introducing
//! target-specific conditionals throughout the crate.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod unsupported;

#[cfg(target_os = "macos")]
pub(crate) use macos::{PlatformEventLoop, PlatformWindow};
#[cfg(not(target_os = "macos"))]
pub(crate) use unsupported::{PlatformEventLoop, PlatformWindow};
