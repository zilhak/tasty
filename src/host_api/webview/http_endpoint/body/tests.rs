//! Real product endpoint/bridge/server/backend bytes; neither GTK nor actual App grant capture.
use super::super::{
    listener::{DocumentIdentity, Endpoint, EndpointConfig},
    runtime::{NativeRuntime, tests::with_runtime},
};
use super::*;
use crate::remote::{
    html_http as core,
    html_http_server::{Capture, OwnerToken, Server},
};
use std::{
    sync::atomic::{AtomicU64, AtomicUsize},
    time::Duration,
};
use tasty_ipc::{
    html_http::{HttpReply, HttpRequest},
    html_transfer::{HtmlMetadata, HtmlReply, HtmlSource, TransferId},
};
use tasty_remote::{
    connection::{QueuedFrame, channel},
    html_http_body::BodyBudget,
    html_http_bridge::{BridgeConfig, BridgeScope},
    html_http_client::{ClientLimits, PendingBudget},
    html_http_native::{self, Destination, NativeBudget, NativeLimits},
    html_mirror::HtmlMirror,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
const UPLOAD: &[u8] = b"\0\xffx";
const BODY: &[u8] =
    b"<!doctype html><title>verified</title><script>document.title='allowed'</script>";
fn limits() -> core::Limits {
    core::Limits {
        workers: 2,
        requests_per_document: 256,
        retained_bytes: 8_000_000,
        upload_bytes: 1_000_000,
        encoded_bytes: 4_000_000,
        decoded_bytes: 4_000_000,
        header_bytes: 65_536,
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        network_time: Duration::from_secs(2),
    }
}
fn mirror(
    sender: &Arc<tasty_remote::connection::ConnectionSender>,
    url: &str,
) -> (HtmlMirror, tasty_remote::html_mirror::HttpView) {
    let mut mirror = HtmlMirror::new(42, 7, sender.epoch());
    let id = TransferId {
        epoch: 1,
        surface_id: 42,
        binding: 7,
        document: 1,
        request: 1,
        transfer: 1,
    };
    assert_eq!(mirror.begin(id).len(), 1);
    assert!(mirror.receive(HtmlReply::HtmlMetadata {
        id,
        metadata: HtmlMetadata {
            source: HtmlSource::Http { url: url.into() },
            source_generation: 9,
            root: None,
            cwd: Some("/remote".into()),
            size: 0,
            regular: false,
            file: None
        }
    }));
    let view = mirror.http_source().unwrap().new_view().unwrap();
    (mirror, view)
}
fn bridge(
    sender: Arc<tasty_remote::connection::ConnectionSender>,
    view: &tasty_remote::html_mirror::HttpView,
) -> Arc<HttpBridge> {
    let source = view.source().clone();
    let captured = source.clone();
    HttpBridge::new(
        BridgeConfig {
            scope: BridgeScope {
                epoch: 1,
                surface_id: 42,
                binding: 7,
                document: 1,
                source_generation: source.source_generation(),
                entry_url: source.entry_url().into(),
                allow_remote_content: false,
            },
            limits: ClientLimits {
                requests: 256,
                upload_bytes: 1_000_000,
                encoded_bytes: 4_000_000,
                decoded_bytes: 4_000_000,
                verify_jobs: 2,
            },
            pending: PendingBudget::new(8_000_000),
            bodies: BodyBudget::new(8_000_000),
        },
        sender,
        Arc::new(move || captured.is_current()),
        Arc::new(|| {}),
    )
    .unwrap_or_else(|_| panic!("bridge"))
}
async fn post(endpoint: &Endpoint, port: u16) -> Vec<u8> {
    let proxy = url::Url::parse(&endpoint.proxy_uri()).unwrap();
    let mut socket = TcpStream::connect(("127.0.0.1", proxy.port().unwrap()))
        .await
        .unwrap();
    socket.write_all(&[5, 1, 2]).await.unwrap();
    let mut status = [0; 2];
    socket.read_exact(&mut status).await.unwrap();
    assert_eq!(status, [5, 2]);
    let password = proxy.password().unwrap().as_bytes();
    let mut auth = vec![1, 5];
    auth.extend_from_slice(b"tasty");
    auth.push(password.len() as u8);
    auth.extend_from_slice(password);
    socket.write_all(&auth).await.unwrap();
    socket.read_exact(&mut status).await.unwrap();
    assert_eq!(status, [1, 0]);
    let mut route = vec![5, 1, 0, 3, 9];
    route.extend_from_slice(b"localhost");
    route.extend_from_slice(&port.to_be_bytes());
    socket.write_all(&route).await.unwrap();
    let mut reply = [0; 10];
    socket.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[1], 0);
    socket.write_all(format!("POST /post HTTP/1.1\r\nHost: localhost:{port}\r\nCookie: native=kept\r\nContent-Length: 3\r\n\r\n").as_bytes()).await.unwrap();
    socket.write_all(UPLOAD).await.unwrap();
    let mut bytes = Vec::new();
    if let Err(error) = socket.read_to_end(&mut bytes).await {
        assert!(matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
        ));
    }
    bytes
}
#[derive(Default)]
struct Counts {
    begins: AtomicUsize,
    commits: AtomicUsize,
    reuses: AtomicUsize,
    closes: AtomicUsize,
    upstream: AtomicUsize,
    transferred: AtomicU64,
}
async fn upstream(
    listener: TcpListener,
    counts: Arc<Counts>,
    mut stop: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        let socket =
            tokio::select! {result=listener.accept()=>result.unwrap().0,_=stop.changed()=>break};
        let port = listener.local_addr().unwrap().port();
        let mut socket = socket;
        let request = html_http_native::read_request(
            &mut socket,
            &Destination::new("http", "localhost", port).unwrap(),
            NativeLimits {
                upload_bytes: 1_000_000,
                io_timeout: Duration::from_secs(2),
            },
            &NativeBudget::new(4_000_000),
            &|| true,
        )
        .await
        .unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.upload.as_slice(), UPLOAD);
        assert!(
            request
                .headers
                .iter()
                .any(|h| h.name.eq_ignore_ascii_case("cookie") && h.value == b"native=kept")
        );
        counts.upstream.fetch_add(1, Ordering::AcqRel);
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nSet-Cookie: one=1\r\nSet-Cookie: two=2\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",BODY.len()).as_bytes()).await.unwrap();
        socket.write_all(BODY).await.unwrap();
    }
}
async fn driver(
    bridge: Arc<HttpBridge>,
    rx: std::sync::mpsc::Receiver<QueuedFrame>,
    runtime: NativeRuntime,
    counts: Arc<Counts>,
    url: String,
    stop: Arc<AtomicBool>,
) {
    let limits = limits();
    let backend = core::Backend::new(limits.clone(), tokio::runtime::Handle::current())
        .unwrap_or_else(|_| panic!("backend"));
    let mut server = Server::new(backend, 8, &limits).unwrap_or_else(|_| panic!("server"));
    let mut token: Option<OwnerToken> = None;
    let mut number = 1;
    while !stop.load(Ordering::Acquire) {
        bridge.drive(
            || {
                number += 1;
                Some(TransferId {
                    epoch: 1,
                    surface_id: 42,
                    binding: 7,
                    document: 1,
                    request: number,
                    transfer: number,
                })
            },
            |job| runtime.spawn_verify(job),
        );
        for frame in rx.try_iter() {
            let request: HttpRequest = serde_json::from_slice(&frame.frame.payload).unwrap();
            let close = matches!(request, HttpRequest::Close { .. });
            match &request {
                HttpRequest::Begin { id, .. } => {
                    counts.begins.fetch_add(1, Ordering::AcqRel);
                    if token.is_none() {
                        token = Some(
                            server
                                .admit(Capture {
                                    entry: *id,
                                    source_generation: 9,
                                    entry_url: url.clone(),
                                    allow_remote_content: false,
                                })
                                .unwrap_or_else(|_| panic!("server admission")),
                        );
                    }
                }
                HttpRequest::Commit { .. } => {
                    counts.commits.fetch_add(1, Ordering::AcqRel);
                }
                HttpRequest::Reuse { .. } => {
                    counts.reuses.fetch_add(1, Ordering::AcqRel);
                }
                HttpRequest::Close { .. } => {
                    counts.closes.fetch_add(1, Ordering::AcqRel);
                }
                _ => {}
            }
            if let Some(owner) = &token
                && let Err(error) = server.handle(owner, |_| true, request)
            {
                panic!("server command rejected: {error:?}");
            }
            if close {
                token = None;
            }
        }
        if let Some(owner) = &token {
            for _ in 0..16 {
                let Some(frame) = server
                    .poll(
                        owner,
                        |_| true,
                        tasty_ipc::stream::MAX_FRAME_LEN as usize
                            + tasty_ipc::stream::FRAME_HEADER_LEN,
                    )
                    .unwrap_or_else(|_| panic!("server poll"))
                else {
                    break;
                };
                let reply: HttpReply = serde_json::from_slice(&frame.payload).unwrap();
                bridge.receive(reply);
            }
            if let Ok(bytes) = server.transferred_bytes(owner) {
                counts.transferred.store(bytes, Ordering::Release);
            }
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    if let Some(token) = token {
        server.retire(&token);
    }
}
mod roundtrip;

mod retirement;
