//! A launch-scoped relay for the native resume picker. Permission overrides are applied to
//! thread requests because the remote TUI cannot send them when resuming a conversation.
use super::command::ResumeSettings;
use crate::{rpc::Endpoint, ws::WsConn};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::Duration;

type Bind = Arc<dyn Fn(&Value) -> Result<(), String> + Send + Sync>;
type Pending = Arc<Mutex<HashMap<String, ResumeSettings>>>;

#[derive(Debug)]
enum Stream {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixStream),
}
impl Stream {
    fn connect(endpoint: &Endpoint) -> io::Result<Self> {
        let out = match endpoint {
            Endpoint::Ws { port, .. } => Self::Tcp(TcpStream::connect_timeout(
                &([127, 0, 0, 1], *port).into(),
                Duration::from_secs(5),
            )?),
            #[cfg(unix)]
            Endpoint::Unix(path) => Self::Unix(std::os::unix::net::UnixStream::connect(path)?),
            _ => return Err(io::Error::other("relay requires an owned server")),
        };
        out.timeout(Some(Duration::from_secs(10)))?;
        Ok(out)
    }
    fn clone_stream(&self) -> io::Result<Self> {
        match self {
            Self::Tcp(s) => s.try_clone().map(Self::Tcp),
            #[cfg(unix)]
            Self::Unix(s) => s.try_clone().map(Self::Unix),
        }
    }
    fn timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        match self {
            Self::Tcp(s) => {
                s.set_read_timeout(timeout)?;
                s.set_write_timeout(timeout)
            }
            #[cfg(unix)]
            Self::Unix(s) => {
                s.set_read_timeout(timeout)?;
                s.set_write_timeout(timeout)
            }
        }
    }
    fn close(&self) {
        let result = match self {
            Self::Tcp(s) => s.shutdown(Shutdown::Both),
            #[cfg(unix)]
            Self::Unix(s) => s.shutdown(Shutdown::Both),
        };
        if let Err(error) = result {
            tracing::debug!("codex relay close: {error}");
        }
    }
}
impl Read for Stream {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(s) => s.read(b),
            #[cfg(unix)]
            Self::Unix(s) => s.read(b),
        }
    }
}
impl Write for Stream {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(s) => s.write(b),
            #[cfg(unix)]
            Self::Unix(s) => s.write(b),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// A reader may answer pings while the opposite direction forwards messages. Whole frames
// share a writer lock so they cannot interleave on a cloned socket.
struct Duplex {
    read: Stream,
    write: Arc<Mutex<Stream>>,
}
impl Read for Duplex {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.read.read(b)
    }
}
impl Write for Duplex {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.write
            .lock()
            .map_err(|_| io::Error::other("relay writer poisoned"))?
            .write(b)
    }
    fn write_all(&mut self, b: &[u8]) -> io::Result<()> {
        self.write
            .lock()
            .map_err(|_| io::Error::other("relay writer poisoned"))?
            .write_all(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn pair(s: &Stream, server: bool) -> io::Result<(WsConn<Duplex>, WsConn<Duplex>)> {
    let write = Arc::new(Mutex::new(s.clone_stream()?));
    Ok((
        WsConn::established(
            Duplex {
                read: s.clone_stream()?,
                write: write.clone(),
            },
            server,
        ),
        WsConn::established(
            Duplex {
                read: s.clone_stream()?,
                write,
            },
            server,
        ),
    ))
}

fn header(s: &mut Stream) -> io::Result<String> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut b = [0];
        s.read_exact(&mut b)?;
        bytes.push(b[0]);
        if bytes.len() > 16384 {
            return Err(io::Error::other("relay header too large"));
        }
    }
    String::from_utf8(bytes).map_err(io::Error::other)
}

