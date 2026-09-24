//! webview surface 의 탐색(navigation) 생명주기 상태.

/// webview 탐색 상태. native backend가 갱신하고 호스트가 로딩·오류 UI와 표시 여부를 결정한다.
/// 모델과 backend·UI가 함께 쓰므로 OS·GUI 타입을 포함하지 않는다. 실패 사유는 별도 로그에 남긴다.
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
