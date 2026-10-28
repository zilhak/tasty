//! Owned streams reuse the same per-poll guard as the finite native HTTP codec.
use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tasty_remote::html_http_native::CurrentIo;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
pub(super) type Current = Arc<dyn Fn() -> bool + Send + Sync>;
pub(super) struct CurrentConnection<S> {
    inner: S,
    current: Current,
}
impl<S> CurrentConnection<S> {
    pub(super) fn new(inner: S, current: Current) -> Self {
        Self { inner, current }
    }
}
impl<S: AsyncRead + Unpin> AsyncRead for CurrentConnection<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut CurrentIo::new(&mut this.inner, this.current.as_ref())).poll_read(cx, buf)
    }
}
impl<S: AsyncWrite + Unpin> AsyncWrite for CurrentConnection<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        Pin::new(&mut CurrentIo::new(&mut this.inner, this.current.as_ref())).poll_write(cx, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut CurrentIo::new(&mut this.inner, this.current.as_ref())).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut CurrentIo::new(&mut this.inner, this.current.as_ref())).poll_shutdown(cx)
    }
}

impl CurrentConnection<tokio::net::TcpStream> {
    pub(super) async fn first_byte(&self) -> io::Result<u8> {
        let mut byte = [0];
        let count = std::future::poll_fn(|cx| {
            if !(self.current)() {
                return Poll::Ready(Err(io::ErrorKind::ConnectionAborted.into()));
            }
            self.inner.poll_peek(cx, &mut ReadBuf::new(&mut byte))
        })
        .await?;
        if count == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        Ok(byte[0])
    }
}

/// Replays an already consumed request head before the remaining stream bytes.
pub(super) struct Prefixed<S> {
    head: Vec<u8>,
    offset: usize,
    inner: S,
}
impl<S> Prefixed<S> {
    pub(super) fn new(head: Vec<u8>, inner: S) -> Self {
        Self {
            head,
            offset: 0,
            inner,
        }
    }
}
impl<S: AsyncRead + Unpin> AsyncRead for Prefixed<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.offset < this.head.len() {
            let n = buf.remaining().min(this.head.len() - this.offset);
            buf.put_slice(&this.head[this.offset..this.offset + n]);
            this.offset += n;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}
impl<S: AsyncWrite + Unpin> AsyncWrite for Prefixed<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}
