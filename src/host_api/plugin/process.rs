//! Plugin 자식 프로세스 + 호스트와의 양방향 채널.
//!
//! `PluginProcess::spawn(...)`는:
//! 1. 토큰 생성
//! 2. 자식 프로세스 spawn (env로 host port + token + plugin id 전달, stdout/stderr는 로그 파일)
//! 3. listener에서 token 매칭된 connection 수신 (timeout 10s)
//! 4. 송신/수신 스레드 가동 → mpsc 채널로 호스트 메인 루프에 노출
//!
//! plugin이 응답할 때마다 `last_pong`이 갱신된다. 헬스체크는 `since_last_pong()` 비교.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use tasty_plugin_protocol::{HandleChannelMessage, PixelRect, SharedBufferId};

use crate::plugin::handle_channel::{HandleListener, HandleStream, HandleStreamReader};
use crate::plugin::listener::HostListener;
use crate::plugin::manifest::{HOST_API_VERSION, PluginPackage};
use crate::plugin::protocol::{PluginEvent, PluginRequest, PluginResponse};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// 보조 채널 stream을 mailbox에서 가져올 때 첫 호출 한도. plugin SDK가 HandleClient::connect
/// 완료 → 호스트 accept thread가 stream을 우편함에 채울 때까지 ms 단위 정도면 충분하지만,
/// startup 직후 호출 가능성을 고려해 500ms 여유.
const HANDLE_STREAM_MATERIALIZE_TIMEOUT: Duration = Duration::from_millis(500);

/// 보조 핸들 채널 상태 머신. spawn 시점에 Pending(rx) 또는 Unavailable로 초기화되고,
/// 첫 사용 시 Pending → Ready(stream)으로 전이. Ready 전이 시 reader 스레드도 함께
/// 시작되어 plugin이 보내는 Dirty 메시지를 수신한다.
enum HandleStreamState {
    /// 아직 plugin이 보조 채널에 connect하지 않음. mailbox에서 try-recv 대기.
    Pending(mpsc::Receiver<HandleStream>),
    /// 한 번 materialize 완료. write 핸들은 Arc로 공유 — reader 스레드가 Pong을
    /// 응답할 때도 같은 stream을 쓴다.
    Ready(Arc<Mutex<HandleStream>>),
    /// 보조 채널 미지원 (handle_listener bind 실패 / Windows stub) 또는 reader
    /// 분리 실패. 향후 호출이 항상 None을 반환하도록 sticky.
    Unavailable,
}

pub struct PluginProcess {
    pub plugin_id: String,
    child: Option<Child>,
    pub req_tx: mpsc::Sender<PluginRequest>,
    pub resp_rx: mpsc::Receiver<PluginResponse>,
    pub event_rx: mpsc::Receiver<PluginEvent>,
    last_pong: Arc<Mutex<Instant>>,
    /// 자식 stdout/stderr가 redirect된 파일 경로. 디버깅/검증 시 직접 참조.
    pub log_path: PathBuf,
    /// 보조 핸들 채널 상태. 첫 사용 시 Pending → Ready 전이하며 reader 스레드 시작.
    handle_state: Mutex<HandleStreamState>,
    /// reader 스레드가 누적하는 dirty rect. `Some(rect)`는 union된 영역, `None`은
    /// "전체 갱신" sticky flag. 호스트 main loop이 frame 합성 시 `take_dirty_rects`로
    /// drain한다.
    dirty_rects: Arc<Mutex<HashMap<SharedBufferId, Option<PixelRect>>>>,
}