pub(crate) struct Relay {
    pub url: String,
    pub token: String,
    stop: Arc<AtomicBool>,
    streams: Arc<Mutex<Vec<Weak<Stream>>>>,
    task: Option<JoinHandle<()>>,
}
impl Relay {
    pub(crate) fn start(
        endpoint: Endpoint,
        settings: ResumeSettings,
        bind: Bind,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let url = format!("ws://127.0.0.1:{}", listener.local_addr()?.port());
        let token = crate::ids::random_hex(32)?;
        let stop = Arc::new(AtomicBool::new(false));
        let streams = Arc::new(Mutex::new(Vec::new()));
        let (done, sockets, secret) = (stop.clone(), streams.clone(), token.clone());
        let task = std::thread::Builder::new()
            .name("codex-tui-relay".into())
            .spawn(move || {
                let mut connections: Vec<JoinHandle<()>> = Vec::new();
                while !done.load(Ordering::Acquire) {
                    reap_finished(&mut connections);
                    match listener.accept() {
                        Ok((socket, _)) => {
                            if connections.len() >= 16 {
                                drop(socket);
                                continue;
                            }
                            connections.push(spawn_connection(
                                socket, &endpoint, &settings, &bind, &secret, &sockets,
                            ));
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(20))
                        }
                        Err(error) => {
                            tracing::warn!("codex relay accept: {error}");
                            break;
                        }
                    }
                }
                close_streams(&sockets);
                join_all(connections);
            })?;
        Ok(Self {
            url,
            token,
            stop,
            streams,
            task: Some(task),
        })
    }
}

/// 끝난 연결 스레드를 거둔다.
fn reap_finished(connections: &mut Vec<JoinHandle<()>>) {
    let mut index = 0;
    while index < connections.len() {
        if connections[index].is_finished() {
            if connections.swap_remove(index).join().is_err() {
                tracing::warn!("codex relay connection panicked");
            }
        } else {
            index += 1;
        }
    }
}

/// 받아들인 연결 하나를 새 스레드에서 중계한다.
fn spawn_connection(
    socket: TcpStream,
    endpoint: &Endpoint,
    settings: &ResumeSettings,
    bind: &Bind,
    secret: &str,
    sockets: &Arc<Mutex<Vec<Weak<Stream>>>>,
) -> JoinHandle<()> {
    let (endpoint, settings, bind, secret, sockets) = (
        endpoint.clone(),
        settings.clone(),
        bind.clone(),
        secret.to_string(),
        sockets.clone(),
    );
    std::thread::spawn(move || {
        if let Err(error) = connection(
            Stream::Tcp(socket),
            &endpoint,
            &secret,
            &settings,
            bind,
            &sockets,
        ) {
            tracing::debug!("codex relay connection: {error}");
        }
    })
}

/// 남은 연결 스레드가 끝날 때까지 기다린다.
fn join_all(connections: Vec<JoinHandle<()>>) {
    for task in connections {
        if task.join().is_err() {
            tracing::warn!("codex relay connection panicked");
        }
    }
}

/// 열린 연결을 모두 닫는다.
fn close_streams(sockets: &Mutex<Vec<Weak<Stream>>>) {
    if let Ok(sockets) = sockets.lock() {
        for socket in sockets.iter() {
            if let Some(socket) = socket.upgrade() {
                socket.close();
            }
        }
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(sockets) = self.streams.lock() {
            for socket in sockets.iter() {
                if let Some(socket) = socket.upgrade() {
                    socket.close();
                }
            }
        }
        if let Some(task) = self.task.take()
            && task.join().is_err()
        {
            tracing::warn!("codex relay panicked");
        }
    }
}

fn apply_settings(
    params: &mut serde_json::Map<String, Value>,
    defaults: &ResumeSettings,
) -> ResumeSettings {
    let config = params.get("config").and_then(Value::as_object);
    let permission_config = config.is_some_and(|config| {
        [
            "approval_policy",
            "approvals_reviewer",
            "sandbox_mode",
            "default_permissions",
            "permissions",
            "network",
            "sandbox_workspace_write",
        ]
        .iter()
        .any(|key| config.get(*key).is_some_and(|value| !value.is_null()))
    });
    let model_config =
        config.is_some_and(|config| config.get("model").is_some_and(|value| !value.is_null()));
    for (key, value) in defaults.params("").as_object().into_iter().flatten() {
        if (permission_config && matches!(key.as_str(), "approvalPolicy" | "sandbox"))
            || (model_config && key == "model")
        {
            continue;
        }
        if key != "threadId" && key != "excludeTurns" && params.get(key).is_none_or(Value::is_null)
        {
            params.insert(key.clone(), value.clone());
        }
    }
    // Later explicit choices in the TUI take precedence over launch defaults.
    let string = |key| params.get(key).and_then(Value::as_str).map(str::to_owned);
    ResumeSettings {
        approval: string("approvalPolicy"),
        sandbox: string("sandbox"),
        model: string("model"),
    }
}

