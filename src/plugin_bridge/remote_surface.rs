//! Plugin이 제공하는 surface를 본체 layout에 끼울 수 있는 stand-in.
//!
//! webview-kind(html) surface 의 실 vehicle — host 는 URL/navigation chrome 만
//! 그리고 콘텐츠는 native WebView overlay 가 담당한다. (UiNode tree 렌더 경로는
//! C1 에서 제거됨.)
//!
//! `Surface` trait의 `kind() -> &'static str` 제약 때문에 plugin manifest의
//! 동적 kind 문자열은 `register_remote_kind`에서 `Box::leak`으로 한 번 정적화한다
//! (plugin 등록 시 1회, 메모리 누수는 plugin 종류 수만큼이라 무시 가능).
//!
//! 일부 메서드는 `SurfaceHandles`를 통한 외부 접근용 surface로 노출돼 있고,
//! 호스트 본문이 직접 호출하지는 않는다 (pump가 핸들 Arc로 직접 동기화) —
//! 향후 view에서 사용 가능하도록 유지.

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::model::{Surface, SurfaceId};
use serde_json::Value;

/// webview surface 의 navigation 생명주기 상태. native backend 콜백(WebView2 /
/// WKNavigationDelegate / WebKitGTK)이 갱신하고, host 가 chrome(loading/error) 렌더와
/// overlay 가시성 게이팅에 쓴다.
///
/// 정의 위치가 webview 모듈이 아니라 여기인 이유: `crate::webview`(=`host_api::webview`)는
/// `#[cfg(feature = "gui")]` 게이트라, webview 안에 두면 비-gui 빌드의 `RemoteSurface` 가
/// 참조할 수 없다. `RemoteSurface` 는 항상 컴파일되므로 NavState 도 여기 둔다. gui 코드
/// 편의용으로 `host_api/webview.rs` 가 `pub use` 로 재노출한다.
///
/// `Default = Idle` + `Copy` 라 native backend 의 `Rc<Cell<NavState>>` 에 그대로 들어간다
/// (실패 사유 문자열은 담지 않음 — backend 콜백이 `tracing::warn!` 로그로만 남기고 화면엔
/// URL 을 쓴다).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavState {
    /// 아직 navigation 시작 전(URL 미지정 직후). placeholder/boundary chrome.
    #[default]
    Idle,
    /// navigation 진행 중. overlay 숨기고 egui spinner 노출.
    Loading,
    /// navigation 성공 완료. overlay reveal(native 페이지가 보임).
    Done,
    /// navigation 실패. overlay 숨긴 채 error chrome. (사유는 tracing 로그로만)
    Failed,
}