#[cfg(test)]
impl PluginProcess {
    /// 단위 테스트 전용 stub. child/last_pong 등 외부에서 접근 불가능한 필드를
    /// 합리적인 기본값으로 채운다. 송수신 채널은 dangling이라 실제로 사용하면 안 된다.
    pub(crate) fn stub_for_test(plugin_id: &str) -> Self {
        let (req_tx, _req_rx) = mpsc::channel();
        let (_resp_tx, resp_rx) = mpsc::channel();
        let (_event_tx, event_rx) = mpsc::channel();
        Self {
            plugin_id: plugin_id.into(),
            child: None,
            req_tx,
            resp_rx,
            event_rx,
            last_pong: Arc::new(Mutex::new(Instant::now())),
            log_path: PathBuf::new(),
            handle_state: Mutex::new(HandleStreamState::Unavailable),
            dirty_rects: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl PluginProcess {
    pub fn spawn(
        package: &PluginPackage,
        listener: &HostListener,
        handle_listener: Option<&HandleListener>,
        log_dir: &Path,
        waker: tasty_core::SharedWakerFactory,
    ) -> anyhow::Result<Self> {
        let token = generate_token();
        std::fs::create_dir_all(log_dir).ok();
        let log_path = log_dir.join(format!("{}.log", sanitize_id(&package.manifest.id)));
        let log_file = std::fs::File::create(&log_path)?;
        let log_clone = log_file.try_clone()?;

        let entry_path = package.entry_command_path();
        let mut cmd = Command::new(&entry_path);
        cmd.args(package.entry_args())
            .env("TASTY_PLUGIN_ID", &package.manifest.id)
            .env("TASTY_HOST_API_VERSION", HOST_API_VERSION)
            .env("TASTY_HOST_IPC_PORT", listener.port().to_string())
            .env("TASTY_PLUGIN_TOKEN", &token)
            .env("TASTY_PLUGIN_DIR", &package.dir)
            .env("TASTY_LOCALE", tasty_core::i18n::current_language())
            .current_dir(&package.dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_clone));

        // 보조 채널 endpoint를 plugin에 알려주고, mailbox를 미리 등록한다. listener가
        // 없거나 stub인 경우 env를 주입하지 않아 plugin SDK가 보조 채널을 skip한다.
        //
        // 중요: mailbox 등록은 *child spawn 전*에 일어나야 한다. 그래야 SDK가 빠르게
        // connect해도 accept thread가 호출 시점에 매핑할 sender를 찾을 수 있다.
        let handle_stream_rx = if let Some(hl) = handle_listener {
            cmd.env("TASTY_PLUGIN_HANDLE_ENDPOINT", hl.endpoint());
            Some(hl.register_token(&token))
        } else {
            None
        };

        // plugin별 격리 디렉터리. 디렉터리 생성은 호스트가 미리 보장한다 — plugin이
        // fs.write 권한 없이도 자기 영역만은 자유롭게 쓸 수 있도록.
        if let Some(home) = tasty_core::paths::tasty_home() {
            let data_dir = home.join("plugin-data").join(&package.manifest.id);
            let config_path = home
                .join("plugin-config")
                .join(format!("{}.toml", &package.manifest.id));
            if let Err(e) = std::fs::create_dir_all(&data_dir) {
                tracing::warn!("plugin data dir {} create failed: {e}", data_dir.display());
            }
            if let Some(parent) = config_path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    tracing::warn!("plugin config dir {} create failed: {e}", parent.display());
                }
            }
            cmd.env("TASTY_PLUGIN_DATA_DIR", &data_dir);
            cmd.env("TASTY_PLUGIN_CONFIG_PATH", &config_path);
            cmd.env("TASTY_PLUGIN_LOG_PATH", &log_path);
        }

        let child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!(
                "failed to spawn plugin '{}' ({}): {}",
                package.manifest.id,
                entry_path.display(),
                e
            )
        })?;

        let stream = match listener.expect_connection(&token, HANDSHAKE_TIMEOUT) {
            Some(s) => s,
            None => {
                anyhow::bail!(
                    "plugin '{}' did not connect within {}s — log: {}",
                    package.manifest.id,
                    HANDSHAKE_TIMEOUT.as_secs(),
                    log_path.display()
                );
            }
        };

        // 보조 채널은 별도 mailbox로 받는다 — blocking하지 않는다. plugin이 connect하면
        // listener accept thread가 stream을 receiver로 넣어 둠. shared buffer 사용 시점에
        // 비로소 try_recv로 가져온다. plugin이 영영 connect 안 해도 startup 지연 0.

        let last_pong = Arc::new(Mutex::new(Instant::now()));
        let (req_tx, req_rx) = mpsc::channel::<PluginRequest>();
        let (resp_tx, resp_rx) = mpsc::channel::<PluginResponse>();
        let (event_tx, event_rx) = mpsc::channel::<PluginEvent>();

        // 송신 스레드
        let mut writer = stream.try_clone()?;
        let plugin_id_tx = package.manifest.id.clone();
        std::thread::Builder::new()
            .name(format!("plugin-tx-{}", sanitize_id(&plugin_id_tx)))
            .spawn(move || {
                for req in req_rx.iter() {
                    let line = match serde_json::to_string(&req) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!("plugin '{}' request encode error: {}", plugin_id_tx, e);
                            continue;
                        }
                    };
                    if writeln!(writer, "{line}").is_err() {
                        break;
                    }
                    if writer.flush().is_err() {
                        break;
                    }
                }
            })?;

        // 수신 스레드
        let waker_clone = waker.clone();
        let last_pong_clone = last_pong.clone();
        let plugin_id_rx = package.manifest.id.clone();
        std::thread::Builder::new()
            .name(format!("plugin-rx-{}", sanitize_id(&plugin_id_rx)))
            .spawn(move || {
                let reader = BufReader::new(stream);
                for line in reader.lines() {
                    let line = match line {
                        Ok(l) => l,
                        Err(_) => break,
                    };
                    let trim = line.trim();
                    if trim.is_empty() {
                        continue;
                    }
                    handle_incoming_line(
                        trim,
                        &resp_tx,
                        &event_tx,
                        &last_pong_clone,
                        &plugin_id_rx,
                    );
                    waker_clone.make_default_waker()();
                }
            })?;

        let initial_state = match handle_stream_rx {
            Some(rx) => HandleStreamState::Pending(rx),
            None => HandleStreamState::Unavailable,
        };
        Ok(Self {
            plugin_id: package.manifest.id.clone(),
            child: Some(child),
            req_tx,
            resp_rx,
            event_rx,
            last_pong,
            log_path,
            handle_state: Mutex::new(initial_state),
            dirty_rects: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// 보조 핸들 채널 stream을 첫 호출 시 materialize한 뒤 closure로 노출한다.
    ///
    /// 첫 호출은 mailbox에서 짧은 timeout(`HANDLE_STREAM_MATERIALIZE_TIMEOUT`)으로
    /// 대기하며, 성공하면 reader 스레드를 함께 spawn해 dirty 수신을 시작한다.
    /// 이후 호출은 캐시된 stream을 lock한 뒤 closure에 넘긴다. 보조 채널이
    /// 활성화되지 않은 plugin이면 `None`.
    pub fn with_handle_stream<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut HandleStream) -> R,
    {
        let arc = self.ensure_handle_stream()?;
        let mut g = arc.lock().ok()?;
        Some(f(&mut g))
    }

    fn ensure_handle_stream(&self) -> Option<Arc<Mutex<HandleStream>>> {
        let mut state = self.handle_state.lock().ok()?;
        match &*state {
            HandleStreamState::Ready(arc) => return Some(arc.clone()),
            HandleStreamState::Unavailable => return None,
            HandleStreamState::Pending(_) => {}
        }
        // Pending → recv_timeout 안에 plugin이 connect했는지 시도.
        // 실패 시 rx를 다시 Pending에 넣어 후속 호출이 재시도할 수 있게 한다.
        let rx = match std::mem::replace(&mut *state, HandleStreamState::Unavailable) {
            HandleStreamState::Pending(rx) => rx,
            other => {
                *state = other;
                return None;
            }
        };
        let stream = match rx.recv_timeout(HANDLE_STREAM_MATERIALIZE_TIMEOUT) {
            Ok(s) => s,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                tracing::warn!(
                    "plugin '{}' handle stream not yet available",
                    self.plugin_id
                );
                *state = HandleStreamState::Pending(rx);
                return None;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // accept thread가 종료됨. 영구적으로 사용 불가.
                return None;
            }
        };
        let reader = match stream.reader() {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    "plugin '{}' handle stream reader split failed: {e}",
                    self.plugin_id
                );
                return None;
            }
        };
        let arc = Arc::new(Mutex::new(stream));
        let dirty = self.dirty_rects.clone();
        let writer = arc.clone();
        let plugin_id = self.plugin_id.clone();
        if let Err(e) = std::thread::Builder::new()
            .name(format!("plugin-aux-rx-{}", sanitize_id(&plugin_id)))
            .spawn(move || aux_reader_loop(reader, dirty, writer, plugin_id))
        {
            tracing::warn!(
                "plugin '{}' aux reader thread spawn failed: {e}",
                self.plugin_id
            );
            return None;
        }
        *state = HandleStreamState::Ready(arc.clone());
        Some(arc)
    }

    /// reader 스레드가 누적한 dirty rect를 drain. 호스트 main loop이 frame 합성 직전에
    /// 호출. 반환된 map의 value가 `None`이면 "전체 갱신".
    pub fn take_dirty_rects(&self) -> HashMap<SharedBufferId, Option<PixelRect>> {
        self.dirty_rects
            .lock()
            .map(|mut m| std::mem::take(&mut *m))
            .unwrap_or_default()
    }

    /// 자식 프로세스의 OS PID. Windows의 `DuplicateHandle` 대상 식별에 필요.
    /// `shutdown` 이후나 stub 인스턴스에서는 `None`.
    pub fn child_pid(&self) -> Option<u32> {
        self.child.as_ref().map(|c| c.id())
    }

    pub fn ping(&self, next_id: u64) {
        if let Err(e) = self.req_tx.send(PluginRequest {
            method: "ping".into(),
            params: serde_json::json!({}),
            id: next_id,
        }) {
            tracing::trace!("plugin ping send dropped (writer exited): {e}");
        }
    }

    pub fn since_last_pong(&self) -> Duration {
        self.last_pong
            .lock()
            .map(|t| t.elapsed())
            .unwrap_or(Duration::MAX)
    }

    pub fn shutdown(mut self, timeout: Duration) {
        if let Err(e) = self.req_tx.send(PluginRequest {
            method: "shutdown".into(),
            params: serde_json::json!({}),
            id: u64::MAX,
        }) {
            tracing::trace!("plugin shutdown send dropped (writer exited): {e}");
        }
        if let Some(mut child) = self.child.take() {
            let deadline = Instant::now() + timeout;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => return,
                    Ok(None) => {
                        if Instant::now() > deadline {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => break,
                }
            }
            // shutdown 메시지 전송 실패 / 타임아웃 시 강제 종료. kill 실패는 이미
            // 죽은 프로세스(`ESRCH`)거나 OS 권한 문제이며, 어느 쪽이든 호스트가
            // 추가로 할 수 있는 일이 없으므로 trace로만 흔적을 남긴다.
            if let Err(e) = child.kill() {
                tracing::trace!("plugin child kill failed (already exited?): {e}");
            }
            if let Err(e) = child.wait() {
                tracing::trace!("plugin child wait failed: {e}");
            }
        }
    }
}

