//! Keep the certificate reservation through the actual TLS stream, including buffered I/O.
use super::super::io::{Current, CurrentConnection};
use super::*;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tasty_remote::html_http_native::CurrentIo;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
pub(crate) struct AuthorityStream<S> {
    stream: tokio_rustls::server::TlsStream<CurrentConnection<S>>,
    _certificate: Arc<AuthorityTls>,
    current: Current,
}
impl AuthorityTls {
    pub(crate) async fn accept<S: AsyncRead + AsyncWrite + Unpin>(
        self: &Arc<Self>,
        io: S,
        deadline: Duration,
    ) -> Result<AuthorityStream<S>, CertificateError> {
        let map = self.map.clone();
        let current: Current = Arc::new(move || map.upgrade().is_some_and(|map| map.current()));
        if !current() {
            return Err(CertificateError::Retired);
        }
        let result = self
            .tls
            .accept(CurrentConnection::new(io, current.clone()), deadline)
            .await;
        if !current() {
            return Err(CertificateError::Retired);
        }
        let stream = result.map_err(|_| CertificateError::Tls)?;
        Ok(AuthorityStream {
            stream,
            _certificate: self.clone(),
            current,
        })
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin> AuthorityStream<S> {
    pub(crate) fn server_name(&self) -> Option<&str> {
        self.stream.get_ref().1.server_name()
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for AuthorityStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut CurrentIo::new(&mut this.stream, this.current.as_ref())).poll_read(cx, buf)
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for AuthorityStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        Pin::new(&mut CurrentIo::new(&mut this.stream, this.current.as_ref())).poll_write(cx, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut CurrentIo::new(&mut this.stream, this.current.as_ref())).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut CurrentIo::new(&mut this.stream, this.current.as_ref())).poll_shutdown(cx)
    }
}
