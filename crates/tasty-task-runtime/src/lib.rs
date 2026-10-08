// reason: Test fixtures intentionally discard setup/cleanup results; production builds retain the must-use lint.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]
//! Task execution and completion ownership. TaskStore remains the sole durable task source.
//! Stopping a runner or dropping an engine scope neither cancels tasks nor kills running children.
pub mod agent_task;
pub mod agent_turns;
pub mod completion;
pub mod event_feed;
pub mod graph_view;
pub mod hook_wait;
mod report;
pub(crate) mod runner_host;
pub(crate) mod runner_thread;
mod service;
pub mod task;
pub mod task_output_ref;
pub mod task_waker;
pub use runner_thread::{RunnerRegistry, RunnerStatus, RunnerStopObservation, RunnerStopReceipt};
pub use service::{TaskAwaiter, TaskScope, TaskService};
