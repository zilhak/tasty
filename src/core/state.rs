use crate::runtime::engine_access::{EngineMut, EngineRef};
use std::collections::HashMap;
use std::sync::Arc;

use crate::runtime::surface_registry::SurfaceKindRegistry;
use crate::model::Workspace;
use crate::notification::NotificationStore;
use crate::settings::Settings;
pub(crate) use message::SurfaceMessage;
use tasty_terminal::Waker;

/// 여러 engine이 같은 Arc 카운터를 써 ID가 겹치지 않게 한다. Clone도 카운터를 공유한다.
/// u32·u64 카운터의 overflow나 ID 범위 소진을 여기서 별도로 막지는 않는다.
#[derive(Clone)]
pub struct IdGenerator {
    workspace: Arc<std::sync::atomic::AtomicU32>,
    /// normal 카테고리의 0을 예약하고 1에서 시작한다.
    category: Arc<std::sync::atomic::AtomicU32>,
    pane: Arc<std::sync::atomic::AtomicU32>,
    tab: Arc<std::sync::atomic::AtomicU32>,
    surface: Arc<std::sync::atomic::AtomicU32>,
    /// 같은 TerminalStore에 넣는 PTY ID는 PTY_ID_BASE에서 시작한다.
    pty: Arc<std::sync::atomic::AtomicU32>,
    observer: Arc<std::sync::atomic::AtomicU64>,
    hook: Arc<std::sync::atomic::AtomicU64>,
    global_hook: Arc<std::sync::atomic::AtomicU32>,
    /// 알림 저장소는 engine별이며 ID·생성 순번은 프로세스에서 공유한다.
    notification: Arc<std::sync::atomic::AtomicU64>,
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl IdGenerator {
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU32, AtomicU64};
        Self {
            workspace: Arc::new(AtomicU32::new(1)),
            category: Arc::new(AtomicU32::new(1)),
            pane: Arc::new(AtomicU32::new(1)),
            tab: Arc::new(AtomicU32::new(1)),
            surface: Arc::new(AtomicU32::new(1)),
            pty: Arc::new(AtomicU32::new(crate::runtime::terminal_store::PTY_ID_BASE)),
            observer: Arc::new(AtomicU64::new(1)),
            hook: Arc::new(AtomicU64::new(1)),
            global_hook: Arc::new(AtomicU32::new(0)),
            notification: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn pty_counter(&self) -> Arc<std::sync::atomic::AtomicU32> {
        Arc::clone(&self.pty)
    }

    pub fn observer_counter(&self) -> Arc<std::sync::atomic::AtomicU64> {
        Arc::clone(&self.observer)
    }

    pub fn hook_counter(&self) -> Arc<std::sync::atomic::AtomicU64> {
        Arc::clone(&self.hook)
    }

    pub fn global_hook_counter(&self) -> Arc<std::sync::atomic::AtomicU32> {
        Arc::clone(&self.global_hook)
    }

    pub fn notification_counter(&self) -> Arc<std::sync::atomic::AtomicU64> {
        Arc::clone(&self.notification)
    }

    pub fn next_workspace(&self) -> u32 {
        self.workspace
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn next_category(&self) -> u32 {
        self.category
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// 복원한 카테고리 ID를 재사용하지 않도록 다음 발급 기준을 높인다. 이미 더 크면 유지한다.
    #[cfg(test)]
    pub fn bump_category_floor(&self, min_next: u32) {
        self.category
            .fetch_max(min_next, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn next_pane(&self) -> u32 {
        self.pane.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn next_tab(&self) -> u32 {
        self.tab.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn next_surface(&self) -> u32 {
        self.surface
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// 이전 실행의 surface 메타데이터 ID를 피하도록 다음 발급 기준을 높인다.
    /// 현재 기준을 낮추지 않으며 이후 overflow까지 막는 함수는 아니다.
    #[cfg(test)]
    pub fn bump_surface_floor(&self, min_next: u32) {
        self.surface
            .fetch_max(min_next, std::sync::atomic::Ordering::Relaxed);
    }
}

pub struct ShellConfig {
    pub shell: String,
    pub args: Vec<String>,
    /// 셸 초기화에 필요한 추가 환경변수. bash의 rcfile 설정은 args로 전달한다.
    pub envs: Vec<(String, String)>,
}

impl ShellConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            shell: settings.general.shell.clone(),
            args: settings.general.effective_shell_args(),
            envs: settings.general.effective_shell_envs(),
        }
    }

    pub fn shell_ref(&self) -> Option<&str> {
        if self.shell.is_empty() {
            None
        } else {
            Some(&self.shell)
        }
    }

    pub fn args_ref(&self) -> Vec<&str> {
        self.args.iter().map(|s| s.as_str()).collect()
    }

    pub fn envs_ref(&self) -> Vec<(&str, &str)> {
        self.envs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }
}

/// 이동 대기 중인 대상의 종류와 ID. "이곳으로 이동"은 메뉴 대상과 종류가 같을 때만 연다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    all(not(feature = "gui"), not(test)),
    expect(
        dead_code,
        reason = "only the gui-only context menus mark items for moving"
    )
)]
pub(crate) enum PendingMove {
    Surface(crate::model::SurfaceId),
    Tab(crate::model::TabId),
    Pane(crate::model::PaneId),
}

/// 사용자가 원격 연결 팝업에서 확정한 요청. 조회에 쓴 SSH 터널을 함께 넘길 수 있다.
/// IPC 요청과 달리 연결 성공 후 새 mirror를 선택할 수 있어 별도 큐다.
#[cfg(feature = "gui")]
pub(crate) struct GuiAttachUserReq {
    pub(crate) port: u16,
    pub(crate) workspace: u32,
    pub(crate) tunnel: Option<tasty_ssh::SshTunnel>,
}

/// 붙여넣기 시점의 mirror 대상을 고정하고 백그라운드 업로드 뒤 그 surface에 원격 경로를 입력한다.
#[cfg(feature = "gui")]
pub(crate) struct PendingImageUpload {
    /// attach 세션을 찾을 로컬 mirror workspace ID.
    pub(crate) mirror_ws_id: u32,
    /// 붙여넣기 시점에 정한 로컬 mirror surface ID.
    pub(crate) surface_id: u32,
    /// 붙여넣기 시점의 bracketed paste 설정.
    pub(crate) bracketed: bool,
    pub(crate) file_name: String,
    pub(crate) png_bytes: Vec<u8>,
}

/// MeshContext의 내용. 로컬 surface ID는 이 요청을 담는 큐의 키다.
#[derive(Debug, Clone)]
#[cfg(feature = "gui")]
pub(crate) struct AttachMeshContextForward {
    pub(crate) width_px: u32,
    pub(crate) height_px: u32,
    pub(crate) pixels_per_point: f32,
    pub(crate) theme: Option<tasty_plugin_protocol::protocol::ThemeWire>,
    pub(crate) focused: bool,
}

/// engine별 도메인 상태. GUI에서는 창마다 따로 보유하고 공유 자원은 Arc로 주입한다.
/// 외부 함수의 타입에 쓰이지만 내부 필드는 crate 밖에 노출하지 않는다.
pub struct CoreState {
    /// Revision of this committed live projection, never a command-decision source.
    pub(crate) committed_structure_revision: Option<u64>,
    pub(crate) local_workspaces: Vec<Workspace>,
    pub(crate) mirror_workspaces: Vec<Workspace>,
    /// Composite display projection; local relative order comes from the committed model.
    workspace_display_order: Vec<u32>,
    /// Lifetime token for volatile annotations; replaced by every remote structural projection.
    mirror_projection_tokens: std::collections::HashMap<u32, std::sync::Arc<()>>,
    /// 표시 순서의 카테고리. 생성·복원 뒤 기본 normal 항목을 앞에 두도록 정규화한다.
    pub(crate) categories: Vec<crate::model::WorkspaceCategory>,
    pub(crate) next_ids: IdGenerator,
    pub(crate) default_cols: usize,
    pub(crate) default_rows: usize,
    pub(crate) settings: Settings,

    pub(crate) notifications: NotificationStore,
    pub(crate) closed_items: crate::model::ClosedItemStore,

    pub(crate) approval_store: std::sync::Arc<tasty_approval::ApprovalStore>,

    /// 같은 밀리초에 발생한 telemetry 키를 구별할 이 engine의 순번.
    pub(crate) telemetry_seq: std::sync::Arc<tasty_telemetry::TelemetrySeq>,

    /// 이 engine의 메모리 내 이상 탐지 상태. 탐지 기록 저장은 호출자가 맡는다.
    pub(crate) anomaly_detector: std::sync::Arc<tasty_telemetry::AnomalyDetector>,

    /// "이동"으로 지정한 대상. 종류와 관계없이 하나만 대기하며 새로 지정하면 덮어쓴다. 저장하지 않는다.
    pub(crate) pending_move: Option<PendingMove>,

    pub(crate) layout_dirty: crate::core::layout_persistence::LayoutDirtyTracker,
    /// 복원한 활성 workspace 인덱스. 창 상태를 만들 때 한 번 소비한다.
    /// deferred Terminal 생성 뒤 적용할 scrollback. 읽지 못했거나 비어 있으면 등록하지 않는다.
    pub(crate) pending_scrollback_inject: HashMap<u32, Vec<tasty_terminal::ScrollbackLine>>,
    /// plugin 준비 대기 후 적용할 레이아웃. 대기와 제한 시간 처리는 App이 맡는다.
    pub(crate) pending_layout_restore: Option<crate::core::layout_persistence::SavedLayout>,
    /// 이 engine의 레이아웃 슬롯. 프로세스 내 engine들의 이 필드로 점유를 확인한다.
    /// 디스크 잠금은 아니며 헤드리스는 None이다.
    pub(crate) layout_slot: Option<crate::core::layout_persistence::LayoutSlotId>,
    /// 읽기 실패·높은 version으로 기존 슬롯을 덮어쓰면 안 되는 상태.
    #[cfg(any(feature = "gui", test))]
    pub(crate) layout_slot_protected: bool,
    /// 해석 실패한 원본을 저장 전에 재확인·백업해야 하는 상태.
    #[cfg(any(feature = "gui", test))]
    pub(crate) layout_slot_unparsable: bool,
    /// 검사에서 실제 홈 대신 사용할 저장 디렉터리. 저장과 백업 공간 판정이 함께 사용한다.
    #[cfg(test)]
    pub(crate) layouts_dir_override: Option<std::path::PathBuf>,
    /// 백업 공간 부족 또는 보존 실패를 사용자에게 알리기 위한 상태.
    #[cfg(any(feature = "gui", test))]
    pub(crate) layout_slot_preserve_failed: bool,

    #[cfg(debug_assertions)]
    #[cfg_attr(
        not(feature = "gui"),
        expect(
            dead_code,
            reason = "only the gui input simulation IPC of debug builds reads the flag"
        )
    )]
    pub(crate) input_simulation_enabled: bool,

}

impl CoreState {
    /// 슬롯 로드 판정을 대기 복원·쓰기 보호·백업 필요 플래그에 반영한다.
    #[cfg(test)]
    pub(crate) fn accept_slot_load(
        &mut self,
        load: crate::core::layout_persistence::SlotLoad,
        slot: crate::core::layout_persistence::LayoutSlotId,
    ) {
        use crate::core::layout_persistence::SlotLoad;
        match load {
            SlotLoad::Loaded(saved) => self.pending_layout_restore = Some(saved),
            SlotLoad::Absent => {}
            SlotLoad::Unreadable => self.layout_slot_protected = true,
            SlotLoad::Unparsable => {
                self.layout_slot_unparsable = true;
                // 첫 저장 전에 뜨는 안내도 백업 공간 부족을 구별해야 한다.
                self.layout_slot_preserve_failed = self.slot_preservation_is_blocked(slot);
            }
        }
    }

