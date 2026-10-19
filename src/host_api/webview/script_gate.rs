//! html surface의 문서 단위 스크립트 허용(docs/surfaces/html/index.md)을 backend navigation 콜백에 연결한다.
//!
//! 상태는 surface 모델이 가진 [`HtmlScriptState`]와 공유한다. backend는 main frame 신호가 올 때
//! 아래 메서드를 부르고 돌려받은 JS 값을 즉시 적용한다. redraw에서 사후에 바꾸지 않는다.
//!
//! 원본 스캔(파일 전체 지문)은 UI 스레드에서 하지 않는다. 결과와 관계없이 JS 값이 같은 응답은
//! 바로 결정하고 스캔은 작업 스레드에서 끝낸다. 허용 기록과 같은 URL의 응답만 스캔 결과가 JS 값을
//! 정한다. 그 결정은 backend가 미룰 수 있으면 미루고([`Response::Deferred`]), 미룰 수 없으면 그 자리에서 스캔한다.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard};

use tasty_model::html_script::scan::{SCAN_LIMIT_BYTES, scan_reader};
use tasty_model::html_script::{HtmlScriptState, ScanTicket, ScriptScan, strip_fragment};

static STATE_POISON_REPORTED: AtomicBool = AtomicBool::new(false);
const STATE_WHAT: &str = "html script state";

/// 작업 스레드의 스캔이 끝났을 때 호스트를 깨운다. 배너를 다시 그리고 미룬 결정을 진행하게 한다.
pub type ScanNotify = Arc<dyn Fn() + Send + Sync>;

/// 한 webview의 스크립트 허용 판단. 복제본은 같은 상태를 가리킨다.
#[derive(Clone)]
pub struct ScriptGate {
    surface_id: u32,
    state: Arc<Mutex<HtmlScriptState>>,
    notify: Option<ScanNotify>,
}

/// main frame 응답의 JS 결정.
pub enum Response {
    /// 지금 적용할 JS 값.
    Decided(bool),
    /// 스캔 결과가 JS 값을 정한다. backend는 응답 결정을 보류하고 [`DeferredScan::run`]을 작업
    /// 스레드에서 실행한 뒤 그 값으로 결정을 끝낸다.
    // 이유: 결정을 미룰 수 있는 backend(Linux)만 이 값을 받는다.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Deferred(DeferredScan),
}

/// 결정을 보류한 응답의 스캔.
pub struct DeferredScan {
    gate: ScriptGate,
    ticket: ScanTicket,
    path: PathBuf,
}

impl DeferredScan {
    /// 파일을 읽어 결과를 붙인다. UI 스레드가 아닌 곳에서 부른다. 적용할 JS 값을 주고, 그 사이 새 로드가
    /// 시작돼 결과가 낡았으면 `None`을 준다(호출자는 보류한 결정을 거절한다).
    // 이유: 결정을 미룰 수 있는 backend(Linux)만 부른다.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn run(self) -> Option<bool> {
        let scan = scan_path(self.gate.surface_id, &self.path);
        self.gate.finish_scan(self.ticket, scan)
    }
}

impl ScriptGate {
    pub fn new(surface_id: u32, state: Arc<Mutex<HtmlScriptState>>) -> Self {
        Self {
            surface_id,
            state,
            notify: None,
        }
    }

    /// 작업 스레드의 스캔이 끝났을 때 부를 함수를 붙인다.
    pub fn with_notify(mut self, notify: ScanNotify) -> Self {
        self.notify = Some(notify);
        self
    }

