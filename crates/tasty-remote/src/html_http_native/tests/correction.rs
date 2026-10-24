//! Actual asynchronous socket writes exercise partial-ready and pending retirement boundaries.
use super::*;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
};
async fn sockets() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (client, server) = tokio::join!(TcpStream::connect(address), listener.accept());
    (client.unwrap(), server.unwrap().0)
}
fn metadata() -> usize {
    use tasty_ipc::html_http::{HEADER_BYTES, HEADER_COUNT, METHOD_BYTES, URL_BYTES};
    4 * HEADER_BYTES + HEADER_COUNT * std::mem::size_of::<Header>() + URL_BYTES + METHOD_BYTES
}
#[tokio::test]
async fn known_upload_capacity_precedes_any_continue_on_actual_socket() {
    let (mut peer, mut io) = sockets().await;
    let budget = NativeBudget::new(metadata());
    peer.write_all(b"POST / HTTP/1.1\r\nHost: example.test:8080\r\nExpect: 100-continue\r\nContent-Length: 3\r\n\r\n").await.unwrap();
    assert!(matches!(
        read_request(&mut io, &destination(), limits(), &budget, &|| true).await,
        Err(NativeError::Quota)
    ));
    drop(io);
    let mut bytes = Vec::new();
    peer.read_to_end(&mut bytes).await.unwrap();
    tracing::info!(
        "known quota: interim_bytes={} retained_capacity={}",
        bytes.len(),
        budget.used()
    );
    assert_eq!(budget.used(), 0);
    assert!(
        bytes.is_empty(),
        "quota rejection must precede 100 Continue"
    );
}
#[tokio::test]
async fn chunked_upload_window_precedes_any_continue_on_actual_socket() {
    let (mut peer, mut io) = sockets().await;
    let budget = NativeBudget::new(metadata());
    peer.write_all(b"POST / HTTP/1.1\r\nHost: example.test:8080\r\nExpect: 100-continue\r\nTransfer-Encoding: chunked\r\n\r\n").await.unwrap();
    let mut bounded = limits();
    bounded.io_timeout = Duration::from_millis(20);
    let result = read_request(&mut io, &destination(), bounded, &budget, &|| true).await;
    drop(io);
    let mut bytes = Vec::new();
    peer.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(budget.used(), 0);
    assert!(
        bytes.is_empty(),
        "chunked quota rejection must precede 100 Continue: {} interim bytes",
        bytes.len()
    );
    assert!(matches!(result, Err(NativeError::Quota)));
}
#[tokio::test]
async fn admitted_chunked_continue_keeps_window_through_binary_body() {
    let (mut peer, mut io) = sockets().await;
    let budget = NativeBudget::new(1_000_000);
    let destination = destination();
    let receive = read_request(&mut io, &destination, limits(), &budget, &|| true);
    let send = async {
        peer.write_all(b"POST / HTTP/1.1\r\nHost: example.test:8080\r\nExpect: 100-continue\r\nTransfer-Encoding: chunked\r\n\r\n").await.unwrap();
        let mut interim = [0; 25];
        peer.read_exact(&mut interim).await.unwrap();
        assert_eq!(&interim, b"HTTP/1.1 100 Continue\r\n\r\n");
        assert!(budget.used() >= metadata() + tasty_ipc::html_http::CHUNK_BYTES);
        peer.write_all(b"3\r\n\0x\xff\r\n0\r\n\r\n").await.unwrap();
    };
    let (request, ()) = tokio::join!(receive, send);
    let request = request.unwrap_or_else(|_| panic!("admitted chunked body"));
    assert_eq!(request.upload.as_slice(), b"\0x\xff");
    assert_eq!(request.identity, identity(b"\0x\xff"));
    assert!(budget.used() >= metadata() + request.upload.capacity());
    drop(request);
    assert_eq!(budget.used(), 0);
}
#[derive(Clone, Copy)]
enum Retire {
    Header,
    Payload,
    PendingPayload,
    ReadPayload,
    PendingFlush,
}
struct PartialSocket {
    io: TcpStream,
    live: Arc<AtomicBool>,
    mode: Retire,
    retired: bool,
    after: usize,
    polls: usize,
    read_after: usize,
    flush_after: usize,
}
impl AsyncWrite for PartialSocket {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.polls += 1;
        if !self.live.load(Ordering::Acquire) {
            self.after += 1;
        }
        let target = match self.mode {
            Retire::Header => bytes.starts_with(b"HTTP/1.1"),
            Retire::Payload
            | Retire::PendingPayload
            | Retire::ReadPayload
            | Retire::PendingFlush => bytes.starts_with(b"PAYLOAD"),
        };
        if target && !self.retired && matches!(self.mode, Retire::PendingPayload) {
            self.retired = true;
            self.live.store(false, Ordering::Release);
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        let result = Pin::new(&mut self.io).poll_write(cx, &bytes[..bytes.len().min(1)]);
        if matches!(result,Poll::Ready(Ok(n)) if n>0) && target && !self.retired {
            self.retired = true;
            self.live.store(false, Ordering::Release);
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.live.load(Ordering::Acquire) {
            self.flush_after += 1;
        }
        if matches!(self.mode, Retire::PendingFlush) && !self.retired {
            self.retired = true;
            self.live.store(false, Ordering::Release);
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        Pin::new(&mut self.io).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}
async fn retired_partial_socket(mode: Retire) {
    let payload = b"PAYLOADabcdefghijklmnopqrstuvwxyz0123456789";
    let body = verified(payload.to_vec(), payload, None);
    let (io, mut peer) = sockets().await;
    let live = Arc::new(AtomicBool::new(true));
    let mut io = PartialSocket {
        io,
        live: live.clone(),
        mode,
        retired: false,
        after: 0,
        polls: 0,
        read_after: 0,
        flush_after: 0,
    };
    assert_eq!(
        write_response(
            &mut io,
            body.clone(),
            body.head().id,
            "POST",
            identity(b"\0x\xff"),
            limits(),
            &|| live.load(Ordering::Acquire)
        )
        .await,
        Err(NativeError::Closed)
    );
    let after = io.after;
    let polls = io.polls;
    drop(io);
    let mut bytes = Vec::new();
    peer.read_to_end(&mut bytes).await.unwrap();
    tracing::info!(
        "actual partial socket: mode={} polls={polls} polls_after_retirement={after} socket_bytes={}",
        mode as u8,
        bytes.len()
    );
    assert_eq!(
        after, 0,
        "each underlying write poll must recheck current generation"
    );
    match mode {
        Retire::Header => assert_eq!(bytes, b"H"),
        Retire::Payload => assert!(bytes.ends_with(b"\r\n\r\nP")),
        Retire::PendingPayload => assert!(bytes.ends_with(b"\r\n\r\n")),
        Retire::ReadPayload | Retire::PendingFlush => unreachable!("separate read/flush fixtures"),
    }
}
#[tokio::test]
async fn actual_partial_header_write_stops_after_retirement() {
    retired_partial_socket(Retire::Header).await;
}
#[tokio::test]
async fn actual_partial_body_write_stops_after_retirement() {
    retired_partial_socket(Retire::Payload).await;
}
#[tokio::test]
async fn actual_pending_body_write_rechecks_before_repoll() {
    retired_partial_socket(Retire::PendingPayload).await;
}

impl AsyncRead for PartialSocket {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if !self.live.load(Ordering::Acquire) {
            self.read_after += 1;
        }
        let mut byte = [0; 1];
        let mut read = ReadBuf::new(&mut byte);
        let result = Pin::new(&mut self.io).poll_read(cx, &mut read);
        if matches!(&result, Poll::Ready(Ok(()))) && !read.filled().is_empty() {
            output.put_slice(read.filled());
            if matches!(self.mode, Retire::ReadPayload) && read.filled()[0] == 255 && !self.retired
            {
                self.retired = true;
                self.live.store(false, Ordering::Release);
            }
        }
        result
    }
}
#[tokio::test]
async fn actual_partial_body_read_stops_after_retirement_and_releases_capacity() {
    let (mut peer, io) = sockets().await;
    let live = Arc::new(AtomicBool::new(true));
    let mut io = PartialSocket {
        io,
        live: live.clone(),
        mode: Retire::ReadPayload,
        retired: false,
        after: 0,
        polls: 0,
        read_after: 0,
        flush_after: 0,
    };
    peer.write_all(
        b"POST / HTTP/1.1\r\nHost: example.test:8080\r\nContent-Length: 3\r\n\r\n\xffab",
    )
    .await
    .unwrap();
    let budget = NativeBudget::new(1_000_000);
    assert!(matches!(
        read_request(&mut io, &destination(), limits(), &budget, &|| live
            .load(Ordering::Acquire))
        .await,
        Err(NativeError::Closed)
    ));
    assert!(io.retired);
    assert_eq!(io.read_after, 0);
    assert_eq!(budget.used(), 0);
    tracing::info!(
        "actual partial read: read_polls_after_retirement={} retained_capacity={}",
        io.read_after,
        budget.used()
    );
}
#[tokio::test]
async fn actual_pending_flush_is_not_repolled_after_retirement() {
    let body = verified(b"owned".to_vec(), b"owned", None);
    let (io, mut peer) = sockets().await;
    let live = Arc::new(AtomicBool::new(true));
    let mut io = PartialSocket {
        io,
        live: live.clone(),
        mode: Retire::PendingFlush,
        retired: false,
        after: 0,
        polls: 0,
        read_after: 0,
        flush_after: 0,
    };
    assert_eq!(
        write_response(
            &mut io,
            body.clone(),
            body.head().id,
            "POST",
            identity(b"\0x\xff"),
            limits(),
            &|| live.load(Ordering::Acquire)
        )
        .await,
        Err(NativeError::Closed)
    );
    assert!(io.retired);
    assert_eq!(io.flush_after, 0);
    tracing::info!(
        "actual pending flush: flush_polls_after_retirement={}",
        io.flush_after
    );
    drop(io);
    let mut bytes = Vec::new();
    peer.read_to_end(&mut bytes).await.unwrap();
    assert!(bytes.ends_with(b"owned"));
}