    /// 저장과 같은 디렉터리에서 백업 공간을 확인한다. 검사 override도 동일하게 적용한다.
    #[cfg(test)]
    fn slot_preservation_is_blocked(
        &self,
        slot: crate::core::layout_persistence::LayoutSlotId,
    ) -> bool {
        #[cfg(test)]
        if let Some(dir) = self.layouts_dir_override.as_deref() {
            return crate::core::layout_persistence::slot_preservation_is_blocked_in(dir, slot);
        }
        crate::core::layout_persistence::slot_preservation_is_blocked(slot)
    }

    pub(crate) fn new_base(
        cols: usize,
        rows: usize,
        next_ids: IdGenerator,
        layout_slot: Option<crate::core::layout_persistence::LayoutSlotId>,
        settings: Settings,
    ) -> Self {
        let mut engine = Self {
            committed_structure_revision: None,
            local_workspaces: Vec::new(),
            mirror_workspaces: Vec::new(),
            workspace_display_order: Vec::new(),
            mirror_projection_tokens: Default::default(),
            categories: vec![crate::model::WorkspaceCategory::normal()],
            next_ids: next_ids.clone(),
            default_cols: cols,
            default_rows: rows,
            settings,
            notifications: NotificationStore::with_counter(500, next_ids.notification_counter()),
            closed_items: crate::model::ClosedItemStore::new(),
            approval_store: std::sync::Arc::new(tasty_approval::ApprovalStore::new()),
            telemetry_seq: std::sync::Arc::new(tasty_telemetry::TelemetrySeq::new()),
            anomaly_detector: std::sync::Arc::new(tasty_telemetry::AnomalyDetector::new()),
            pending_move: None,
            layout_dirty: crate::core::layout_persistence::LayoutDirtyTracker::new(),
            pending_scrollback_inject: HashMap::new(),
            pending_layout_restore: None,
            layout_slot,
            #[cfg(any(feature = "gui", test))]
            layout_slot_protected: false,
            #[cfg(any(feature = "gui", test))]
            layout_slot_unparsable: false,
            #[cfg(test)]
            layouts_dir_override: None,
            #[cfg(any(feature = "gui", test))]
            layout_slot_preserve_failed: false,
            #[cfg(debug_assertions)]
            input_simulation_enabled: false,
        };

        engine.notifications = NotificationStore::with_counter(
            engine.settings.notification.coalesce_ms,
            next_ids.notification_counter(),
        );

        engine
    }

