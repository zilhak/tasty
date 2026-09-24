//! 플러그인 surface를 호스트 레이아웃에 보관한다.
//! WebView 콘텐츠는 네이티브 오버레이가 그리고 호스트는 주소·탐색 UI를 표시한다.
//! 이름과 snapshot은 매니저가 공유 핸들로 갱신한다.
//! GUI별 접근 범위는 docs/dev-guide/headless-build-boundaries.md를 따른다.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

#[cfg(any(feature = "gui", test))]
use crate::model::NavState;
use crate::model::{Surface, SurfaceId};
use serde_json::Value;

pub struct RemoteSurface {
    pub id: SurfaceId,
    /// 등록 시 프로세스 수명으로 할당한 kind 문자열.
    pub kind_static: &'static str,
    pub plugin_id: String,
    /// 플러그인에서 받은 snapshot. 저장·복원에 사용한다.
    pub snapshot_cache: Arc<Mutex<Option<Value>>>,
    /// 플러그인 응답으로 갱신하는 표시 이름.
    pub display_name: Arc<Mutex<String>>,
    /// webview.set_url로 받은 URL. sync_webviews가 네이티브 WebView에 반영한다.
    pub webview_url: Arc<Mutex<Option<String>>>,
    /// 현재 URL을 설정한 호출자가 surface 소유 플러그인인지 표시한다.
    /// 사용자 navigation 판정은 소유 플러그인이 설정한 페이지에서만 허용한다.
    #[cfg(feature = "gui")]
    pub webview_page_by_owner: Arc<AtomicBool>,
    /// 마지막 조회 뒤 URL 작성자가 외부 호출자에서 소유 플러그인으로 바뀌었는지 표시한다.
    /// 클릭 이후 페이지가 바뀌면 작성자를 잘못 판단할 수 있어 해당 프레임의 기록을 버린다.
    #[cfg(feature = "gui")]
    pub webview_owner_took_over: Arc<AtomicBool>,
    /// 네이티브 WebView의 탐색 상태 사본. 호스트 UI가 로딩·오류를 표시할 때 쓴다.
    #[cfg(any(feature = "gui", test))]
    pub nav_state: Arc<Mutex<NavState>>,
    /// 호스트가 보관하는 cwd. 생성 시 선언된 파일 경로나 상속 cwd로 채우고,
    /// surface.set_cwd로 갱신할 수 있다.
    pub cwd: Arc<Mutex<Option<PathBuf>>>,
}

// 필드별로 poison을 처음 한 번 기록한다. 값 하나를 교체하는 락이므로 기존 값을 복구한다.
// 정책: docs/dev-guide/error-handling.md.
pub(crate) static SNAPSHOT_POISON_REPORTED: AtomicBool = AtomicBool::new(false);
static DISPLAY_NAME_POISON_REPORTED: AtomicBool = AtomicBool::new(false);
static WEBVIEW_URL_POISON_REPORTED: AtomicBool = AtomicBool::new(false);
#[cfg(any(feature = "gui", test))]
static NAV_STATE_POISON_REPORTED: AtomicBool = AtomicBool::new(false);
static CWD_POISON_REPORTED: AtomicBool = AtomicBool::new(false);

pub(crate) const SNAPSHOT_WHAT: &str = "remote surface snapshot cache";
const DISPLAY_NAME_WHAT: &str = "remote surface display name";
const WEBVIEW_URL_WHAT: &str = "remote surface webview url";
#[cfg(any(feature = "gui", test))]
const NAV_STATE_WHAT: &str = "remote surface nav state";
const CWD_WHAT: &str = "remote surface cwd";

impl RemoteSurface {
    pub fn new(
        id: SurfaceId,
        kind_static: &'static str,
        plugin_id: String,
        initial_name: String,
    ) -> Self {
        Self {
            id,
            kind_static,
            plugin_id,
            snapshot_cache: Arc::new(Mutex::new(None)),
            display_name: Arc::new(Mutex::new(initial_name)),
            webview_url: Arc::new(Mutex::new(None)),
            #[cfg(feature = "gui")]
            webview_page_by_owner: Arc::new(AtomicBool::new(false)),
            #[cfg(feature = "gui")]
            webview_owner_took_over: Arc::new(AtomicBool::new(false)),
            #[cfg(any(feature = "gui", test))]
            nav_state: Arc::new(Mutex::new(NavState::Idle)),
            cwd: Arc::new(Mutex::new(None)),
        }
    }