pub struct RemoteSurface {
    pub id: SurfaceId,
    /// `Box::leak` 으로 정적화된 plugin kind. registry에 등록 시 한 번만 leak.
    pub kind_static: &'static str,
    pub plugin_id: String,
    /// snapshot 데이터 캐시 — plugin이 미리 보낸 값. 영속화 시 사용.
    pub snapshot_cache: Arc<Mutex<Option<Value>>>,
    /// surface가 plugin에 의해 invalidated 됐는지 — true면 호스트가 다음 프레임에 redraw.
    pub invalidated: Arc<Mutex<bool>>,
    /// 탭 제목 등에 표시되는 이름. plugin이 surface.create / event 응답에서 갱신 가능.
    pub display_name: Arc<Mutex<String>>,
    /// webview-enabled kind 인 경우 plugin 이 `webview.set_url` 로 전달한 URL.
    /// host 의 sync_webviews 가 매 프레임 이 값을 읽어 native webview 동기화.
    pub webview_url: Arc<Mutex<Option<String>>>,
    /// webview-enabled kind 의 navigation 생명주기 상태 mirror. host 의 sync_webviews 가
    /// 매 프레임 native `PlatformWebView.nav_state()` 를 이 값에 복사하고, egui 렌더 경로
    /// (egui_panels → webview_chrome)가 여기서 읽어 loading/error chrome 을 그린다.
    pub nav_state: Arc<Mutex<NavState>>,
    /// plugin 이 `surface.set_cwd` 로 통보한 현재 cwd. `source_cwd()` 가 이 값을
    /// 반환하여 다음 surface 의 carry 후보 cwd 로 사용된다 (예: explorer 가 root
    /// 변경 시 갱신). 초기값 None — host 가 SurfaceCreateCtx.cwd 로 받은 carry cwd
    /// 를 생성 직후 `set_cwd` 로 채워 넣는다.
    pub cwd: Arc<Mutex<Option<PathBuf>>>,
}

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
            invalidated: Arc::new(Mutex::new(true)),
            display_name: Arc::new(Mutex::new(initial_name)),
            webview_url: Arc::new(Mutex::new(None)),
            nav_state: Arc::new(Mutex::new(NavState::Idle)),
            cwd: Arc::new(Mutex::new(None)),
        }
    }

    /// `webview.set_url` IPC 가 호출 — webview-enabled kind 의 surface 만 의미 있음.
    pub fn set_webview_url(&self, url: Option<String>) {
        if let Ok(mut slot) = self.webview_url.lock() {
            *slot = url;
        }
    }

    /// sync_webviews 가 매 프레임 native nav_state 를 mirror 할 때 호출.
    pub fn set_nav_state(&self, s: NavState) {
        if let Ok(mut slot) = self.nav_state.lock() {
            *slot = s;
        }
    }

    /// 현재 mirror 된 navigation 상태. egui 렌더 경로가 chrome 분기에 읽는다.
    pub fn nav_state(&self) -> NavState {
        self.nav_state.lock().map(|g| *g).unwrap_or(NavState::Idle)
    }

    /// `surface.set_cwd` IPC 가 호출. plugin 측 root/working dir 변경을 host 에 통보.
    /// `Surface::source_cwd()` 가 다음 surface 의 carry 후보로 이 값을 노출.
    pub fn set_cwd(&self, cwd: Option<PathBuf>) {
        if let Ok(mut slot) = self.cwd.lock() {
            *slot = cwd;
        }
    }

    pub fn set_display_name(&self, name: String) {
        if let Ok(mut slot) = self.display_name.lock() {
            *slot = name;
        }
    }

    pub fn take_invalidated(&self) -> bool {
        match self.invalidated.lock() {
            Ok(mut slot) => {
                let was = *slot;
                *slot = false;
                was
            }
            Err(_) => false,
        }
    }

    pub fn mark_invalidated(&self) {
        if let Ok(mut slot) = self.invalidated.lock() {
            *slot = true;
        }
    }

    pub fn cache_snapshot(&self, data: Value) {
        if let Ok(mut slot) = self.snapshot_cache.lock() {
            *slot = Some(data);
        }
    }

    /// manager가 surface와 동기화하기 위해 필요한 핸들 묶음을 클론하여 반환.
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

    /// RemoteSurface 는 `cwd` 필드 (plugin 이 `surface.set_cwd` 로 갱신) 의 현재
    /// 값을 반환. 초기값은 host 가 생성 시점에 `set_cwd(SurfaceCreateCtx.cwd)` 로
    /// 채워둔 carry cwd. explorer 같이 root 가 *현재 폴더* 의 의미를 갖는 surface 는
    /// root 변경마다 `surface.set_cwd` 를 발사하여 이 값을 갱신한다. cwd 의미가
    /// 없는 다른 RemoteSurface kind 는 None 유지.
    fn source_cwd(&self) -> Option<std::path::PathBuf> {
        self.cwd.lock().ok().and_then(|g| g.clone())
    }

    fn display_name(&self) -> String {
        self.display_name
            .lock()
            .map(|n| n.clone())
            .unwrap_or_else(|_| self.kind_static.to_string())
    }

    fn webview_url(&self) -> Option<String> {
        self.webview_url.lock().ok().and_then(|g| g.clone())
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
mod tests {
    use super::*;

    #[test]
    fn invalidated_take_resets() {
        let s = RemoteSurface::new(1, "explorer", "com.x".into(), "Files".into());
        assert!(s.take_invalidated()); // 초기값 true
        assert!(!s.take_invalidated());
        s.mark_invalidated();
        assert!(s.take_invalidated());
    }

    #[test]
    fn surface_kind_returns_static() {
        let s = RemoteSurface::new(1, "explorer", "com.x".into(), "Files".into());
        assert_eq!(s.kind(), "explorer");
        assert_eq!(s.type_name(), "Remote");
        assert_eq!(s.display_name(), "Files");
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
