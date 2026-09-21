//! 요청·intent 를 **누가 발화했는가** — 사용자 / 에이전트 / 시스템 cascade.
//!
//! 정책 분기(포커스·닫은 항목 히스토리)의 판정 입력이라 도메인 실행과 GUI intent 큐가 함께
//! 읽는다. 그래서 정의는 도메인 쪽인 여기 있고, `crate::intent` 는 같은 이름을 재수출한다 —
//! 도메인이 GUI intent 큐 모듈을 거꾸로 부르지 않게 하려는 것이다.
//!
//! 파일 열기의 발화 주체([`FileDispatchOrigin`])와 그 요청이 가리킨 surface 의 소유 판정
//! ([`require_origin_pane`])도 같은 이유로 여기 있다 — 도메인의 `DispatchFile` intent 와
//! identify 포트가 쓰고, `crate::file::dispatch` 가 재수출한다.

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
    // 이유: 사용자 발화를 만드는 자리(단축키·메뉴·우클릭)가 GUI 뿐이다. headless 시험은 만든다.
    #[cfg_attr(
        all(not(feature = "gui"), not(test)),
        expect(dead_code, reason = "only the gui raises a user intent")
    )]
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
// 이유: `IntentOrigin::User` 와 같다 — 사용자 발화의 출처를 만드는 자리가 GUI 뿐이다.
#[cfg_attr(
    not(feature = "gui"),
    expect(dead_code, reason = "only the gui raises a user intent")
)]
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

/// 파일 열기를 **누가** 시작했는가. `origin_surface_id` 와 축이 다르다 — 저쪽은
/// *어디로* 가는가(라우팅)이고 이쪽은 *누가* 요청했는가다.
///
/// 이 값이 따로 있는 이유는 [`IntentOrigin`] 이 **비동기 식별 왕복을
/// 못 건너기** 때문이다. 파일 식별은 워커 스레드로 나갔다 `AppEvent::IdentifyDone` 으로
/// 돌아오고, 그 이벤트가 나르는 것은 발화 당시 intent 가 아니라 명시된 필드들뿐이다.
/// 그래서 `WorkspaceCloseOrigin` 과 같은 방식으로 **출처를 값으로 싣는다** — 사용자
/// 경로와 에이전트 경로의 차이를 하나의 값으로 표현하고 갈리는 부수효과를 거기서
/// 파생시킨다(`docs/design/policies/focus.md`).
///
/// **전송 채널이 아니라 행위의 성질로 정한다.** plugin 이 사용자의 클릭을 받아
/// `file_handler.dispatch` 로 보내는 경우가 있으므로(markdown 문서 안의 링크), "IPC 로
/// 들어왔는가" 는 이 값의 좌변이 아니다. 같은 기준을 `WorkspaceCloseOrigin` 이 이미
/// 쓴다 — 사용자 입력을 재현하는 debug IPC 를 `User` 로 친다.
// 이유: 이 값을 만드는 자리(explorer·링크·드롭·picker 확정·`file_handler.dispatch` arm)가 전부
// gui 에만 있다 — headless 는 파일 열기를 `-32017` 로 거절한다(ADR-0425). 정의를 cfg 로 가리지
// 않는 것은 도메인의 `DispatchFile` intent 가 headless 에서도 이 값을 싣고 타입체크를 받게 하려는
// 것이다. 이 값에 딸린 판정(`selects_result` · `require_origin_pane`)은 부르는 자리가 전부 GUI 라
// 항목마다 `cfg(feature = "gui")` 다.
#[cfg_attr(
    not(feature = "gui"),
    expect(
        dead_code,
        reason = "every producer of a file dispatch origin is gui-only"
    )
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDispatchOrigin {
    /// 사용자가 자기 손으로 열었다 — explorer 더블클릭 · 터미널 링크 클릭 · 파일 드롭 ·
    /// 파일 피커 확정.
    User,
    /// 에이전트가 release IPC/CLI(`file_handler.dispatch`)로 열었다.
    Agent,
}

impl FileDispatchOrigin {
    /// 결과 탭을 선택하는가. 사용자가 방금 그 자리에서 한 행동의 결과는 사용자가
    /// 보려고 연 것이므로 선택하고, 에이전트가 만든 것으로는 포커스를 옮기지 않는다
    /// ([ADR-0302](../../docs/adr/0302-a-user-file-open-selects-its-result-tab.md)).
    #[cfg(feature = "gui")]
    pub(crate) fn selects_result(self) -> bool {
        matches!(self, Self::User)
    }
}

/// 파일 열기 요청이 준 origin surface 의 pane. 준 origin 은 대상이지, 없을 때 포커스로
/// 물러날 허락이 아니다 — 없으면 거절한다.
#[cfg(feature = "gui")]
pub(crate) fn require_origin_pane(
    engine: &crate::core::CoreState,
    surface_id: u32,
) -> Result<u32, String> {
    engine.find_pane_for_surface(surface_id).ok_or_else(|| {
        crate::core::request_target::unowned_target_message(
            crate::core::request_target::ResourceId {
                kind: crate::core::request_target::Kind::Surface,
                id: u64::from(surface_id),
            },
            "file_handler.dispatch",
        )
    })
}
