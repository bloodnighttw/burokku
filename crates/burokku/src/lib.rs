#![forbid(unsafe_code)]

//! Burokku's native UI host and high-level application API.
//!
//! [`Burokku::builder`] composes the native event loop with a configurable
//! [`DualRuntimeBuilder`]. Application-facing plugins are installed in BTS with
//! [`BurokkuBuilder::runtime_plugin`], while latency-sensitive plugins can be
//! installed in MTS with [`BurokkuBuilder::main_runtime_plugin`].

mod app;
#[cfg(debug_assertions)]
pub mod debug;
pub mod ui;

pub use app::{Burokku, BurokkuBuilder, Error, Result, RunMode};
pub use runtime;
pub use runtime::{DualRuntime, DualRuntimeBuilder, Plugin};
pub use ui::js_bridge::DomPlugin as RuntimePlugin;
