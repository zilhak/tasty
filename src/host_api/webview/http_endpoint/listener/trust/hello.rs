//! Inspect a bounded ClientHello without releasing a server certificate, then replay it.
use super::*;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
const MAX_HELLO: usize = 65_536;
struct Recording<S> {
    io: S,
    bytes: Vec<u8>,
}
pub(in super::super) struct Replay<S> {
    io: S,
    bytes: std::io::Cursor<Vec<u8>>,
    pub(super) flight: Option<std::sync::Weak<Slot>>,
}
pub(in super::super) async fn capture<S: AsyncRead + AsyncWrite + Unpin>(
    io: S,
    authority: &Authority,
    deadline: Duration,
) -> Result<Replay<S>, EndpointError> {
    let recording = Recording {
        io,
        bytes: Vec::new(),
    };
    let accepted = tokio::time::timeout(
        deadline,
        tokio_rustls::LazyConfigAcceptor::new(
            tokio_rustls::rustls::server::Acceptor::default(),
            recording,
        ),
    )
    .await
    .map_err(|_| EndpointError::Tls)?
    .map_err(|_| EndpointError::Tls)?;
    if !sni_matches(authority, accepted.client_hello().server_name()) {
        return Err(EndpointError::Destination);
    }
    let Recording { io, bytes } = accepted.io;
    Ok(Replay {
        io,
        bytes: std::io::Cursor::new(bytes),
        flight: None,
    })
}
impl<S: AsyncRead + Unpin> AsyncRead for Recording<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let available = MAX_HELLO - this.bytes.len();
        if available == 0 {
            return Poll::Ready(Err(io::ErrorKind::InvalidData.into()));
        }
        let mut bytes = [0; 4096];
        let count = buf.remaining().min(available).min(bytes.len());
        let mut read = ReadBuf::new(&mut bytes[..count]);
        match Pin::new(&mut this.io).poll_read(cx, &mut read) {
            Poll::Ready(Ok(())) => {
                this.bytes.extend_from_slice(read.filled());
                buf.put_slice(read.filled());
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}
impl<S: AsyncRead + Unpin> AsyncRead for Replay<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let position = this.bytes.position() as usize;
        if position < this.bytes.get_ref().len() {
            let count = buf.remaining().min(this.bytes.get_ref().len() - position);
            buf.put_slice(&this.bytes.get_ref()[position..position + count]);
            this.bytes.set_position((position + count) as u64);
            Poll::Ready(Ok(()))
        } else {
            Pin::new(&mut this.io).poll_read(cx, buf)
        }
    }
}
impl<S: AsyncWrite + Unpin> AsyncWrite for Recording<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().io).poll_write(cx, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_shutdown(cx)
    }
}
impl<S: AsyncWrite + Unpin> AsyncWrite for Replay<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.io).poll_write(cx, bytes);
        if let Poll::Ready(Ok(count)) = result
            && count > 0
            && let Some(slot) = this.flight.as_ref().and_then(std::sync::Weak::upgrade)
        {
            slot.flight.store(true, Ordering::Release);
        }
        result
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_shutdown(cx)
    }
}