    fn lock(&self) -> MutexGuard<'_, HtmlScriptState> {
        tasty_utils::poison::recover_mutex(self.state.lock(), STATE_WHAT, &STATE_POISON_REPORTED)
    }

    /// 지금 적용할 JS 값. macOS는 서브프레임 navigation의 preferences에 쓴다.
    pub fn effective_js(&self) -> bool {
        self.lock().effective_js()
    }

    /// 전역 sandbox 설정이 바뀌었다.
    pub fn set_sandbox(&self, sandbox: bool) -> bool {
        let js = self.lock().set_sandbox(sandbox);
        tracing::debug!(
            "WebView surface {}: html script sandbox={sandbox} -> js={js}",
            self.surface_id
        );
        js
    }

    /// main frame 로드가 시작됐다.
    pub fn load_started(&self) -> bool {
        let js = self.lock().on_load_started();
        tracing::debug!(
            "WebView surface {}: html script load started -> js={js}",
            self.surface_id
        );
        js
    }

    /// main frame main resource의 응답 결정. 결정을 미룰 수 없는 backend가 쓴다.
    /// 허용 기록과 같은 URL의 응답만 그 자리에서 원본을 읽는다. 나머지는 작업 스레드가 읽는다.
    // 이유: Linux는 결정을 미룰 수 있어 main_response_deferrable을 쓴다(시험은 이 경로도 쓴다).
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub fn main_response(&self, url: &str) -> bool {
        match self.respond(url, false) {
            Response::Decided(js) => js,
            Response::Deferred(scan) => scan.run().unwrap_or(false),
        }
    }

    /// main frame main resource의 응답 결정. 결정을 미룰 수 있는 backend가 쓴다.
    // 이유: 결정을 미룰 수 있는 backend(Linux)만 부른다.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn main_response_deferrable(&self, url: &str) -> Response {
        self.respond(url, true)
    }

    fn respond(&self, url: &str, defer: bool) -> Response {
        let Some(path) = file_url_to_path(url) else {
            return Response::Decided(self.respond_without_file(url));
        };
        let mut st = self.lock();
        let needs_scan = st.needs_scan_before_js(url);
        if needs_scan && !defer {
            drop(st);
            return Response::Decided(self.respond_in_place(url, &path));
        }
        let (js, ticket) = st.on_main_response_pending(url);
        drop(st);
        if needs_scan {
            tracing::debug!(
                "WebView surface {}: html script response {} deferred until the scan ends",
                self.surface_id,
                strip_fragment(url)
            );
            return Response::Deferred(DeferredScan {
                gate: self.clone(),
                ticket,
                path,
            });
        }
        tracing::debug!(
            "WebView surface {}: html script response {} scanning in the background -> js={js}",
            self.surface_id,
            strip_fragment(url)
        );
        self.spawn_scan(ticket, path);
        Response::Decided(js)
    }

    /// 로컬 파일이 아닌 문서는 지문 없이 기록한다(허용 불가).
    fn respond_without_file(&self, url: &str) -> bool {
        let js = self.lock().on_main_response(url, None);
        tracing::debug!(
            "WebView surface {}: html script response {} (not a local file) -> js={js}",
            self.surface_id,
            strip_fragment(url)
        );
        js
    }

    /// 결정을 미룰 수 없는 backend에서 스캔 결과가 JS 값을 정하는 응답. 그 자리에서 읽는다.
    fn respond_in_place(&self, url: &str, path: &std::path::Path) -> bool {
        // 파일 읽기는 잠금 밖에서 한다.
        let scan = scan_path(self.surface_id, path);
        let js = self.lock().on_main_response(url, scan);
        tracing::debug!(
            "WebView surface {}: html script response {} read in place detection={:?} -> js={js}",
            self.surface_id,
            strip_fragment(url),
            scan.map(|s| s.detection)
        );
        js
    }

    /// 작업 스레드에서 스캔한다. 스레드를 띄우지 못하면 그 자리에서 읽는다.
    fn spawn_scan(&self, ticket: ScanTicket, path: PathBuf) {
        let gate = self.clone();
        let fallback = (self.clone(), path.clone());
        let spawned = std::thread::Builder::new()
            .name("html-script-scan".into())
            .spawn(move || {
                let scan = scan_path(gate.surface_id, &path);
                gate.finish_scan(ticket, scan);
            });
        if let Err(e) = spawned {
            tracing::warn!(
                "WebView surface {}: html script scan thread not started ({e}); reading in place",
                self.surface_id
            );
            let (gate, path) = fallback;
            let scan = scan_path(gate.surface_id, &path);
            gate.finish_scan(ticket, scan);
        }
    }

    /// 스캔 결과를 상태에 붙이고 호스트를 깨운다. 낡은 결과면 `None`.
    fn finish_scan(&self, ticket: ScanTicket, scan: Option<ScriptScan>) -> Option<bool> {
        let js = self.lock().on_scan_finished(ticket, scan);
        tracing::debug!(
            "WebView surface {}: html script scan finished detection={:?} -> {}",
            self.surface_id,
            scan.map(|s| s.detection),
            match js {
                Some(js) => format!("js={js}"),
                None => "dropped (the load it belongs to was replaced)".to_string(),
            }
        );
        if let Some(notify) = &self.notify {
            notify();
        }
        js
    }

    /// main frame 문서가 commit됐다. `url`은 응답 단계가 없었을 때만 쓴다.
    pub fn committed(&self, url: Option<&str>) -> bool {
        let mut st = self.lock();
        let js = st.on_committed(url);
        tracing::debug!(
            "WebView surface {}: html script committed {:?} allowed={} -> js={js}",
            self.surface_id,
            st.current().map(|d| d.url.as_str()),
            st.current_is_allowed()
        );
        js
    }

    /// main frame 로드가 끝났다. commit 없이 끝났으면 되돌릴 JS 값을 준다.
    pub fn finished(&self) -> Option<bool> {
        let restore = self.lock().on_load_finished();
        if let Some(js) = restore {
            tracing::debug!(
                "WebView surface {}: html script load ended without a commit -> restore js={js}",
                self.surface_id
            );
        }
        restore
    }

    /// main frame 로드가 실패했다. 다음 commit까지 배너와 탭 표지를 내린다.
    /// 로드 종료는 [`Self::finished`]로 따로 알린다.
    pub fn failed(&self) {
        self.lock().on_load_failed();
        tracing::debug!(
            "WebView surface {}: html script load failed -> banner and marker hidden until a commit",
            self.surface_id
        );
    }

    /// 화면 문서와 같은 문서의 fragment 이동인지. Windows·macOS에서 로드 신호를 건너뛸 때 쓴다.
    // 이유: Linux는 fragment 이동에 로드 신호가 오지 않아 이 판정을 쓰지 않는다.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub fn is_fragment_move(&self, url: &str) -> bool {
        url.contains('#')
            && self
                .lock()
                .current()
                .is_some_and(|d| d.url == strip_fragment(url))
    }
}

