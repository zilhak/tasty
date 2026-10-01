use crate::core::CoreState;
use crate::runtime::engine_access::{EngineMut, EngineRef};
use std::collections::HashMap;
use std::sync::Arc;

use crate::model::Workspace;
use crate::notification::NotificationStore;
use crate::runtime::surface_registry::SurfaceKindRegistry;
use crate::settings::Settings;
pub(crate) use message::SurfaceMessage;
use tasty_terminal::Waker;

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
#[derive(Clone, Debug)]
pub(crate) struct PendingImageUpload {
    /// Original View captured when the user enqueues the paste, preserved through retry.
    pub(crate) origin_view: std::sync::Weak<()>,
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

impl EngineMut<'_> {
    /// 키보드·IME·붙여넣기의 사용자 입력 시각을 기록한다. 마우스 보고·파일 열기·에이전트 전송은 제외한다.
    #[cfg(feature = "gui")]
    pub fn record_typing(&mut self, surface_id: u32) {
        self.live
            .last_key_input
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

impl EngineRef<'_> {
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
                Some(self.runtime.settings.general.explorer_view_mode.clone())
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

impl EngineMut<'_> {
    #[cfg(feature = "gui")]
    pub fn update_grid_size(&mut self, cols: usize, rows: usize) {
        self.runtime.default_cols = cols;
        self.runtime.default_rows = rows;
    }
}

pub(crate) mod attention;
mod busy;
pub mod child_liveness;
mod finders;
mod global_hooks;
mod idle_hooks;
mod message;
mod output_read;
mod pty;
mod shell_integration_hint;
mod soft_occupancy;
mod surface_cwd;
mod terminal_finders;

pub(crate) use attention::AttentionKind;
pub(crate) use surface_cwd::RemoteCwd;
#[cfg(any(feature = "gui", test))]
pub(crate) use surface_cwd::SurfaceCwd;
#[cfg(feature = "gui")]
pub use tasty_core::SurfaceDisplayPath;

impl EngineMut<'_> {
    #[cfg(feature = "gui")]
    pub fn resync_terminal_palettes(&mut self) {
        self.runtime.terminals.resync_palettes();
    }

    pub fn refresh_tab_display_name(&mut self, surface: u32) {
        if !self.core.has_surface(surface) {
            return;
        }
        let cwd = self.runtime.terminals.cwd(surface);
        let home = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
        let name = cwd.as_deref().and_then(|cwd| {
            if Some(cwd) == home.as_deref() {
                Some("~".into())
            } else if cwd == std::path::Path::new("/") {
                Some("/".into())
            } else {
                cwd.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            }
        });
        self.live
            .surface_titles
            .entry(surface)
            .or_default()
            .cwd_name = name;
    }

    pub fn refresh_tab_osc_title(&mut self, surface: u32) {
        if !self.core.has_surface(surface) {
            return;
        }
        self.live
            .surface_titles
            .entry(surface)
            .or_default()
            .osc_title = self
            .runtime
            .terminals
            .get(surface)
            .and_then(|terminal| terminal.current_title());
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
        let mut snap_fn = crate::runtime::surface_registry::snapshot_fn_for(
            &self.runtime.surface_registry,
            &self.runtime.surfaces,
        );
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
        let mut snap_fn = crate::runtime::surface_registry::snapshot_fn_for(
            &self.runtime.surface_registry,
            &self.runtime.surfaces,
        );
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
        assert_eq!(
            params["view_mode"],
            e.runtime.settings.general.explorer_view_mode
        );
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
        assert_eq!(
            params["view_mode"],
            e.runtime.settings.general.explorer_view_mode
        );
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

    fn registry() -> std::sync::Arc<tasty_task_runtime::RunnerRegistry> {
        std::sync::Arc::new(tasty_task_runtime::RunnerRegistry::new())
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

pub(crate) use finders::mirror_workspace_index_for_structural;