impl Drop for PluginProcess {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            if let Err(e) = child.kill() {
                tracing::trace!("plugin child kill on drop failed: {e}");
            }
            if let Err(e) = child.wait() {
                tracing::trace!("plugin child wait on drop failed: {e}");
            }
        }
    }
}

fn handle_incoming_line(
    line: &str,
    resp_tx: &mpsc::Sender<PluginResponse>,
    event_tx: &mpsc::Sender<PluginEvent>,
    last_pong: &Arc<Mutex<Instant>>,
    plugin_id: &str,
) {
    let v: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("plugin '{plugin_id}' sent unparseable line: {e}");
            return;
        }
    };
    if v.get("id").and_then(|x| x.as_u64()).is_some() {
        match serde_json::from_value::<PluginResponse>(v) {
            Ok(resp) => {
                if let Ok(mut p) = last_pong.lock() {
                    *p = Instant::now();
                }
                if let Err(e) = resp_tx.send(resp) {
                    tracing::trace!("plugin response forward dropped (consumer exited): {e}");
                }
            }
            Err(e) => {
                tracing::warn!("plugin '{plugin_id}' response decode error: {e}");
            }
        }
        return;
    }
    if let Some(ev_value) = v.get("event") {
        match serde_json::from_value::<PluginEvent>(ev_value.clone()) {
            Ok(ev) => {
                if let Err(e) = event_tx.send(ev) {
                    tracing::trace!("plugin event forward dropped (consumer exited): {e}");
                }
            }
            Err(e) => {
                tracing::warn!("plugin '{plugin_id}' event decode error: {e}");
            }
        }
    }
}

