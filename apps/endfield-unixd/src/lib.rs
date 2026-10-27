#![forbid(unsafe_code)]

pub mod cli;
pub mod errors;
pub mod ipc_service;
pub mod lifecycle;
pub mod runtime;
pub mod session_service;
pub mod signals;

pub use runtime::{ShutdownPolicy, run_foreground, run_foreground_with_policy};
