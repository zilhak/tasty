//! Modeless 에디터 계열 윈도우 trait.
//!
//! 현재 구현체: `PresetWindow`. 미래 후보: 키바인딩 에디터, 테마 에디터 등.
//!
//! 모달 (`ModalWindow`) 과 달리:
//! - 다른 윈도우 입력을 차단하지 않음
//! - Esc 자동 닫기 없음
//! - 별도 엔진 전역 단일 인스턴스 제약은 host (App) 측이 관리
//!
//! 단순 supertrait — `Window` 를 통한 다운캐스트 hook 만 제공한다.

use crate::window::{Modality, Window};

pub trait EditorWindow: Window {}

pub const EDITOR_MODALITY: Modality = Modality::Modeless;
