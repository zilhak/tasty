//! webview surface 의 탐색(navigation) 생명주기 상태.

/// webview surface 의 navigation 생명주기 상태. native backend 콜백(WebView2 /
/// WKNavigationDelegate / WebKitGTK)이 갱신하고, host 가 chrome(loading/error) 렌더와
/// overlay 가시성 게이팅에 쓴다.
///
/// 정의 위치가 도메인 모델인 이유: 이 값은 **세 자리가 같이 쓰는 계약**이다 — 항상
/// 컴파일되는 `RemoteSurface`(비-gui 빌드에도 있다)가 mirror 로 담고, gui 뒤의 native
/// 백엔드가 쓰고, egui chrome 이 읽는다. 셋 중 어느 한쪽에 정의를 두면 나머지가 그쪽
/// 모듈 경로를 역참조하게 된다(한때 백엔드가 `plugin_bridge` 경로로 이 타입을 불렀다).
/// OS·webview·egui 타입을 하나도 담지 않는 네 값짜리 enum 이라 도메인 경계에 둬도 아무것도
/// 새지 않는다. 근거·대안·재검토 조건:
/// `docs/design/systems/webview.md#키보드--별도-계약`.
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