    /// 현재 트리에서 복원 항목의 출처 workspace를 찾는다. 트리를 바꾸기 전에 호출해야 한다.
    /// 이미 제거했거나 workspace 전체 항목이면 None이라 workspace 범위 복원에서 제외된다.
    fn origin_workspace_of(&self, item: &crate::model::ClosedItem) -> Option<u32> {
        use crate::model::closed_item::ClosedItem;
        let ws_idx = match item {
            ClosedItem::Surface { surface, .. } => self
                .find_workspace_index_for_surface(surface.id)
                .map(|(i, _)| i),
            ClosedItem::Tab(tab) => self
                .find_pane_for_tab(tab.id)
                .and_then(|pid| self.find_workspace_index_for_pane(pid)),
            ClosedItem::Pane { pane, .. } => self.find_workspace_index_for_pane(pane.id),
            ClosedItem::Workspace { .. } => return None,
        }?;
        self.workspace_at(ws_idx).map(|ws| ws.id)
    }

    pub fn push_closed_item(
        &mut self,
        mut item: crate::model::ClosedItem,
    ) -> crate::close_trace::PushClosedItemTimings {
        let mut timings = crate::close_trace::PushClosedItemTimings::default();
        let origin_workspace = self.origin_workspace_of(&item);
        let mem = self.runtime.memory.clone();
        let t_inject = std::time::Instant::now();
        crate::model::closed_item::inject_restore_commands(&mut item, &|sid| {
            let mut guard = crate::poison::recover_mutex(
                mem.lock(),
                crate::core::MEMORY_WHAT,
                &crate::core::MEMORY_POISONED,
            );
            crate::surface_meta::SurfaceMetaStore::get(&mut *guard, sid, "restore.command")
        });
        timings.restore_inject = t_inject.elapsed();
        // 닫힌 항목은 큰 scrollback을 메모리에 계속 들지 않도록 별도 파일 ID로 저장한다.
        // 원래 surface의 저장 ID와 분리해 surface 정리가 이 파일까지 지우지 않게 한다.
        let t_persist = std::time::Instant::now();
        crate::model::closed_item::persist_closed_scrollback(&mut item, &mut |blob| {
            let id = crate::scrollback_store::new_persist_id();
            match crate::scrollback_store::write_bytes(&id, &blob.bytes) {
                Ok(()) => Some(id),
                Err(e) => {
                    tracing::warn!("closed-item scrollback persist failed: {e}");
                    None
                }
            }
        });
        timings.scrollback_persist = t_persist.elapsed();
        // 복원 목록에서 밀려난 항목의 별도 scrollback 파일도 지운다.
        let t_evict = std::time::Instant::now();
        if let Some(evicted) = self.closed_items.push(item, origin_workspace) {
            let mut refs = Vec::new();
            crate::model::closed_item::collect_scrollback_refs(&evicted, &mut refs);
            for id in refs {
                crate::scrollback_store::delete(&id);
            }
        }
        timings.evict = t_evict.elapsed();
        timings
    }

