//! `DetectorInfo` trait — file_handler 등 외부 모듈이 file_format 의 rule 내부 타입을
//! import 하지 않고 detector 메타 (확장자 광고, enabled 여부) 를 조회하기 위한 추상화.
//!
//! 유일한 구현체는 `FileFormatRegistry` 지만 trait 으로 노출해 단방향 의존 (file_handler
//! → file_format) 을 유지한다. file_handler 는 `Arc<dyn DetectorInfo>` 만 보유한다.
//!
//! **자기-광고 패턴 (self-advertising)**: detector 가 실제로 매칭 가능한 확장자라도
//! `kind = "extension"` rule 로 명시 광고하지 않으면 `advertised_extensions` 가 반환하지
//! 않는다. magic / lua / glob 매칭은 광고로 간주하지 않는다.

use super::types::DetectorId;

/// file_format 의 rule 내부를 모르고도 detector 메타를 조회해야 하는 모듈이 사용하는
/// 추상화.
pub trait DetectorInfo: Send + Sync {
    /// `detector` 가 광고한 확장자 목록 (소문자, '.' 제외).
    /// Extension rule 만 추출. magic / glob / lua 는 포함하지 않는다.
    /// detector 가 존재하지 않거나 disabled 여도 빈 벡터 반환 (`is_enabled` 와 무관).
    fn advertised_extensions(&self, detector: &DetectorId) -> Vec<String>;

    /// 광고된 확장자 → 그 확장자를 광고한 detector id 들 (install_order 오름차순,
    /// tie-break 으로 id 사전순). 우선순위 표 적용 전의 raw 순서.
    /// disabled detector 는 포함하지 않는다.
    fn detectors_for_extension(&self, ext: &str) -> Vec<DetectorId>;

    /// 모든 광고된 확장자 (Settings UI 의 Extension Mapping 탭이 사용).
    fn all_advertised_extensions(&self) -> Vec<String>;

    /// detector 가 현재 enabled 인지. 존재하지 않으면 `false`.
    fn is_enabled(&self, detector: &DetectorId) -> bool;
}
