//! 한 engine이 소유하는 실제 Terminal·PTY 원본 컬렉션.
//! CoreState의 논리 세션 사실(workspace 트리·설정 등)과 구별해 한곳에 모은다.

use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use crate::core::child_terminal::ChildTerminalRegistry;
use crate::core::pty_registry::PtyRegistry;
use crate::core::terminal_store::TerminalStore;

/// 필드는 선언 순서대로 drop된다. Terminal(Pty Drop이 셸을 종료)을 먼저 정리한다.
pub(crate) struct EngineRuntime {
    /// 실제 Terminal과 scrollback 저장 ID. 레이아웃 트리의 TerminalSurface는 ID만 참조한다.
    pub(crate) terminals: TerminalStore,

    /// 자식 terminal surface의 부모·번호·상태 기록. 파일에서 읽으며 저장은 호출자가 요청한다.
    pub(crate) child_terminals: ChildTerminalRegistry,

    /// surface가 없는 PTY의 등록 정보와 watcher 결과. Terminal은 terminals에 있다. 비영속이다.
    pub(crate) pty_registry: PtyRegistry,

    /// hard attach 중 서버의 표시 사본. 원본 PTY와 별개인 기존 detached Terminal이다.
    pub(crate) readonly_views: std::collections::HashMap<u32, tasty_terminal::Terminal>,
}

impl EngineRuntime {
    /// PTY ID 발급기는 같은 프로세스의 engine들이 공유해야 ID가 겹치지 않는다.
    pub(crate) fn new(pty_counter: Arc<AtomicU32>) -> Self {
        Self {
            terminals: TerminalStore::new(),
            child_terminals: ChildTerminalRegistry::load(),
            pty_registry: PtyRegistry::with_counter(pty_counter),
            readonly_views: std::collections::HashMap::new(),
        }
    }
}