    /// 키보드·IME·붙여넣기의 사용자 입력 시각을 기록한다. 마우스 보고·파일 열기·에이전트 전송은 제외한다.
    #[cfg(feature = "gui")]
    pub fn record_typing(&mut self, surface_id: u32) {
        self.live.last_key_input
            .insert(surface_id, std::time::Instant::now());
    }

    pub fn is_typing(&self, surface_id: u32) -> bool {
        if let Some(last) = self.live.last_key_input.get(&surface_id) {
            last.elapsed().as_secs_f64() < 5.0
        } else {
            false
        }
    }
}

impl CoreState {
    /// 등록된 종류로 surface를 만든다. Terminal의 PTY 생성은 호출자가 별도로 처리한다.
    /// cwd는 호출자가 정해 넘기며 사용 여부는 각 종류가 결정한다.
    pub(crate) fn create_surface_via_registry(
        &self,
        kind: &str,
        surface_id: u32,
        cwd: Option<&std::path::Path>,
        params: &serde_json::Value,
    ) -> anyhow::Result<Box<dyn crate::model::Surface>> {
        // 철회된 plugin 종류는 알 수 없는 종류와 구별해 필요한 조치를 안내한다.
        if let Some(plugin_id) = self.runtime.surface_registry.withdrawn_by(kind) {
            return Err(crate::runtime::surface_registry::SurfaceKindWithdrawn {
                kind: kind.to_string(),
                plugin_id,
            }
            .into());
        }
        let def = self.runtime.surface_registry
            .get_live(kind)
            .ok_or_else(|| anyhow::anyhow!("unknown surface kind: {}", kind))?;
        // 명시한 params가 우선이다. cwd 상속 경로에서 홈으로 바꾸지 않도록 @home은 여기서 해석하지 않는다.
        if def.default_params.is_empty() {
            return (def.create)(surface_id, cwd, params).and_then(|prepared| prepared.publish());
        }
        let mut owned = params.clone();
        if self.apply_kind_default_params(&def, &mut owned, None) {
            (def.create)(surface_id, cwd, &owned).and_then(|prepared| prepared.publish())
        } else {
            (def.create)(surface_id, cwd, params).and_then(|prepared| prepared.publish())
        }
    }

