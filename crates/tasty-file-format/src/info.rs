//! 파일 핸들러가 규칙의 내부 타입 없이 detector 메타데이터를 읽는 trait.
//! 확장자 목록에는 Extension 규칙만 포함하며 magic·Lua·glob의 매칭 결과로 추론하지 않는다.

use super::types::DetectorId;

/// detector의 메타데이터를 조회한다.
pub trait DetectorInfo: Send + Sync {
    /// Extension 규칙의 확장자 목록. 소문자이며 '.'은 제외한다.
    /// 없으면 빈 목록이고, 비활성 detector의 선언도 반환한다.
    fn advertised_extensions(&self, detector: &DetectorId) -> Vec<String>;

    /// 활성 detector 중 해당 확장자를 선언한 ID. 설치 순서, ID 순이며 우선순위 표 적용 전이다.
    fn detectors_for_extension(&self, ext: &str) -> Vec<DetectorId>;

    /// 모든 광고된 확장자 (Settings UI 의 Extension Mapping 탭이 사용).
    fn all_advertised_extensions(&self) -> Vec<String>;

    /// detector 가 현재 enabled 인지. 존재하지 않으면 `false`.
    fn is_enabled(&self, detector: &DetectorId) -> bool;
}
