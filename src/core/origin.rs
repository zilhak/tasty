//! 요청 주체를 구분해 사용자 선택·복원 기록 정책에 사용한다.
//! 도메인이 GUI 큐 타입에 의존하지 않도록 여기서 정의하고 UI 쪽이 재사용한다.

/// User·Agent·System을 구별한다. OSC 등 자동 후속 처리는 System이며 사용자 조작으로 보지 않는다.
/// System은 도메인 요청에 사용하고 UI 조작을 자동으로 만드는 용도로 쓰지 않는다.
#[derive(Debug, Clone)]
pub enum IntentOrigin {
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

/// 사용자 요청 출처를 Debug 출력에 남기기 위한 값.
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

/// 에이전트 요청 채널과 plugin 식별자.
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

    #[allow(dead_code)]
    pub fn is_agent(&self) -> bool {
        matches!(self, IntentOrigin::Agent { .. })
    }

    #[allow(dead_code)]
    pub fn is_system(&self) -> bool {
        matches!(self, IntentOrigin::System)
    }
}

/// 파일 식별 worker를 거쳐 돌아올 때도 사용자 요청인지 구별하기 위한 값.
/// origin_surface_id는 대상 위치이며 요청 주체와는 별개다.
/// plugin이 확인된 클릭을 전달할 수도 있으므로 IPC로 왔다는 이유만으로 Agent로 정하지 않는다.
#[cfg_attr(
    not(feature = "gui"),
    expect(
        dead_code,
        reason = "every producer of a file dispatch origin is gui-only"
    )
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDispatchOrigin {
    /// 직접 사용자 조작 또는 host가 확인한 사용자 입력을 plugin이 중계한 요청.
    User,
    /// 에이전트의 파일 열기 요청.
    Agent,
}

impl FileDispatchOrigin {
    /// 사용자가 연 결과만 선택해 에이전트 요청이 사용자 포커스를 옮기지 않게 한다.
    #[cfg(feature = "gui")]
    pub(crate) fn selects_result(self) -> bool {
        matches!(self, Self::User)
    }
}

/// 명시된 surface의 pane을 찾는다. 대상이 사라졌으면 현재 선택 pane으로 대체하지 않고 거절한다.
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