    /// 없는 키에만 기본값을 넣고 하나라도 넣으면 true다. params가 객체가 아니면 변경하지 않는다.
    /// @settings.explorer_view_mode와 전달된 @home을 해석하고 알 수 없는 @ 토큰은 경고 후 건너뛴다.
    pub(crate) fn apply_kind_default_params(
        &self,
        def: &crate::runtime::surface_registry::SurfaceKindDef,
        params: &mut serde_json::Value,
        home: Option<&std::path::Path>,
    ) -> bool {
        if def.default_params.is_empty() {
            return false;
        }
        let Some(obj) = params.as_object_mut() else {
            return false;
        };
        let mut injected = false;
        for (key, token) in &def.default_params {
            if obj.contains_key(key.as_str()) {
                continue;
            }
            let Some(val) = self.resolve_default_param_token(token, home) else {
                continue;
            };
            obj.insert(key.clone(), serde_json::Value::String(val));
            injected = true;
        }
        injected
    }

    fn resolve_default_param_token(
        &self,
        token: &str,
        home: Option<&std::path::Path>,
    ) -> Option<String> {
        match token {
            "@settings.explorer_view_mode" => {
                Some(self.settings.general.explorer_view_mode.clone())
            }
            "@home" => home.map(|p| p.to_string_lossy().to_string()),
            t if t.starts_with('@') => {
                tracing::warn!("unknown default_param policy token: {t}");
                None
            }
            literal => Some(literal.to_string()),
        }
    }
}

impl CoreState {
    #[cfg(feature = "gui")]
    pub fn update_grid_size(&mut self, cols: usize, rows: usize) {
        self.default_cols = cols;
        self.default_rows = rows;
    }
}

pub(crate) mod attention;
mod busy;
mod category;
pub mod child_liveness;
mod finders;
mod global_hooks;
mod idle_hooks;
mod message;
mod output_read;
mod pty;
mod shell_integration_hint;
mod soft_occupancy;
mod surface_cleanup;
mod surface_cwd;
mod terminal_finders;
pub(crate) mod workspaces;

pub(crate) use attention::AttentionKind;
#[cfg(feature = "gui")]
pub use finders::SurfaceDisplayPath;
pub(crate) use surface_cwd::RemoteCwd;
#[cfg(any(feature = "gui", test))]
pub(crate) use surface_cwd::SurfaceCwd;

impl EngineMut<'_> {
    #[cfg(feature = "gui")]
    pub fn resync_terminal_palettes(&mut self) {
        self.runtime.terminals.resync_palettes();
    }

