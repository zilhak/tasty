//! Task execution and completion ownership. TaskStore remains the sole durable task source.
//! Stopping a runner or dropping an engine scope neither cancels tasks nor kills running children.
pub mod completion;
pub mod event_feed;
pub mod graph_view;
pub mod hook_wait;
pub(crate) mod runner_host;
pub(crate) mod runner_thread;
pub mod task;
pub mod task_output_ref;
pub mod task_waker;
mod service;
pub use service::{TaskService,TaskScope,TaskAwaiter};
pub use runner_thread::{RunnerRegistry,RunnerStatus,RunnerStopReceipt,RunnerStopObservation};