struct CloseOnDrop(Arc<Stream>);
impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        self.0.close();
    }
}

fn track(socket: &Stream, sockets: &Mutex<Vec<Weak<Stream>>>) -> io::Result<CloseOnDrop> {
    let socket = Arc::new(socket.clone_stream()?);
    let mut live = sockets
        .lock()
        .map_err(|_| io::Error::other("relay socket list poisoned"))?;
    live.retain(|socket| socket.strong_count() > 0);
    live.push(Arc::downgrade(&socket));
    Ok(CloseOnDrop(socket))
}

fn connection(
    mut front: Stream,
    endpoint: &Endpoint,
    token: &str,
    settings: &ResumeSettings,
    bind: Bind,
    sockets: &Mutex<Vec<Weak<Stream>>>,
) -> io::Result<()> {
    let _front_guard = track(&front, sockets)?;
    front.timeout(Some(Duration::from_secs(10)))?;
    let request = header(&mut front)?;
    let auth = format!("Bearer {token}");
    if !request
        .lines()
        .filter_map(|l| l.split_once(':'))
        .any(|(k, v)| k.eq_ignore_ascii_case("authorization") && v.trim() == auth)
    {
        front.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n")?;
        return Err(io::Error::other("relay authentication failed"));
    }
    let mut backend = Stream::connect(endpoint)?;
    let _backend_guard = track(&backend, sockets)?;
    let mut forwarded = request
        .lines()
        .filter(|l| {
            !l.is_empty()
                && !l
                    .split_once(':')
                    .is_some_and(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        })
        .collect::<Vec<_>>()
        .join("\r\n");
    if let Endpoint::Ws { token, .. } = endpoint {
        forwarded.push_str(&format!("\r\nAuthorization: Bearer {token}"));
    }
    forwarded.push_str("\r\n\r\n");
    backend.write_all(forwarded.as_bytes())?;
    let response = header(&mut backend)?;
    front.write_all(response.as_bytes())?;
    if !response.lines().next().is_some_and(|l| l.contains(" 101 ")) {
        return Err(io::Error::other("backend refused relay connection"));
    }
    front.timeout(None)?;
    backend.timeout(None)?;
    let (mut from_tui, mut to_tui) = pair(&front, true)?;
    let (mut from_server, mut to_server) = pair(&backend, false)?;
    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
    let (requests, overrides) = (pending.clone(), settings.clone());
    let close = backend.clone_stream()?;
    let upstream = std::thread::spawn(move || {
        let result = (|| -> io::Result<()> {
            loop {
                let text = from_tui.recv_text()?;
                let mut message: Value = serde_json::from_str(&text).map_err(io::Error::other)?;
                if matches!(
                    message["method"].as_str(),
                    Some("thread/start" | "thread/resume" | "thread/fork")
                ) {
                    let params = message
                        .get_mut("params")
                        .and_then(Value::as_object_mut)
                        .ok_or_else(|| io::Error::other("missing thread parameters"))?;
                    let effective = apply_settings(params, &overrides);
                    requests
                        .lock()
                        .map_err(|_| io::Error::other("relay pending poisoned"))?
                        .insert(message["id"].to_string(), effective);
                }
                to_server.send_text(&message.to_string())?;
            }
        })();
        close.close();
        result
    });
    let result = (|| -> io::Result<()> {
        loop {
            let text = from_server.recv_text()?;
            let mut message: Value = serde_json::from_str(&text).map_err(io::Error::other)?;
            if message.get("result").is_some() || message.get("error").is_some() {
                let settings = pending
                    .lock()
                    .map_err(|_| io::Error::other("relay pending poisoned"))?
                    .remove(&message["id"].to_string());
                if let Some(settings) = settings
                    && let Some(result) = message.get("result")
                    && let Err(error) = settings.verify(result).and_then(|()| bind(result))
                {
                    message = json!({"id":message["id"], "error":{"code":-32000,"message":error}});
                }
            }
            to_tui.send_text(&message.to_string())?;
        }
    })();
    front.close();
    backend.close();
    if upstream.join().is_err() {
        tracing::warn!("codex relay upstream panicked");
    }
    result
}

#[cfg(test)]
#[path = "relay_tests.rs"]
mod tests;