    pub fn refresh_tab_display_name(&mut self, surface_id: u32) {
        let workspaces = self.core.workspaces_mut();
        let terminals = &self.runtime.terminals;
        for workspace in workspaces {
            let pane_ids = workspace.pane_layout().all_pane_ids();
            for pid in pane_ids {
                if let Some(pane) = workspace.pane_layout_mut().find_pane_mut(pid) {
                    for tab in &mut pane.tabs {
                        if tab.contains_surface(surface_id) {
                            let cwd = terminals.cwd(surface_id);
                            tab.refresh_display_name(surface_id, cwd.as_deref());
                            return;
                        }
                    }
                }
            }
        }
    }

    /// surface_id가 속한 탭에서 실제 선택된 surface의 제목을 읽는다.
    /// 제목이 없으면 OSC 제목을 비우고 사용자가 명시한 탭 이름은 유지한다.
    pub fn refresh_tab_osc_title(&mut self, surface_id: u32) {
        let workspaces = self.core.workspaces_mut();
        let terminals = &self.runtime.terminals;
        for workspace in workspaces {
            let pane_ids = workspace.pane_layout().all_pane_ids();
            for pid in pane_ids {
                if let Some(pane) = workspace.pane_layout_mut().find_pane_mut(pid) {
                    for tab in &mut pane.tabs {
                        if tab.contains_surface(surface_id) {
                            tab.surface_titles.entry(surface_id).or_default().osc_title =
                                terminals.get(surface_id).and_then(|t| t.current_title());
                            return;
                        }
                    }
                }
            }
        }
    }
}

impl EngineRef<'_> {
    /// 트리에서 제거하기 전에 탭의 복원 snapshot을 만든다. 복원 목록에 넣는 일은 호출자가 맡는다.
    pub(crate) fn capture_closed_tab(
        &self,
        pane_id: u32,
        tab_index: usize,
        presentation: &dyn crate::model::StructurePresentation,
    ) -> Option<crate::model::ClosedItem> {
        let tab = self.find_pane_by_id(pane_id)?.tabs.get(tab_index)?;
        let mut snap_fn = crate::runtime::surface_registry::snapshot_fn_for(&self.runtime.surface_registry);
        let terminals = &self.runtime.terminals;
        crate::model::closed_item::ClosedTab::from_tab(
            tab,
            &mut snap_fn,
            &|id| terminals.closed_capture(id),
            presentation,
        )
        .map(crate::model::ClosedItem::Tab)
    }

    /// pane 제거 전에 분할 위치를 포함한 snapshot을 만든다. workspace의 유일한 pane이면 None이다.
    pub(crate) fn capture_closed_pane(
        &self,
        pane_id: u32,
        presentation: &dyn crate::model::StructurePresentation,
    ) -> Option<crate::model::ClosedItem> {
        let ws = self.workspace_at(self.find_workspace_index_for_pane(pane_id)?)?;
        if ws.pane_layout().all_pane_ids().len() <= 1 {
            return None;
        }
        let pane = ws.pane_layout().find_pane(pane_id)?;
        let (direction, ratio, was_first, sibling_pane_id) =
            ws.pane_layout().locate_split_context(pane_id)?;
        let mut snap_fn = crate::runtime::surface_registry::snapshot_fn_for(&self.runtime.surface_registry);
        let terminals = &self.runtime.terminals;
        Some(crate::model::ClosedItem::from_pane(
            pane,
            sibling_pane_id,
            direction,
            ratio,
            was_first,
            &mut snap_fn,
            &|id| terminals.closed_capture(id),
            presentation,
        ))
    }
}

