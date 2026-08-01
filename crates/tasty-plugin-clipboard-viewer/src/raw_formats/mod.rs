//! 플랫폼별 raw 클립보드 포맷 열거("기타" 버킷).
//!
//! arboard 는 이 열거를 노출하지 않는다 — `Error::ContentNotAvailable`의 doc comment
//! (`arboard-3.6.1/src/common.rs:20-24`)가 "클립보드가 비어있는 것과 요청한 포맷이
//! 아닌 것을 구분하지 않는다"고 명시한다. 즉 arboard 의 `get_text`/`get_image`/
//! `get().html()`/`get().file_list()`가 전부 실패해도 "완전히 비었다"와 "이 4개가
//! 아닌 뭔가 있다"를 구분할 수 없다 — 플랫폼 raw API 를 직접 호출해야 한다.
//!
//! 세 서브모듈(windows/macos/x11)은 전부 arboard 가 실제로 읽는 semantic 포맷의
//! 변형(예: Windows `CF_TEXT`/`CF_UNICODETEXT`/`CF_OEMTEXT` 전부가 "텍스트")을 자체
//! 매핑 테이블로 제외하고 나머지만 raw 로 읽는다(단일 ID 비교 금지 — text/html 이
//! 동시에 클립보드에 있는 흔한 경우에도 "기타"에 중복으로 잡히지 않아야 한다).
//! macOS/Linux 는 이미 arboard 가 transitive dependency 로 끌어온 `objc2-app-kit`/
//! `x11rb` 를 semver 호환 버전으로 직접 의존성 선언해 동일 인스턴스로 재사용한다
//! (`cargo tree -d` 로 중복 버전 부재를 검증).

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
mod macos;

// wayland-data-control feature 를 켜지 않아(text/image/html 도 이미 이 경로) Linux 는
// X11/XWayland 백엔드만 쓴다 — 순수 Wayland(XWayland 미실행)에서는 연결 자체가
// 실패해 빈 벡터 + debug 로그로 처리한다(x11.rs 내부).
#[cfg(all(unix, not(target_os = "macos")))]
mod x11;

use crate::clipboard::OtherFormatEntry;

/// raw 포맷 1개당 실제로 읽어들이는 최대 바이트 수 — 과도하게 큰 포맷(대용량
/// 바이너리 등)을 통째로 메모리에 올리지 않기 위한 데이터 계층 안전판. 미리보기
/// 표시 줄 수 상한(`view::OTHER_PREVIEW_MAX_LINES`)과는 별개다(그건 표시 계층 절삭).
pub(crate) const MAX_RAW_BYTES: usize = 16 * 1024;

/// 현재 클립보드에서 text/files/image/html 가 아닌 나머지 포맷을 전부 열거해
/// 반환한다. 플랫폼이 지원되지 않거나(예: 순수 Wayland 세션의 X11 연결 실패, 또는
/// 이 셋 다 아닌 예외적 빌드 타깃) 조회 자체가 안 되면 빈 벡터 — 호출부
/// (`clipboard::read_other`)가 이를 "Other 타입 자체가 없음"으로 처리한다(빈
/// 클립보드와 조회 실패를 구분해야 할 필요가 있다면 서브모듈이 개별적으로
/// `tracing::debug!` 로 남긴다).
pub(crate) fn read_other() -> Vec<OtherFormatEntry> {
    #[cfg(target_os = "windows")]
    {
        windows::read_other()
    }
    #[cfg(target_os = "macos")]
    {
        macos::read_other()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        x11::read_other()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
    {
        Vec::new()
    }
}