    /// 상태 Arc를 공유해 같은 surface의 새 래퍼를 만든다. 플러그인에 생성 요청은 보내지 않는다.
    /// attach 트리를 다시 만들 때 기존 문서의 상태를 유지하는 데 사용한다.
    #[cfg(feature = "gui")]
    pub fn share_handles(&self) -> Self {
        Self {
            id: self.id,
            kind_static: self.kind_static,
            plugin_id: self.plugin_id.clone(),
            snapshot_cache: Arc::clone(&self.snapshot_cache),
            display_name: Arc::clone(&self.display_name),
            webview_url: Arc::clone(&self.webview_url),
            webview_page_by_owner: Arc::clone(&self.webview_page_by_owner),
            webview_owner_took_over: Arc::clone(&self.webview_owner_took_over),
            nav_state: Arc::clone(&self.nav_state),
            cwd: Arc::clone(&self.cwd),
        }
    }

    /// URL과 작성자를 갱신한다. by_owner는 호출자가 이 surface의 소유 플러그인인지 나타낸다.
    #[cfg(feature = "gui")]
    pub fn set_webview_url(&self, url: Option<String>, by_owner: bool) {
        let mut slot = crate::poison::recover_mutex(
            self.webview_url.lock(),
            WEBVIEW_URL_WHAT,
            &WEBVIEW_URL_POISON_REPORTED,
        );
        *slot = url;
        // URL 변경과 함께 작성자를 기록하고, 작성자보다 전이 표지를 먼저 저장한다.
        let was_owner = self
            .webview_page_by_owner
            .load(std::sync::atomic::Ordering::Acquire);
        if by_owner && !was_owner {
            self.webview_owner_took_over
                .store(true, std::sync::atomic::Ordering::Release);
        }
        self.webview_page_by_owner
            .store(by_owner, std::sync::atomic::Ordering::Release);
    }

    /// 외부 작성자에서 소유 플러그인으로 바뀐 표지를 반환하고 지운다.
    /// sync_webviews가 해당 프레임의 navigation을 반영한 뒤 호출한다.
    #[cfg(feature = "gui")]
    pub fn take_webview_owner_takeover(&self) -> bool {
        self.webview_owner_took_over
            .swap(false, std::sync::atomic::Ordering::AcqRel)
    }

    #[cfg(feature = "gui")]
    pub fn webview_page_by_owner(&self) -> bool {
        self.webview_page_by_owner
            .load(std::sync::atomic::Ordering::Acquire)
    }

    #[cfg(any(feature = "gui", test))]
    pub fn set_nav_state(&self, s: NavState) {
        *crate::poison::recover_mutex(
            self.nav_state.lock(),
            NAV_STATE_WHAT,
            &NAV_STATE_POISON_REPORTED,
        ) = s;
    }

    #[cfg(any(feature = "gui", test))]
    pub fn nav_state(&self) -> NavState {
        *crate::poison::recover_mutex(
            self.nav_state.lock(),
            NAV_STATE_WHAT,
            &NAV_STATE_POISON_REPORTED,
        )
    }

    pub fn set_cwd(&self, cwd: Option<PathBuf>) {
        *crate::poison::recover_mutex(self.cwd.lock(), CWD_WHAT, &CWD_POISON_REPORTED) = cwd;
    }

    #[cfg(test)]
    pub fn set_display_name(&self, name: String) {
        *crate::poison::recover_mutex(
            self.display_name.lock(),
            DISPLAY_NAME_WHAT,
            &DISPLAY_NAME_POISON_REPORTED,
        ) = name;
    }

    pub fn cache_snapshot(&self, data: Value) {
        *crate::poison::recover_mutex(
            self.snapshot_cache.lock(),
            SNAPSHOT_WHAT,
            &SNAPSHOT_POISON_REPORTED,
        ) = Some(data);
    }

    pub fn handles(&self) -> crate::plugin_bridge::host_cmd::SurfaceHandles {
        crate::plugin_bridge::host_cmd::SurfaceHandles {
            display_name: self.display_name.clone(),
            snapshot_cache: self.snapshot_cache.clone(),
        }
    }
}

impl Surface for RemoteSurface {
    tasty_model::impl_surface_any!();

