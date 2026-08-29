#![deny(unsafe_code)]

//! Burokku's native UI host and high-level application API.
//!
//! [`Burokku::builder`] composes the native event loop with one configurable,
//! thread-affine [`RuntimeBuilder`]. Plugins are installed with
//! [`BurokkuBuilder::runtime_plugin`].

mod app;
pub mod ui;

pub use app::{Burokku, BurokkuBuilder, BurokkuError};
pub use runtime;
pub use runtime::{Plugin, RuntimeBuilder};
