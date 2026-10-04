//! html surface의 문서 단위 스크립트 허용(ADR-0053)을 backend navigation 콜백에 연결한다.
//!
//! 상태는 surface 모델이 가진 [`HtmlScriptState`]와 공유한다. backend는 main frame 신호가 올 때
//! 아래 메서드를 부르고 돌려받은 JS 값을 즉시 적용한다. redraw에서 사후에 바꾸지 않는다.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard};

use tasty_model::html_script::scan::{SCAN_LIMIT_BYTES, scan_reader};
use tasty_model::html_script::{HtmlScriptState, ScriptScan, strip_fragment};

static STATE_POISON_REPORTED: AtomicBool = AtomicBool::new(false);
const STATE_WHAT: &str = "html script state";

/// 한 webview의 스크립트 허용 판단. 복제본은 같은 상태를 가리킨다.
#[derive(Clone)]
pub struct ScriptGate {
    surface_id: u32,
    state: Arc<Mutex<HtmlScriptState>>,
}

impl ScriptGate {
    pub fn new(surface_id: u32, state: Arc<Mutex<HtmlScriptState>>) -> Self {
        Self { surface_id, state }
    }

    fn lock(&self) -> MutexGuard<'_, HtmlScriptState> {
        tasty_utils::poison::recover_mutex(self.state.lock(), STATE_WHAT, &STATE_POISON_REPORTED)
    }

    /// 지금 적용할 JS 값.
    // 이유: macOS는 전역 JS를 켜 두고 탐색 단위 preferences로 문서 JS를 정해 이 값을 쓰지 않는다.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
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

    /// main frame main resource의 응답 결정. `file://` 문서는 여기서 원본을 한 번 읽는다.
    pub fn main_response(&self, url: &str) -> bool {
        // 파일 읽기는 잠금 밖에서 한다.
        let scan = scan_document_url(self.surface_id, url);
        let js = self.lock().on_main_response(url, scan);
        tracing::debug!(
            "WebView surface {}: html script response {} detection={:?} -> js={js}",
            self.surface_id,
            strip_fragment(url),
            scan.map(|s| s.detection)
        );
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

/// `file://` URL이면 원본을 읽어 스캔한다. 읽지 못하면 지문이 없는 문서가 된다(허용 불가).
fn scan_document_url(surface_id: u32, url: &str) -> Option<ScriptScan> {
    let path = file_url_to_path(url)?;
    match scan_file(&path) {
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

    #[test]
    fn the_gate_reads_the_file_at_the_response_and_allows_the_seen_content() {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("tasty-script-gate-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("doc.html");
        std::fs::write(&path, "<script>a()</script>").expect("write");
        let url = format!("file://{}", path.to_string_lossy().replace('\\', "/"));
        let url = if url.starts_with("file:///") {
            url
        } else {
            url.replacen("file://", "file:///", 1)
        };
        let gate = ScriptGate::new(1, Arc::new(Mutex::new(HtmlScriptState::new(true))));
        assert!(!gate.load_started());
        assert!(!gate.main_response(&url));
        assert!(!gate.committed(Some(&url)));
        assert_eq!(gate.finished(), None);
        gate.lock().allow_current().expect("allow");
        gate.load_started();
        assert!(gate.main_response(&url), "같은 내용의 재로드는 켠다");
        assert!(gate.committed(Some(&url)));
        assert!(gate.is_fragment_move(&format!("{url}#sec")));
        assert!(!gate.is_fragment_move(&url));
        // 허용 뒤 파일이 바뀌면 재로드에서 켜지 않는다(로드 때 지문과 다르다).
        std::fs::write(&path, "<script>b()</script>").expect("rewrite");
        gate.load_started();
        assert!(!gate.main_response(&url));
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            tracing::warn!("temp dir cleanup failed: {e}");
        }
    }
}