/// 보조 채널 reader 스레드의 메시지 처리 루프.
///
/// - `Dirty`: `dirty_rects`에 union(coalesce)해 누적.
/// - `Ping`: 동일 `seq`로 `Pong` 응답.
/// - `Pong`: 호스트는 ping을 보내지 않으므로 무시(트레이스 로그만).
/// - `HandleAttach`: plugin→host로 오는 일은 없어야 함. 받으면 fd 즉시 close 후 경고.
///
/// EOF가 도착하면 (plugin 종료/재시작 또는 정상 shutdown) 조용히 종료.
fn aux_reader_loop(
    mut reader: HandleStreamReader,
    dirty: Arc<Mutex<HashMap<SharedBufferId, Option<PixelRect>>>>,
    writer: Arc<Mutex<HandleStream>>,
    plugin_id: String,
) {
    loop {
        match reader.recv_message() {
            Ok((HandleChannelMessage::Dirty { id, rect }, _)) => {
                merge_dirty(&dirty, id, rect);
            }
            Ok((HandleChannelMessage::Ping { seq }, _)) => {
                if let Ok(mut w) = writer.lock() {
                    if let Err(e) = w.send_message(&HandleChannelMessage::Pong { seq }) {
                        tracing::warn!("plugin '{plugin_id}' aux Pong send failed: {e}");
                    }
                }
            }
            Ok((HandleChannelMessage::Pong { .. }, _)) => {
                // 호스트가 Ping을 보내지 않으므로 정상 시나리오에서는 도착하지 않는다.
            }
            Ok((HandleChannelMessage::HandleAttach { .. }, aux)) => {
                tracing::warn!("plugin '{plugin_id}' sent unexpected HandleAttach on aux channel");
                #[cfg(unix)]
                if let Some(fd) = aux {
                    // SAFETY: 동행 fd가 있다면 우리가 SCM_RIGHTS로 받은 새 fd 소유권.
                    // 사용처가 없으므로 leak 방지를 위해 close.
                    unsafe { libc::close(fd) };
                }
                // Windows: aux는 DuplicateHandle 결과인데 unexpected HandleAttach라 사용처
                // 없음. windows-rs OwnedHandle을 받았다면 Drop이 CloseHandle 처리하므로
                // 별도 액션 불필요 — drop만 시키면 된다.
                #[cfg(windows)]
                drop(aux);
            }
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => {
                tracing::warn!("plugin '{plugin_id}' aux channel reader error: {e}");
                break;
            }
        }
    }
}

