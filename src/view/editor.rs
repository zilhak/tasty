//! Modeless 에디터 계열 윈도우 trait.
//!
//! 현재 구현체: `PresetView`. 미래 후보: 키바인딩 에디터, 테마 에디터 등.
//!
//! 모달 (`ModalView`) 과 달리:
//! - 다른 윈도우 입력을 차단하지 않음
//! - Esc 자동 닫기 없음
//! - 별도 엔진 전역 단일 인스턴스 제약은 host (App) 측이 관리
//!
//! 단순 supertrait — `View` 를 통한 다운캐스트 hook 만 제공한다.

use crate::view::Modality;
use crate::view::ui::View;

/// `impl EditorView for PresetView {}` 가 존재하지만 trait object 사용 0.
/// 도메인 계열 표현(`docs/concepts/ubiquitous-language.md`)과 미래 에디터(키바인딩/테마)
/// placeholder로 보존.
#[allow(dead_code)]
pub(crate) trait EditorView: View {}

#[allow(dead_code)]
pub(crate) const EDITOR_MODALITY: Modality = Modality::Modeless;
