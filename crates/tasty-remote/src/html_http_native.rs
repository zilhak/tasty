//! Finite native HTTP codec. It performs no DNS, connect, upstream request or browser trust change.
mod budget;
mod io;
mod request;
mod response;
pub use budget::NativeBudget;
pub use request::{Destination, NativeRequest, read_request};
pub use response::write_response;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeError {
    Invalid,
    Destination,
    Upgrade,
    Quota,
    Closed,
    Timeout,
    Io,
    Identity,
}
pub type Result<T> = std::result::Result<T, NativeError>;
#[derive(Clone, Copy)]
pub struct NativeLimits {
    pub upload_bytes: usize,
    pub io_timeout: std::time::Duration,
}

#[cfg(test)]
mod tests;