impl EngineMut<'_> {
    /// 트리에서 제거하기 전에 탭의 복원 snapshot을 만든다. 복원 목록에 넣는 일은 호출자가 맡는다.
    pub(crate) fn capture_closed_tab(
        &self,
        pane_id: u32,
        tab_index: usize,
        presentation: &dyn crate::model::StructurePresentation,
    ) -> Option<crate::model::ClosedItem> {
        self.as_ref()
            .capture_closed_tab(pane_id, tab_index, presentation)
    }

    /// pane 제거 전에 분할 위치를 포함한 snapshot을 만든다. workspace의 유일한 pane이면 None이다.
    pub(crate) fn capture_closed_pane(
        &self,
        pane_id: u32,
        presentation: &dyn crate::model::StructurePresentation,
    ) -> Option<crate::model::ClosedItem> {
        self.as_ref().capture_closed_pane(pane_id, presentation)
    }
}

#[cfg(test)]
mod id_generator_tests {
    use super::IdGenerator;

    #[test]
    fn next_surface_starts_at_one() {
        let ids = IdGenerator::new();
        assert_eq!(ids.next_surface(), 1);
        assert_eq!(ids.next_surface(), 2);
    }

    #[test]
    fn bump_surface_floor_raises_counter() {
        let ids = IdGenerator::new();
        ids.bump_surface_floor(18);
        assert_eq!(
            ids.next_surface(),
            18,
            "floor 이후 첫 id 는 min_next 와 같아야 한다"
        );
        assert_eq!(ids.next_surface(), 19);
    }

    #[test]
    fn two_engines_do_not_hand_out_the_same_hook_id() {
        use tasty_hooks::{HookBinding, HookEvent, HookManager};
        let ids = IdGenerator::new();
        let mut a = HookManager::with_counter(ids.hook_counter());
        let mut b = HookManager::with_counter(ids.hook_counter());
        let ia = a.add_hook(
            1,
            HookEvent::CommandCompleted(None),
            HookBinding::InlineShell("echo a".into()),
            false,
        );
        let ib = b.add_hook(
            1,
            HookEvent::CommandCompleted(None),
            HookBinding::InlineShell("echo b".into()),
            false,
        );
        assert_ne!(ia, ib, "공유 발급기를 쓰는 두 engine의 hook ID가 겹쳤다");
    }

    #[test]
    fn two_engines_do_not_hand_out_the_same_global_hook_id() {
        use crate::hook_runtime::global::{GlobalHookManager, HookCondition};
        let ids = IdGenerator::new();
        let mut a = GlobalHookManager::with_counter(ids.global_hook_counter());
        let mut b = GlobalHookManager::with_counter(ids.global_hook_counter());
        let ia = a.add(
            HookCondition::Interval(std::time::Duration::from_secs(60)),
            "echo a".into(),
            None,
        );
        let ib = b.add(
            HookCondition::Interval(std::time::Duration::from_secs(60)),
            "echo b".into(),
            None,
        );
        assert_ne!(
            ia, ib,
            "공유 발급기를 쓰는 두 engine의 global hook ID가 겹쳤다"
        );
    }

    #[test]
    fn bump_surface_floor_is_noop_when_already_higher() {
        let ids = IdGenerator::new();
        for _ in 0..4 {
            ids.next_surface();
        }
        ids.bump_surface_floor(3);
        assert_eq!(ids.next_surface(), 5);
    }
}

#[cfg(test)]
mod default_params_tests {

    fn engine() -> crate::runtime::engine_session::EngineSession {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        crate::runtime::engine_session::EngineSession::new(80, 24, waker).expect("engine")
    }

    #[test]
    fn explorer_defaults_without_home_inject_view_mode_only() {
        let mut e_session = engine();
        let e = e_session.borrow_mut();
        let def = e.surface_registry.get("explorer").unwrap();
        let mut params = serde_json::json!({});
        let injected = e.apply_kind_default_params(&def, &mut params, None);
        assert!(injected);
        assert_eq!(params["view_mode"], e.settings.general.explorer_view_mode);
        assert!(
            params.get("path").is_none(),
            "@home must not resolve when home=None"
        );
    }

