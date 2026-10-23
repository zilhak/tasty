//! Ordered owned operations. Payloads and results have no diagnostic representation.
use super::{FileError, FileExecutor, FileOutput, FileRequest};
use std::{any::Any, marker::PhantomData};
use tasty_utils::batch_writer::{Completion, Handle, RequestReceipt};

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

pub(crate) struct Command {
    pub(crate) bytes: usize,
    work: Option<Box<dyn FnOnce() -> Output + Send>>,
}
pub(crate) struct Output {
    value: Box<dyn Any + Send>,
    pub(crate) bytes: usize,
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
    ) -> Self {
        Self {
            bytes,
            work: Some(Box::new(move || {
                let value = work();
                let bytes = weigh(&value);
                Output {
                    value: Box::new(value),
                    bytes,
                }
            })),
        }
    }
    pub(super) fn execute(&mut self) -> Result<Output, FileError> {
        self.work
            .take()
            .ok_or_else(|| FileError("ordered file command already consumed".into()))
            .map(|work| work())
    }
}
#[derive(Debug)]
pub(crate) struct Failure {
    pub(crate) message: String,
    pub(crate) unknown: bool,
}
pub(crate) struct Ticket<T> {
    inner: Inner<T>,
}
enum Inner<T> {
    Writer(RequestReceipt<FileExecutor>, PhantomData<fn() -> T>),
    #[cfg(test)]
    Ready(std::sync::Mutex<Option<Result<T, Failure>>>),
}
impl<T: Send + 'static> Ticket<T> {
    pub(crate) fn poll(&self) -> Option<Result<T, Failure>> {
        match &self.inner {
            Inner::Writer(receipt, _) => receipt.try_take().map(convert),
            #[cfg(test)]
            Inner::Ready(result) => result.lock().unwrap().take(),
        }
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
fn convert<T: Send + 'static>(completion: Completion<FileOutput, FileError>) -> Result<T, Failure> {
    match completion {
        Completion::Done(Ok(FileOutput::Command(output))) => value(output),
        Completion::Done(Err(error)) | Completion::Failed(error) => {
            Err(failure(error.to_string(), false))
        }
        Completion::Unknown(error) => Err(failure(error.to_string(), true)),
        Completion::Lost => Err(failure(
            "file writer ended before command result; outcome unknown",
            true,
        )),
        Completion::Superseded { .. } => Err(failure(
            "ordered file command was unexpectedly superseded",
            true,
        )),
        Completion::Done(Ok(_)) => Err(failure("unrelated ordered file result", true)),
    }
}
pub(crate) fn submit<T: Send + 'static>(
    queue: Option<&Handle<FileExecutor>>,
    bytes: usize,
    work: impl FnOnce() -> T + Send + 'static,
    weigh: impl FnOnce(&T) -> usize + Send + 'static,
) -> Result<Ticket<T>, Failure> {
    let command = Command::new(bytes, work, weigh);
    if let Some(queue) = queue {
        return queue
            .submit(FileRequest::Command(command))
            .map(|receipt| Ticket {
                inner: Inner::Writer(receipt, PhantomData),
            })
            .map_err(|error| failure(error.to_string(), false));
    }
    // Only small existing unit fixtures omit the writer. Shipping configurations reject this
    // condition; production receipt regressions explicitly inject a real writer.
    #[cfg(test)]
    {
        let mut command = command;
        let result = command
            .execute()
            .map_err(|error| failure(error.to_string(), false))
            .and_then(value);
        Ok(Ticket {
            inner: Inner::Ready(std::sync::Mutex::new(Some(result))),
        })
    }
    #[cfg(not(test))]
    Err(failure(
        "file writer is unavailable; command not accepted",
        false,
    ))
}
