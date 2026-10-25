//! Exercise the production recorder through Rustls, including its actual retained capacity.
use super::*;
use tokio::io::AsyncReadExt;
use tokio_rustls::{LazyConfigAcceptor, rustls};

struct Fragmented {
    bytes: std::io::Cursor<Vec<u8>>,
    chunks: &'static [usize],
    reads: usize,
}
impl Fragmented {
    fn new(bytes: Vec<u8>, chunks: &'static [usize]) -> Self {
        Self {
            bytes: std::io::Cursor::new(bytes),
            chunks,
            reads: 0,
        }
    }
}
impl AsyncRead for Fragmented {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let position = self.bytes.position() as usize;
        let count = self.chunks[self.reads % self.chunks.len()]
            .min(buf.remaining())
            .min(self.bytes.get_ref().len() - position);
        self.reads += 1;
        buf.put_slice(&self.bytes.get_ref()[position..position + count]);
        self.bytes.set_position((position + count) as u64);
        Poll::Ready(Ok(()))
    }
}
impl AsyncWrite for Fragmented {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, _: &[u8]) -> Poll<io::Result<usize>> {
        panic!("ClientHello capture must not release a server TLS flight")
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

// Observe every poll of the actual production constructor/reader; do not replace its storage.
struct Measured {
    recording: Recording<Fragmented>,
    peak: usize,
}
impl Measured {
    fn new(bytes: Vec<u8>, chunks: &'static [usize]) -> Self {
        let recording = Recording::new(Fragmented::new(bytes, chunks));
        let peak = recording.bytes.capacity();
        Self { recording, peak }
    }
}
impl AsyncRead for Measured {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.recording).poll_read(cx, buf);
        this.peak = this.peak.max(this.recording.bytes.capacity());
        result
    }
}
impl AsyncWrite for Measured {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().recording).poll_write(cx, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().recording).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().recording).poll_shutdown(cx)
    }
}
fn oversized_hello(declared: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for index in 0..5 {
        bytes.extend_from_slice(&[22, 3, 3, 0x40, 0]);
        let mut record = vec![0; 16_384];
        if index == 0 {
            record[..4].copy_from_slice(&[
                1,
                (declared >> 16) as u8,
                (declared >> 8) as u8,
                declared as u8,
            ]);
        }
        bytes.extend_from_slice(&record);
    }
    bytes
}
async fn rejected(bytes: Vec<u8>, chunks: &'static [usize]) -> Measured {
    let mut acceptor = LazyConfigAcceptor::new(
        rustls::server::Acceptor::default(),
        Measured::new(bytes, chunks),
    );
    assert!((&mut acceptor).await.is_err());
    acceptor.take_io().unwrap()
}
#[tokio::test]
async fn actual_lazy_acceptor_recording_allocation_stays_within_advertised_65536() {
    let measured = rejected(oversized_hello(65_504), &[4095, 4096]).await;
    assert_eq!(measured.recording.bytes.len(), 65_535);
    assert!(
        measured.peak <= MAX_HELLO,
        "actual replay allocation exceeds advertised 64KiB"
    );
}
#[tokio::test]
async fn malformed_and_truncated_fragmentation_never_grows_recording_past_its_bound() {
    for declared in [65_504, 65_535, 65_536] {
        for chunks in [&[1][..], &[4095, 4096][..], &[4096][..], &[7, 4093, 2][..]] {
            let measured = rejected(oversized_hello(declared), chunks).await;
            assert!(measured.peak <= MAX_HELLO);
            assert!(measured.recording.bytes.len() <= MAX_HELLO);
        }
    }
    let mut truncated = oversized_hello(65_504);
    truncated.truncate(8191);
    let measured = rejected(truncated, &[4095, 4096]).await;
    assert_eq!(measured.recording.bytes.len(), 8191);
    assert!(measured.peak <= MAX_HELLO);
}
fn valid_hello() -> Vec<u8> {
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(rustls::RootCertStore::empty())
    .with_no_client_auth();
    let mut client = rustls::ClientConnection::new(
        Arc::new(config),
        rustls::pki_types::ServerName::try_from("remote.example").unwrap(),
    )
    .unwrap();
    let mut bytes = Vec::new();
    client.write_tls(&mut bytes).unwrap();
    bytes
}
#[tokio::test]
async fn production_capture_retains_bounded_storage_and_replays_exact_bytes() {
    for chunks in [&[1][..], &[4095, 4096][..]] {
        let mut bytes = valid_hello();
        bytes.extend_from_slice(b"\xde\xad\xbe\xef");
        let authority = Authority::new("remote.example", 8443).unwrap();
        let mut replay = capture(
            Fragmented::new(bytes.clone(), chunks),
            &authority,
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert!(replay.bytes.get_ref().capacity() <= MAX_HELLO);
        let mut actual = Vec::new();
        replay.read_to_end(&mut actual).await.unwrap();
        assert_eq!(
            actual, bytes,
            "recorded prefix and unread socket bytes must replay once without padding"
        );
        assert!(replay.bytes.get_ref().capacity() <= MAX_HELLO);
        let foreign = Authority::new("foreign.example", 8443).unwrap();
        assert!(matches!(
            capture(
                Fragmented::new(valid_hello(), chunks),
                &foreign,
                Duration::from_secs(1)
            )
            .await,
            Err(EndpointError::Destination)
        ));
    }
}
#[tokio::test]
async fn production_recording_enforces_exact_byte_limit_without_overread() {
    for length in [0, 1, 4095, 4096, MAX_HELLO - 1, MAX_HELLO, MAX_HELLO + 1] {
        let mut recording = Recording::new(Fragmented::new(vec![42; length], &[4095, 4096]));
        let mut read = Vec::new();
        let result = recording.read_to_end(&mut read).await;
        assert_eq!(read.len(), length.min(MAX_HELLO));
        assert_eq!(
            recording.io.bytes.position() as usize,
            length.min(MAX_HELLO)
        );
        assert_eq!(recording.bytes, read);
        assert!(recording.bytes.capacity() <= MAX_HELLO);
        if length >= MAX_HELLO {
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
        } else {
            result.unwrap();
        }
    }
}
