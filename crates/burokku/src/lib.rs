#![deny(unsafe_code)]

//! Burokku's native UI host and high-level application API.
//!
//! [`Burokku::builder`] composes the native event loop with a configurable
//! [`DualRuntimeBuilder`]. Application-facing plugins are installed in BTS with
//! [`BurokkuBuilder::runtime_plugin`], while latency-sensitive plugins can be
//! installed in MTS with [`BurokkuBuilder::main_runtime_plugin`].

mod app;
pub mod ui;

pub use app::{Burokku, BurokkuBuilder, BurokkuError};
pub use runtime;
pub use runtime::{DualRuntime, DualRuntimeBuilder, Plugin};
