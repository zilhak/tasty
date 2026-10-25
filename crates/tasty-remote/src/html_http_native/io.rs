//! Recheck retirement at each underlying I/O poll, including partial writes and Pending repolls.
use super::{NativeError, Result};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
type Current<'a> = dyn Fn() -> bool + Send + Sync + 'a;
/// Borrowed guard for each underlying poll, including owned native TLS adapters.
pub struct CurrentIo<'a, S> {
    inner: &'a mut S,
    current: &'a Current<'a>,
}
impl<'a, S> CurrentIo<'a, S> {
    /// The predicate must be pure and must not reenter the guarded I/O.
    pub fn new(inner: &'a mut S, current: &'a Current<'a>) -> Self {
        Self { inner, current }
    }
}
fn closed() -> io::Error {
    io::ErrorKind::ConnectionAborted.into()
}
impl<S: AsyncRead + Unpin> AsyncRead for CurrentIo<'_, S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if !(this.current)() {
            return Poll::Ready(Err(closed()));
        }
        Pin::new(&mut *this.inner).poll_read(cx, buf)
    }
}
impl<S: AsyncWrite + Unpin> AsyncWrite for CurrentIo<'_, S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if !(this.current)() {
            return Poll::Ready(Err(closed()));
        }
        Pin::new(&mut *this.inner).poll_write(cx, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if !(this.current)() {
            return Poll::Ready(Err(closed()));
        }
        Pin::new(&mut *this.inner).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if !(this.current)() {
            return Poll::Ready(Err(closed()));
        }
        Pin::new(&mut *this.inner).poll_shutdown(cx)
    }
}
pub(super) fn finish<T>(result: Result<T>, current: &Current<'_>) -> Result<T> {
    if current() {
        result
    } else {
        Err(NativeError::Closed)
    }
}
