//! App progress and one-shot cross-View request values. Services, handles and object registries have other owners.
#[cfg(feature="gui")]
use winit::window::WindowId;
#[cfg(feature="gui")]
use std::time::Instant;
#[cfg(feature="gui")]
use crate::app::shutdown_machine;
#[derive(Default)]
pub(crate) struct AppState {
    pub(crate) started:bool,
    #[cfg(not(feature="gui"))]
    pub(crate) stopping:bool,
    #[cfg(feature="gui")]
    pub(crate) boot:Option<BootProgress>,

    /// 종료를 시작한 뒤 이벤트 루프를 나갈 때까지 보유한다.
    #[cfg(feature = "gui")]
    pub(crate) shutdown: Option<shutdown_machine::ShutdownState>,
    #[cfg(feature = "gui")]
    pub(crate) shell_setup_mode: bool,
    #[cfg(feature = "gui")]
    pub(crate) shell_setup_path: String,
    // 엔진 생성 실패 시 창과 GPU가 남아 있으면 종료 버튼이 있는 오류 화면을 유지한다.
    // 창이나 GPU도 없으면 이 경로로 화면을 표시할 수 없다.
    #[cfg(feature = "gui")]
    pub(crate) boot_error_mode: bool,
    /// `drive_boot_frame`이 오류 화면으로 전환할 때 표시할 진단.
    #[cfg(feature = "gui")]
    pub(crate) boot_error_info: Option<crate::gpu::BootErrorInfo>,
    #[cfg(debug_assertions)]
    #[cfg_attr(
        not(feature = "gui"),
        expect(
            dead_code,
            reason = "read only by the gui event loop; headless has no reader"
        )
    )]
    pub(crate) input_simulation_enabled: bool,
    /// 헤드리스의 전체 플러그인 설치·기동을 수행했는지 구별한다.
    /// 메타데이터 조회만으로도 매니저는 만들어지므로 Some 여부로 대신할 수 없다.
    #[cfg(not(feature = "gui"))]
    pub(crate) plugin_started: bool,
    /// PresetView는 하나만 열며, 다시 열기를 요청하면 기존 창에 포커스를 준다.
    #[cfg(feature = "gui")]
    pub(crate) preset_view_id: Option<WindowId>,
    /// Plugins의 Configure에서 설정을 열 때 Plugin 탭을 한 번 선택한다.
    #[cfg(feature = "gui")]
    pub(crate) pending_settings_plugin_tab: bool,
    /// 파일 핸들러 등록 안내에서 설정을 열 때 FileHandler 탭을 한 번 선택한다.
    #[cfg(feature = "gui")]
    pub(crate) pending_settings_file_handler_tab: bool,
    /// 부팅 권한 안내에서 설정 창을 열 때 일반 > 권한 탭으로 바로 들어가게 하는
    /// 일회성 표시. `open_settings_modal`이 읽고 지운다.
    #[cfg(feature = "gui")]
    pub(crate) pending_settings_macos_permissions_tab: bool,
    /// debug.settings.open이 지정한 초기 탭. 다음 설정 창 열기에서 소비한다.
    #[cfg(all(feature = "gui", debug_assertions))]
    pub(crate) pending_settings_tab: Option<String>,
    /// debug.settings.open이 지정한 초기 하위 탭. 다음 설정 창 열기에서 소비한다.
    #[cfg(all(feature = "gui", debug_assertions))]
    pub(crate) pending_settings_subtab: Option<String>,
}

#[cfg(feature="gui")]
pub(crate) enum BootPhase {
    GpuInit,
    WaitingJournal,
    /// 엔진·플러그인 워커를 기다린다. 결과 없이 채널이 끊기면 동기로 다시 초기화한다.
    WaitingEngine {
        started: Instant,
        /// 워커 결과를 확인한 횟수. 성공한 화면 렌더 횟수와는 다르다.
        frames: u32,
    },
    /// 복원에 필요한 플러그인 kind 등록을 기다리며 이벤트를 처리한다.
    WaitingPlugins {
        started: Instant,
        deadline: Instant,
        needed: Vec<String>,
    },
    /// 레이아웃 적용 뒤 RemoteSurface 복원 응답을 기다린다.
    RestoringLayout {
        started: Instant,
        deadline: Instant,
    },
}


#[cfg(feature="gui")]
pub(crate) struct BootProgress {
    pub(crate) settings:crate::settings::Settings,
    pub(crate) settings_origin:tasty_settings::SettingsOrigin,
    pub(crate) phase:BootPhase,
    pub(crate) boot_t0:std::time::Instant,
    pub(crate) db_init_error:Option<crate::db::DbInitError>,
    pub(crate) invalid_theme_name:Option<String>,
    pub(crate) restored_idx:Option<crate::model::RestoredPresentation>,
    pub(crate) journal_plugins_waited:bool,
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum AppPhase {Starting,Running,Stopping,Failed}
impl AppState {
    pub(crate) fn phase(&self)->AppPhase {
        #[cfg(feature="gui")]
        {
            if self.shutdown.is_some() {return AppPhase::Stopping;}
            if self.boot_error_mode {return AppPhase::Failed;}
        }
        #[cfg(not(feature="gui"))]
        if self.stopping {return AppPhase::Stopping;}
        if self.started {AppPhase::Running}else{AppPhase::Starting}
    }
}
