//! Connect the neutral owned-command port to the existing process file writer.
use super::{FileError, FileExecutor, FileOutput, FileRequest};
use crate::file_command::{self, Port, Receipt};
pub(crate) use crate::file_command::{
    Command, CommandDisposition, CommandOutcome, Failure, Output, Ticket, value_bytes,
};
use tasty_utils::batch_writer::{Completion, Handle, RequestReceipt};

impl Port for Handle<FileExecutor> {
    fn submit(&self, command: Command) -> Result<Box<dyn Receipt>, Failure> {
        Handle::submit(self, FileRequest::Command(command))
            .map(|receipt| Box::new(WriterReceipt(receipt)) as Box<dyn Receipt>)
            .map_err(|error| failure(error.to_string(), false))
    }
}
struct WriterReceipt(RequestReceipt<FileExecutor>);
impl Receipt for WriterReceipt {
    fn poll(&self) -> Option<Result<Output, Failure>> {
        self.0.try_take().map(convert)
    }
}
fn failure(message: impl Into<String>, unknown: bool) -> Failure {
    Failure {
        message: message.into(),
        unknown,
    }
}
fn convert(completion: Completion<FileOutput, FileError>) -> Result<Output, Failure> {
    match completion {
        Completion::Done(Ok(FileOutput::Command(output))) => Ok(output),
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

// Legacy small App fixtures alone retain immediate execution. Injected neutral ports never
// fall back to synchronous work, even in tests. Production always requires the actual writer.
#[cfg(test)]
struct Immediate;
#[cfg(test)]
impl Port for Immediate {
    fn submit(&self, mut command: Command) -> Result<Box<dyn Receipt>, Failure> {
        struct Ready(std::sync::Mutex<Option<Result<Output, Failure>>>);
        impl Receipt for Ready {
            fn poll(&self) -> Option<Result<Output, Failure>> {
                self.0.lock().unwrap().take()
            }
        }
        Ok(Box::new(Ready(std::sync::Mutex::new(Some(
            command.execute(),
        )))))
    }
}
fn port(queue: Option<&Handle<FileExecutor>>) -> Option<&dyn Port> {
    #[cfg(test)]
    if queue.is_none() {
        return Some(&Immediate);
    }
    queue.map(|queue| queue as &dyn Port)
}
pub(crate) fn submit<T: Send + 'static>(
    queue: Option<&Handle<FileExecutor>>,
    bytes: usize,
    work: impl FnOnce() -> T + Send + 'static,
    weigh: impl FnOnce(&T) -> usize + Send + 'static,
) -> Result<Ticket<T>, Failure> {
    file_command::submit(port(queue), bytes, work, weigh)
}
pub(crate) fn submit_classified<T: Send + 'static>(
    queue: Option<&Handle<FileExecutor>>,
    bytes: usize,
    work: impl FnOnce() -> T + Send + 'static,
    weigh: impl FnOnce(&T) -> usize + Send + 'static,
    classify: impl FnOnce(&T) -> CommandDisposition + Send + 'static,
) -> Result<Ticket<T>, Failure> {
    file_command::submit_classified(port(queue), bytes, work, weigh, classify)
}