    fn kind(&self) -> &'static str {
        self.kind_static
    }

    fn type_name(&self) -> &'static str {
        "Remote"
    }

    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }

    /// 호스트에 보관된 cwd를 반환한다. 로컬 실행에 사용하기 전에는 출처를 확인해야 한다.
    fn source_cwd(&self) -> Option<std::path::PathBuf> {
        crate::poison::recover_mutex(self.cwd.lock(), CWD_WHAT, &CWD_POISON_REPORTED).clone()
    }

    fn display_name(&self) -> String {
        crate::poison::recover_mutex(
            self.display_name.lock(),
            DISPLAY_NAME_WHAT,
            &DISPLAY_NAME_POISON_REPORTED,
        )
        .clone()
    }

    fn webview_url(&self) -> Option<String> {
        crate::poison::recover_mutex(
            self.webview_url.lock(),
            WEBVIEW_URL_WHAT,
            &WEBVIEW_URL_POISON_REPORTED,
        )
        .clone()
    }

    /// attach 원문 전달 후보의 kind·소유 플러그인·snapshot의 file 경로를 반환한다.
    /// 실제 허용 여부는 attach_runtime::content_mirror_candidates가 별도로 판단한다.
    fn attach_content_info(&self) -> Option<(&str, &str, Option<PathBuf>)> {
        let file = crate::poison::recover_mutex(
            self.snapshot_cache.lock(),
            SNAPSHOT_WHAT,
            &SNAPSHOT_POISON_REPORTED,
        )
        .as_ref()
        .and_then(|v| v.get("file"))
        .and_then(|v| v.as_str())
        .map(PathBuf::from);
        Some((self.kind_static, self.plugin_id.as_str(), file))
    }

    fn to_tree_json(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": self.kind_static,
            "type": "Remote",
            "id": self.id,
            "plugin_id": self.plugin_id,
            "display_name": self.display_name(),
        })
    }
}

#[cfg(test)]
// reason: 시험 코드의 Result 무시는 사유 주석 대상에서 제외한다.
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;

    #[test]
    fn surface_kind_returns_static() {
        let s = RemoteSurface::new(1, "explorer", "com.x".into(), "Files".into());
        assert_eq!(s.kind(), "explorer");
        assert_eq!(s.type_name(), "Remote");
        assert_eq!(s.display_name(), "Files");
    }

    // poison 뒤에도 기존 이름과 탐색 상태를 읽고 갱신할 수 있어야 한다.
    #[test]
    fn poisoned_mirrors_still_read_their_values() {
        let s = RemoteSurface::new(1, "explorer", "com.x".into(), "Files".into());
        s.set_nav_state(NavState::Loading);

        let name_lock = Arc::clone(&s.display_name);
        // 이유: poison을 만들기 위해 발생시킨 패닉이므로 join 오류를 무시한다.
        let _ = std::thread::spawn(move || {
            let _guard = name_lock.lock().expect("fresh lock");
            panic!("poison the display name mirror on purpose");
        })
        .join();
        let nav_lock = Arc::clone(&s.nav_state);
        // 이유: 같은 방식으로 탐색 상태 락의 poison을 만든다.
        let _ = std::thread::spawn(move || {
            let _guard = nav_lock.lock().expect("fresh lock");
            panic!("poison the nav state mirror on purpose");
        })
        .join();
        assert!(s.display_name.is_poisoned() && s.nav_state.is_poisoned());

        assert_eq!(s.display_name(), "Files");
        assert_eq!(s.nav_state(), NavState::Loading);
        s.set_display_name("Browser".into());
        assert_eq!(s.display_name(), "Browser");
    }

    #[test]
    fn set_display_name_updates() {
        let s = RemoteSurface::new(1, "explorer", "com.x".into(), "Files".into());
        s.set_display_name("Browser".into());
        assert_eq!(s.display_name(), "Browser");
    }

    #[test]
    fn initial_cwd_is_none_and_source_cwd_returns_none() {
        let s = RemoteSurface::new(1, "explorer", "com.x".into(), "Files".into());
        assert_eq!(s.source_cwd(), None);
    }

    #[test]
    fn set_cwd_then_source_cwd_returns_path() {
        let s = RemoteSurface::new(1, "explorer", "com.x".into(), "Files".into());
        let p = PathBuf::from("/tmp/foo");
        s.set_cwd(Some(p.clone()));
        assert_eq!(s.source_cwd(), Some(p));
        s.set_cwd(None);
        assert_eq!(s.source_cwd(), None);
    }
}
