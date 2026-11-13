//! 호스트가 플러그인 입력칸에 보내는 편집 요청(복사·잘라내기·붙여넣기)과 그 결과 회신.
//!
//! 호스트는 편집 키를 실제로 동작했을 때만 소비한다. 플러그인 상태는 다른 프로세스에 있으므로
//! 플러그인이 결과를 알려 주어야 한다. 계약은 `docs/dev-guide/plugin-development.md`의
//! "편집 결과 회신" 절에 있다.
//!
//! - 플러그인은 [`PluginEvent::Hello`](crate::PluginEvent::Hello)의 `capabilities`에
//!   [`CAPABILITY_EDIT_OUTCOME`]을 넣어 회신을 약속한다. 넣지 않은 플러그인에는 호스트가
//!   요청을 보내지 않는다.
//! - 요청은 입력 이벤트 [`RawInputEventWire::EditRequest`](crate::RawInputEventWire::EditRequest)로
//!   오고, 플러그인은 요청마다 [`PluginEvent::EditOutcome`](crate::PluginEvent::EditOutcome)을
//!   정확히 한 번 보낸다.

use serde::{Deserialize, Serialize};

/// 편집 결과 회신을 지원한다는 `Hello.capabilities` 값.
pub const CAPABILITY_EDIT_OUTCOME: &str = "edit_outcome";

/// 편집 요청의 동작.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "a", rename_all = "snake_case")]
pub enum EditRequestWire {
    /// 포커스 위젯의 선택을 복사한다.
    Copy,
    /// 포커스 입력칸의 선택을 잘라낸다.
    Cut,
    /// 호스트가 OS 클립보드에서 읽은 텍스트를 포커스 입력칸에 붙여넣는다.
    PasteText { text: String },
    /// 내용 없는 형식 붙여넣기. 플러그인이 필요한 형식(이미지 등)을 직접 읽는다.
    Paste,
    /// 모르는 동작. 플러그인은 실행하지 않고 [`EditResultWire::NotActed`]로 회신한다.
    #[serde(other)]
    Unknown,
}

/// 편집 요청의 결과.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EditResultWire {
    /// 편집이 실제로 일어났다. 호스트는 원래 키를 소비한다.
    Acted,
    /// 편집이 일어나지 않았다. 호스트는 원래 키를 다음 처리자에 넘긴다.
    NotActed,
    /// 플러그인이 결과를 알 수 없다(실행됐을 수 있다). 호스트는 원래 키를 소비하고 재생하지 않는다.
    /// 모르는 값도 이것으로 읽는다.
    #[serde(other)]
    Unknown,
}
