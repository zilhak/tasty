//! Ordered owned operations. Payloads and results have no diagnostic representation.
use std::{any::Any, marker::PhantomData, sync::Arc};

/// Injected admission to the process file worker; this contract owns no worker or App.
pub(crate) trait Port: Send + Sync {
    fn submit(&self, command: Command) -> Result<Box<dyn Receipt>, Failure>;
}
/// Nonblocking, consuming completion. Implementations retain the worker receipt and its budget.
pub(crate) trait Receipt: Send + Sync {
    fn poll(&self) -> Option<Result<Output, Failure>>;
}
pub(crate) type Queue = Arc<dyn Port>;

pub(crate) fn value_bytes<T: serde::Serialize>(value: &T) -> usize {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(std::mem::size_of::<T>());
    match serde_json::to_writer(&mut count, value) {
        Ok(()) => count.0,
        Err(_) => usize::MAX,
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CommandOutcome {
    KnownSuccess,
    KnownFailure,
    Unknown,
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct CommandDisposition {
    pub(crate) outcome: CommandOutcome,
    pub(crate) partial: bool,
}

pub(crate) struct Command {
    pub(crate) bytes: usize,
    work: Option<Box<dyn FnOnce() -> Output + Send>>,
}
pub(crate) struct Output {
    value: Box<dyn Any + Send>,
    pub(crate) bytes: usize,
    pub(crate) disposition: Option<CommandDisposition>,
}
impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FileCommandOutput")
    }
}
impl Command {
    fn new<T: Send + 'static>(
        bytes: usize,
        work: impl FnOnce() -> T + Send + 'static,
        weigh: impl FnOnce(&T) -> usize + Send + 'static,
        classify: impl FnOnce(&T) -> Option<CommandDisposition> + Send + 'static,
    ) -> Self {
        Self {
            bytes,
            work: Some(Box::new(move || {
                let value = work();
                let bytes = weigh(&value).saturating_add(std::mem::size_of::<CommandDisposition>());
                let disposition = classify(&value);
                Output {
                    value: Box::new(value),
                    bytes,
                    disposition,
                }
            })),
        }
    }
    pub(crate) fn execute(&mut self) -> Result<Output, Failure> {
        self.work
            .take()
            .ok_or_else(|| failure("ordered file command already consumed", false))
            .map(|work| work())
    }
}
#[derive(Debug)]
pub(crate) struct Failure {
    pub(crate) message: String,
    pub(crate) unknown: bool,
}
pub(crate) struct Ticket<T> {
    receipt: Box<dyn Receipt>,
    kind: PhantomData<fn() -> T>,
}
impl<T: Send + 'static> Ticket<T> {
    pub(crate) fn poll(&self) -> Option<Result<T, Failure>> {
        self.receipt.poll().map(|result| result.and_then(value))
    }
}
fn failure(message: impl Into<String>, unknown: bool) -> Failure {
    Failure {
        message: message.into(),
        unknown,
    }
}
fn value<T: Send + 'static>(output: Output) -> Result<T, Failure> {
    output
        .value
        .downcast::<T>()
        .map(|value| *value)
        .map_err(|_| failure("unexpected ordered file output", true))
}
pub(crate) fn submit<T: Send + 'static>(
    queue: Option<&dyn Port>,
    bytes: usize,
    work: impl FnOnce() -> T + Send + 'static,
    weigh: impl FnOnce(&T) -> usize + Send + 'static,
) -> Result<Ticket<T>, Failure> {
    admit(queue, Command::new(bytes, work, weigh, |_| None))
}
/// The classifier describes backend success/partial state, not merely delivery of a typed output.
pub(crate) fn submit_classified<T: Send + 'static>(
    queue: Option<&dyn Port>,
    bytes: usize,
    work: impl FnOnce() -> T + Send + 'static,
    weigh: impl FnOnce(&T) -> usize + Send + 'static,
    classify: impl FnOnce(&T) -> CommandDisposition + Send + 'static,
) -> Result<Ticket<T>, Failure> {
    admit(
        queue,
        Command::new(bytes, work, weigh, move |value| Some(classify(value))),
    )
}
fn admit<T: Send + 'static>(
    queue: Option<&dyn Port>,
    command: Command,
) -> Result<Ticket<T>, Failure> {
    let queue =
        queue.ok_or_else(|| failure("file writer is unavailable; command not accepted", false))?;
    queue.submit(command).map(|receipt| Ticket {
        receipt,
        kind: PhantomData,
    })
}

#[cfg(test)]
mod tests;
