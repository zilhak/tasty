//! 요청·intent 를 **누가 발화했는가** — 사용자 / 에이전트 / 시스템 cascade.
//!
//! 정책 분기(포커스·닫은 항목 히스토리)의 판정 입력이라 도메인 실행과 GUI intent 큐가 함께
//! 읽는다. 그래서 정의는 도메인 쪽인 여기 있고, `crate::intent` 는 같은 이름을 재수출한다 —
//! 도메인이 GUI intent 큐 모듈을 거꾸로 부르지 않게 하려는 것이다.

/// Intent 를 발화한 주체. 핸들러가 정책 분기에 사용.
///
/// `System` variant 는 *사용자도 에이전트도 아닌 시스템 내부 cascade* 를 표현 —
/// PTY 가 출력한 escape sequence 가 trigger 한 cascade (OSC 9 알림, OSC 7 cwd
/// 변경, OSC 52 클립보드 등) 와 같이 *사용자/에이전트의 직접 발화가 아닌
/// 자동 cascade*. focus 정책상 `User` 도 `Agent` 도 아닌 *제3 카테고리* —
/// focus 가져가지 않고, closed-tab restore 스택에도 push 하지 않는다 (기존
/// `is_user()` 가 false 인 경로와 동일 동작).
///
/// `System` 발화는 *Domain Intent 한정* — UI Intent (`Intent::Ui`) 의 자동
/// 발화는 release 표면에서 금지되므로 `UiIntent` 위에는 `from_system()` 을
/// 두지 않는다 (`popup-system.md` "Popup 발화 정책").
///
/// `source` 페이로드는 audit/debug trace 용 — match arm 에서 destructure 하지
/// 않으나 `Debug` derive 로 노출 — 아직 이 값을 읽는 소비처는 없다.
#[derive(Debug, Clone)]
pub enum IntentOrigin {
    User {
        #[allow(dead_code)]
        source: UserSource,
    },
    Agent {
        #[allow(dead_code)]
        source: AgentSource,
    },
    System,
}

/// 사용자 발화의 정확한 origin (shortcut id, menu id 등). 페이로드는 audit
/// trace 전용으로 destructure 되지 않으나 Debug 출력에 노출.
#[derive(Debug, Clone)]
pub enum UserSource {
    Shortcut(#[allow(dead_code)] &'static str),
    Menu(#[allow(dead_code)] &'static str),
    ContextMenu,
}

/// 에이전트 발화의 channel + plugin id. audit trace 용 — Plugin/Cli variant
/// 자체는 추후 plugin/CLI dispatch chain 통합 시 활성화 예정 (E.B 영역).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum AgentSource {
    Ipc,
    Plugin(String),
    Cli,
}

impl IntentOrigin {
    pub fn is_user(&self) -> bool {
        matches!(self, IntentOrigin::User { .. })
    }

    /// audit/branch helper — origin 분기 시 IntentOrigin 패턴 매칭 대신 사용.
    #[allow(dead_code)]
    pub fn is_agent(&self) -> bool {
        matches!(self, IntentOrigin::Agent { .. })
    }

    /// `is_agent` 와 짝인 audit/branch helper — origin 분기 호출처 추가 시 사용.
    #[allow(dead_code)]
    pub fn is_system(&self) -> bool {
        matches!(self, IntentOrigin::System)
    }
}