/// 로컬 파일을 읽어 스캔한다. 읽지 못하면 지문이 없는 문서가 된다(허용 불가).
fn scan_path(surface_id: u32, path: &std::path::Path) -> Option<ScriptScan> {
    match scan_file(path) {
        Ok(scan) => Some(scan),
        Err(e) => {
            tracing::warn!(
                "WebView surface {surface_id}: html script scan failed for {}: {e} \
                 — the document cannot be allowed",
                path.display()
            );
            None
        }
    }
}

/// 정규 파일을 열어 [`scan_reader`]로 스캔한다.
///
/// FIFO·장치 파일은 읽기가 끝나지 않거나 쓰는 쪽을 기다리며 막힐 수 있어 거절한다.
/// 경로를 연 뒤 같은 핸들로 종류를 확인하므로 확인과 읽기 사이에 파일이 바뀌지 않는다.
fn scan_file(path: &std::path::Path) -> std::io::Result<ScriptScan> {
    let file = open_without_blocking(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    scan_reader(file, SCAN_LIMIT_BYTES)
}

/// FIFO를 열 때 쓰는 쪽을 기다리지 않도록 unix에서는 `O_NONBLOCK`으로 연다.
/// 정규 파일 읽기에는 영향이 없다.
#[cfg(unix)]
fn open_without_blocking(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_without_blocking(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

/// `file://` URL을 로컬 경로로 바꾼다. query·fragment는 버리고 percent escape를 바이트로 복원한다.
pub fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let rest = url
        .get(..7)
        .filter(|s| s.eq_ignore_ascii_case("file://"))
        .map(|_| &url[7..])?;
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let bytes = percent_decode(path)?;
    let host = if host.eq_ignore_ascii_case("localhost") {
        ""
    } else {
        host
    };
    path_from_parts(host, bytes)
}

fn percent_decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Some(out)
}

#[cfg(unix)]
fn path_from_parts(host: &str, bytes: Vec<u8>) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    // 다른 호스트의 파일은 로컬 경로로 읽지 않는다.
    if !host.is_empty() || bytes.first() != Some(&b'/') {
        return None;
    }
    Some(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

#[cfg(windows)]
fn path_from_parts(host: &str, bytes: Vec<u8>) -> Option<PathBuf> {
    let path = String::from_utf8(bytes).ok()?;
    if !host.is_empty() {
        // UNC: file://server/share/a.html -> \\server\share\a.html
        return Some(PathBuf::from(format!(
            r"\\{host}{}",
            path.replace('/', "\\")
        )));
    }
    // file:///C:/a.html -> C:\a.html
    let trimmed = path.strip_prefix('/').unwrap_or(&path);
    Some(PathBuf::from(trimmed.replace('/', "\\")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn file_urls_become_local_paths() {
        assert_eq!(
            file_url_to_path("file:///home/u/a.html"),
            Some(PathBuf::from("/home/u/a.html"))
        );
        assert_eq!(
            file_url_to_path("file://localhost/home/u/a.html?x=1#top"),
            Some(PathBuf::from("/home/u/a.html"))
        );
        assert_eq!(
            file_url_to_path("FILE:///home/u/My%20Doc%ED%95%9C.html"),
            Some(PathBuf::from("/home/u/My Doc한.html"))
        );
        assert_eq!(file_url_to_path("file://server/share/a.html"), None);
        assert_eq!(file_url_to_path("file:///bad%ZZ.html"), None);
        assert_eq!(file_url_to_path("https://example.com/a.html"), None);
        assert_eq!(file_url_to_path("about:blank"), None);
    }

    #[cfg(windows)]
    #[test]
    fn file_urls_become_windows_paths() {
        assert_eq!(
            file_url_to_path("file:///C:/Users/a%20b.html#x"),
            Some(PathBuf::from(r"C:\Users\a b.html"))
        );
        assert_eq!(
            file_url_to_path("file://server/share/a.html"),
            Some(PathBuf::from(r"\\server\share\a.html"))
        );
    }

    #[test]
    fn scan_file_reads_a_file_from_disk() {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("tasty-html-scan-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("doc.html");
        std::fs::write(&path, "<button onclick=x()>b</button>").expect("write");
        let s = scan_file(&path).expect("scan");
        assert_eq!(
            s.detection,
            tasty_model::html_script::ScriptDetection::Scripts
        );
        assert!(scan_file(&dir.join("missing.html")).is_err());
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            tracing::warn!("temp dir cleanup failed: {e}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn special_files_are_refused_without_blocking() {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("tasty-html-fifo-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let fifo = dir.join("pipe.html");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo");
        assert!(status.success(), "mkfifo");
        // 쓰는 쪽이 없는 FIFO를 막히는 방식으로 열면 이 테스트가 끝나지 않는다.
        let err = scan_file(&fifo).expect_err("fifo");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        let err = scan_file(std::path::Path::new("/dev/zero")).expect_err("device");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            tracing::warn!("temp dir cleanup failed: {e}");
        }
    }

    /// 작업 스레드의 스캔 완료를 기다릴 수 있는 gate와 문서 URL.
    fn gate_with_doc(
        tag: &str,
        body: &str,
    ) -> (ScriptGate, std::sync::mpsc::Receiver<()>, PathBuf, String) {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "tasty-script-gate-{tag}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("doc.html");
        std::fs::write(&path, body).expect("write");
        let url = format!("file://{}", path.to_string_lossy().replace('\\', "/"));
        let url = if url.starts_with("file:///") {
            url
        } else {
            url.replacen("file://", "file:///", 1)
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        let gate = ScriptGate::new(1, Arc::new(Mutex::new(HtmlScriptState::new(true))))
            .with_notify(Arc::new(move || {
                if let Err(e) = tx.lock().expect("sender").send(()) {
                    tracing::warn!("scan notify receiver gone: {e}");
                }
            }));
        (gate, rx, dir, url)
    }

    fn wait_scan(rx: &std::sync::mpsc::Receiver<()>) {
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("the background scan finished");
    }

    fn cleanup(dir: &std::path::Path) {
        if let Err(e) = std::fs::remove_dir_all(dir) {
            tracing::warn!("temp dir cleanup failed: {e}");
        }
    }

    #[test]
    fn the_gate_reads_the_file_off_the_caller_and_allows_the_seen_content() {
        let (gate, rx, dir, url) = gate_with_doc("allow", "<script>a()</script>");
        assert!(!gate.load_started());
        assert!(
            !gate.main_response(&url),
            "허용 기록이 없으면 결과를 기다리지 않고 끈다"
        );
        assert!(!gate.committed(Some(&url)));
        assert_eq!(gate.finished(), None);
        wait_scan(&rx);
        assert!(!gate.lock().scan_pending());
        gate.lock().allow_current().expect("allow");
        gate.load_started();
        // 허용한 URL은 결과가 JS 값을 정하므로 미룰 수 없는 경로에서는 그 자리에서 읽는다.
        assert!(gate.main_response(&url), "같은 내용의 재로드는 켠다");
        assert!(gate.committed(Some(&url)));
        assert!(gate.is_fragment_move(&format!("{url}#sec")));
        assert!(!gate.is_fragment_move(&url));
        // 허용 뒤 파일이 바뀌면 재로드에서 켜지 않는다(로드 때 지문과 다르다).
        std::fs::write(dir.join("doc.html"), "<script>b()</script>").expect("rewrite");
        gate.load_started();
        assert!(!gate.main_response(&url));
        cleanup(&dir);
    }

    #[test]
    fn a_deferred_response_is_decided_by_its_own_scan() {
        let (gate, rx, dir, url) = gate_with_doc("defer", "<script>a()</script>");
        gate.load_started();
        assert!(matches!(
            gate.main_response_deferrable(&url),
            Response::Decided(false)
        ));
        gate.committed(Some(&url));
        gate.finished();
        wait_scan(&rx);
        gate.lock().allow_current().expect("allow");
        // 같은 내용: 보류한 결정이 JS를 켠다.
        gate.load_started();
        let Response::Deferred(scan) = gate.main_response_deferrable(&url) else {
            panic!("an allowed URL defers its decision");
        };
        assert!(gate.lock().scan_pending());
        assert_eq!(
            std::thread::spawn(move || scan.run()).join().expect("join"),
            Some(true)
        );
        assert!(gate.committed(Some(&url)));
        gate.finished();
        // 결정을 보류한 사이 새 로드가 시작되면 늦은 결과는 버린다.
        gate.load_started();
        let Response::Deferred(stale) = gate.main_response_deferrable(&url) else {
            panic!("still allowed");
        };
        gate.load_started();
        assert_eq!(stale.run(), None);
        cleanup(&dir);
    }

    /// 감지는 상한까지, 지문은 파일 전체다. 상한 뒤에만 스크립트가 있는 큰 파일에서 두 값이 갈린다.
    #[test]
    fn a_large_file_is_fingerprinted_in_full_off_the_caller() {
        let mut body = vec![b' '; SCAN_LIMIT_BYTES + 1024];
        body.extend_from_slice(b"<script>tail()</script>");
        let (gate, rx, dir, url) = gate_with_doc("large", "");
        std::fs::write(dir.join("doc.html"), &body).expect("write large");
        gate.load_started();
        gate.main_response(&url);
        gate.committed(Some(&url));
        wait_scan(&rx);
        let first = gate.lock().current().and_then(|d| d.scan).expect("scan");
        assert_eq!(
            first.detection,
            tasty_model::html_script::ScriptDetection::None
        );
        body.extend_from_slice(b"<script>more()</script>");
        std::fs::write(dir.join("doc.html"), &body).expect("append");
        gate.load_started();
        gate.main_response(&url);
        gate.committed(Some(&url));
        wait_scan(&rx);
        let second = gate.lock().current().and_then(|d| d.scan).expect("scan");
        assert_ne!(
            first.fingerprint, second.fingerprint,
            "상한 뒤 변경도 지문에 들어간다"
        );
        cleanup(&dir);
    }
}
