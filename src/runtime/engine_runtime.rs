//! 한 engine이 소유하는 실제 Terminal·PTY 원본 컬렉션.
//! CoreState의 논리 세션 사실(workspace 트리·설정 등)과 구별해 한곳에 모은다.

use crate::runtime::surface_registry::SurfaceKindRegistry;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use tasty_terminal::Waker;

use crate::runtime::child_terminal::ChildTerminalRegistry;
use crate::runtime::terminal_store::TerminalStore;

/// Kind instances retire before terminal/Pty owners. Shared service references do not cancel tasks.
pub(crate) struct EngineRuntime {
    pub(crate) settings: crate::settings::Settings,
    pub(crate) default_cols: usize,
    pub(crate) default_rows: usize,
    #[cfg(all(feature = "gui", debug_assertions))]
    pub(crate) input_simulation_enabled: bool,

    pub(crate) ids: crate::runtime::id_reservations::IdReservations,
    #[cfg(feature = "gui")]
    pub(crate) counters: super::counters::RuntimeCounters,
    pub(crate) waker: Waker,
    pub(crate) pending_host_events: Vec<crate::core::host_event::PendingHostEvent>,
    pub(crate) pending_lifecycle_events: Vec<crate::core::host_event::PendingSurfaceClosed>,
    #[cfg(feature = "gui")]
    pub(crate) pending_plugin_retirements: Vec<(u32, tasty_host_plugin::host_cmd::SurfaceBinding)>,
    /// 대상별 출력 알림을 만드는 인터페이스. 도메인은 winit EventLoopProxy를 직접 보유하지 않는다.
    pub(crate) waker_factory: Option<crate::waker::SharedWakerFactory>,
    /// surface 종류와 생성·복원 동작의 등록부.
    pub(crate) surface_registry: Arc<SurfaceKindRegistry>,
    /// 내장 키 외에 plugin이 선언한 hook 키를 검증할 때 사용한다.
    pub(crate) plugin_hook_events: Arc<crate::core::hook_event_registry::PluginHookEventRegistry>,
    /// 기본·plugin·사용자 파일 형식 등록부. PluginManager와 같은 Arc를 쓴다.
    pub(crate) file_format: Arc<crate::file::format::FileFormatRegistry>,
    /// PluginManager와 공유하는 파일 처리기 등록부.
    pub(crate) file_handler: Arc<crate::file::handler::FileHandlerRegistry>,
    /// App이 GUI 이벤트 루프를 준비한 뒤 주입하는 파일 식별 worker 인터페이스.
    #[cfg(feature = "gui")]
    pub(crate) identify_worker:
        Option<std::sync::Arc<dyn crate::core::identify_port::IdentifySpawner>>,
    /// 공용 설정 파일에서 읽은 Explorer 즐겨찾기. 변경 뒤 저장은 호출자가 요청한다.
    #[cfg(feature = "gui")]
    pub(crate) explorer_favorites: crate::core::explorer_favorites::ExplorerFavorites,
    /// 공용 설정 파일에서 읽은 주소·포트 즐겨찾기. 변경 뒤 저장은 호출자가 요청한다.
    #[cfg(feature = "gui")]
    pub(crate) port_favorites: crate::core::port_favorites::PortFavorites,
    /// Core와 공유하는 저장소. engine 내부에서 직접 메타데이터를 기록할 때 쓴다.
    pub(crate) memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,

    /// Sole owner of materialized/deferred kind instances. CoreState leaves contain descriptors only.
    pub(crate) surfaces: std::collections::HashMap<u32, Box<dyn crate::model::Surface>>,
    /// 실제 Terminal과 scrollback 저장 ID. 레이아웃 트리의 TerminalSurface는 ID만 참조한다.
    pub(crate) terminals: TerminalStore,
    pub(crate) pending_submits: Vec<super::pending_submit::PendingSubmit>,
    pub(crate) pending_scrollback_inject:
        std::collections::HashMap<u32, Vec<tasty_terminal::ScrollbackLine>>,

    /// 자식 terminal surface의 부모·번호·상태 기록. 파일에서 읽으며 저장은 호출자가 요청한다.
    pub(crate) child_terminals: ChildTerminalRegistry,
    /// Local display content while the source PTY is hard-occupied. No remote transport ownership.
    #[cfg(feature = "gui")]
    pub(crate) readonly_views: std::collections::HashMap<u32, tasty_terminal::Terminal>,
}

impl EngineRuntime {
    #[cfg(feature = "gui")]
    pub(crate) fn has_pending_delivery(&self) -> bool {
        !self.pending_host_events.is_empty()
            || !self.pending_lifecycle_events.is_empty()
            || !self.pending_plugin_retirements.is_empty()
    }

    /// PTY ID 발급기는 같은 프로세스의 engine들이 공유해야 ID가 겹치지 않는다.
    pub(crate) fn new(
        counters: super::counters::RuntimeCounters,
        waker: Waker,
        memory: Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
        settings: crate::settings::Settings,
        cols: usize,
        rows: usize,
        registries: super::registries::RuntimeRegistries,
    ) -> Self {
        let runtime = Self {
            settings,
            default_cols: cols,
            default_rows: rows,
            #[cfg(all(feature = "gui", debug_assertions))]
            input_simulation_enabled: false,
            ids: Default::default(),
            #[cfg(feature = "gui")]
            counters: counters.clone(),
            waker: waker.clone(),
            pending_host_events: Vec::new(),
            pending_lifecycle_events: Vec::new(),
            #[cfg(feature = "gui")]
            pending_plugin_retirements: Vec::new(),
            waker_factory: None,
            surface_registry: registries.surface_registry,
            plugin_hook_events: registries.plugin_hook_events,
            file_format: registries.file_format,
            file_handler: registries.file_handler,
            #[cfg(feature = "gui")]
            identify_worker: None,
            #[cfg(feature = "gui")]
            explorer_favorites: crate::core::explorer_favorites::ExplorerFavorites::load(),
            #[cfg(feature = "gui")]
            port_favorites: crate::core::port_favorites::PortFavorites::load(),
            memory,

            surfaces: Default::default(),
            pending_submits: Vec::new(),
            terminals: TerminalStore::new(counters.pty_counter()),
            pending_scrollback_inject: Default::default(),
            child_terminals: ChildTerminalRegistry::load(),
            #[cfg(feature = "gui")]
            readonly_views: Default::default(),
        };
        runtime
    }
}