/// 한 buffer의 dirty 상태를 incoming rect와 union한다. value가 `None`이면 "전체 갱신"
/// sticky flag — 더 이상 좁히지 않는다.
fn merge_dirty(
    map: &Arc<Mutex<HashMap<SharedBufferId, Option<PixelRect>>>>,
    id: SharedBufferId,
    incoming: Option<PixelRect>,
) {
    let Ok(mut m) = map.lock() else {
        return;
    };
    match (m.get(&id).copied(), incoming) {
        (Some(None), _) => {} // 이미 full — 무시.
        (_, None) => {
            m.insert(id, None);
        }
        (None, Some(r)) => {
            m.insert(id, Some(r));
        }
        (Some(Some(existing)), Some(r)) => {
            m.insert(id, Some(union_rect(existing, r)));
        }
    }
}

/// 두 정수 rect의 bounding union. tasty-plugin-protocol의 PixelRect는 (x, y, w, h)이고
/// w/h=0은 invalid 취급이지만 reader는 wire 그대로 union한다 (필터링은 호출자).
fn union_rect(a: PixelRect, b: PixelRect) -> PixelRect {
    let x1 = a.x.min(b.x);
    let y1 = a.y.min(b.y);
    let x2 = a.x.saturating_add(a.w).max(b.x.saturating_add(b.w));
    let y2 = a.y.saturating_add(a.h).max(b.y.saturating_add(b.h));
    PixelRect {
        x: x1,
        y: y1,
        w: x2.saturating_sub(x1),
        h: y2.saturating_sub(y1),
    }
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn generate_token() -> String {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // 단순한 의사 랜덤 — 단계 07에서 강화 가능 (rand 크레이트 등).
    let a = (nanos as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let b = ((nanos >> 64) as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    format!("{a:016x}{b:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_32_hex_chars() {
        let t = generate_token();
        assert_eq!(t.len(), 32);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn sanitize_strips_special() {
        assert_eq!(sanitize_id("com.foo/bar:baz"), "com.foo_bar_baz");
        assert_eq!(sanitize_id("com.example-x"), "com.example-x");
    }

    #[test]
    fn union_rect_combines_bbox() {
        let a = PixelRect {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        };
        let b = PixelRect {
            x: 5,
            y: 5,
            w: 10,
            h: 10,
        };
        let u = union_rect(a, b);
        assert_eq!(
            u,
            PixelRect {
                x: 0,
                y: 0,
                w: 15,
                h: 15
            }
        );
    }

    #[test]
    fn union_rect_disjoint_gives_outer_bbox() {
        let a = PixelRect {
            x: 0,
            y: 0,
            w: 4,
            h: 4,
        };
        let b = PixelRect {
            x: 10,
            y: 10,
            w: 5,
            h: 5,
        };
        let u = union_rect(a, b);
        assert_eq!(
            u,
            PixelRect {
                x: 0,
                y: 0,
                w: 15,
                h: 15
            }
        );
    }

    #[test]
    fn merge_dirty_full_is_sticky() {
        let map: Arc<Mutex<HashMap<SharedBufferId, Option<PixelRect>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let id = SharedBufferId(1);
        merge_dirty(&map, id, None);
        // 이후 Some이 와도 None 유지.
        merge_dirty(
            &map,
            id,
            Some(PixelRect {
                x: 0,
                y: 0,
                w: 2,
                h: 2,
            }),
        );
        assert_eq!(map.lock().unwrap().get(&id).copied(), Some(None));
    }

    #[test]
    fn merge_dirty_some_unions_with_existing() {
        let map: Arc<Mutex<HashMap<SharedBufferId, Option<PixelRect>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let id = SharedBufferId(2);
        merge_dirty(
            &map,
            id,
            Some(PixelRect {
                x: 0,
                y: 0,
                w: 5,
                h: 5,
            }),
        );
        merge_dirty(
            &map,
            id,
            Some(PixelRect {
                x: 10,
                y: 10,
                w: 5,
                h: 5,
            }),
        );
        let got = map.lock().unwrap().get(&id).copied().flatten();
        assert_eq!(
            got,
            Some(PixelRect {
                x: 0,
                y: 0,
                w: 15,
                h: 15
            })
        );
    }

    #[test]
    fn merge_dirty_some_then_full_becomes_full() {
        let map: Arc<Mutex<HashMap<SharedBufferId, Option<PixelRect>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let id = SharedBufferId(3);
        merge_dirty(
            &map,
            id,
            Some(PixelRect {
                x: 0,
                y: 0,
                w: 5,
                h: 5,
            }),
        );
        merge_dirty(&map, id, None);
        assert_eq!(map.lock().unwrap().get(&id).copied(), Some(None));
    }
}