    #[test]
    fn explorer_defaults_with_home_inject_path() {
        let mut e_session = engine();
        let e = e_session.borrow_mut();
        let def = e.surface_registry.get("explorer").unwrap();
        let mut params = serde_json::json!({});
        let home = std::path::PathBuf::from("/home/tester");
        e.apply_kind_default_params(&def, &mut params, Some(&home));
        assert_eq!(params["view_mode"], e.settings.general.explorer_view_mode);
        assert_eq!(params["path"], "/home/tester");
    }

    #[test]
    fn explicit_params_preserved() {
        let mut e_session = engine();
        let e = e_session.borrow_mut();
        let def = e.surface_registry.get("explorer").unwrap();
        let home = std::path::PathBuf::from("/home/tester");
        let mut params = serde_json::json!({"view_mode": "list", "path": "/explicit"});
        e.apply_kind_default_params(&def, &mut params, Some(&home));
        assert_eq!(params["view_mode"], "list");
        assert_eq!(params["path"], "/explicit");
    }

    #[test]
    fn kind_without_defaults_is_noop() {
        let mut e_session = engine();
        let e = e_session.borrow_mut();
        let def = e.surface_registry.get("terminal").unwrap();
        let mut params = serde_json::json!({});
        assert!(!e.apply_kind_default_params(&def, &mut params, None));
    }
}

/// 잘못된 셸 설정이 engine 생성의 Err로 전달되는지 확인한다. 창 전체의 오류 처리 검사는 아니다.
#[cfg(test)]
mod engine_creation_failure_tests {
    use super::*;

    fn bogus_shell_settings() -> Settings {
        let mut s = Settings::default();
        s.general.shell = "/nonexistent/definitely/not/a/real/shell-xyzzy".to_string();
        // 복원 대신 새 workspace 생성 경로를 실행해 셸 생성 오류를 확인한다.
        s.general.restore_layout = false;
        s
    }

    fn registry() -> std::sync::Arc<crate::runtime::agent::runner_thread::RunnerRegistry> {
        std::sync::Arc::new(crate::runtime::agent::runner_thread::RunnerRegistry::new())
    }

    fn in_memory() -> std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> {
        std::sync::Arc::new(std::sync::Mutex::new(
            tasty_memory::MemoryStore::open_in_memory().expect("in-memory store"),
        ))
    }

    #[test]
    fn a_bogus_shell_path_makes_engine_creation_return_err_not_panic() {
        let waker: Waker = std::sync::Arc::new(|| {});
        let result = crate::runtime::engine_session::EngineSession::new_with_ids_and_settings(
            80,
            24,
            waker,
            None,
            None,
            in_memory(),
            registry(),
            bogus_shell_settings(),
        );
        let err = result
            .err()
            .expect("a bogus shell must fail engine creation");
        let msg = format!("{err}");
        assert!(
            msg.contains("shell-xyzzy"),
            "the error must name the shell that could not be spawned, got: {msg}"
        );
    }

    #[test]
    fn a_valid_shell_still_produces_an_engine_with_one_workspace() {
        let waker: Waker = std::sync::Arc::new(|| {});
        let mut ok = Settings::default();
        ok.general.restore_layout = false;
        let mut engine_session =
            crate::runtime::engine_session::EngineSession::new_with_ids_and_settings(
                80,
                24,
                waker,
                None,
                None,
                in_memory(),
                registry(),
                ok,
            )
            .expect("default settings must produce an engine");
        let engine = engine_session.borrow_mut();
        assert_eq!(engine.workspaces().len(), 1);
    }

    #[test]
    fn the_engine_task_scope_holds_the_runner_registry_it_was_built_with() {
        let waker: Waker = std::sync::Arc::new(|| {});
        let mut settings = Settings::default();
        settings.general.restore_layout = false;
        let shared = registry();
        let mut engine_session =
            crate::runtime::engine_session::EngineSession::new_with_ids_and_settings(
                80,
                24,
                waker,
                None,
                None,
                in_memory(),
                std::sync::Arc::clone(&shared),
                settings,
            )
            .expect("default settings must produce an engine");
        let engine = engine_session.borrow_mut();
        assert!(std::sync::Arc::ptr_eq(
            engine.task_scope.runner_registry(),
            &shared
        ));
    }
}
