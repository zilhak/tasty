//! `PresetView` (modeless editor — engine 전역 단일 인스턴스) 라이프사이클.

use std::sync::Arc;

use winit::window::WindowId;

use crate::app::App;
use crate::view;

impl App {
    /// PresetView 를 연다. 이미 열려 있으면 새 윈도우를 만들지 않고 기존 윈도우에
    /// 포커스만 옮긴다 (엔진 전역 단일 인스턴스).
    pub(crate) fn open_preset_window(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if let Some(id) = self.preset_view_id {
            if let Some(w) = self.view.views.get(&id) {
                w.base().winit.focus_window();
                return;
            }
            self.preset_view_id = None;
        }

        let attrs = Self::preset_window_attributes();
        let window = match Self::create_window_or_warn(event_loop, attrs) {
            Some(w) => w,
            None => return,
        };

        let appearance = self.focused_appearance_or_disk();
        // 편집 모드 표준 단축키 스냅샷 — appearance 와 동일하게 focused window
        // 설정에서 clone(부재 시 디스크 로드). 설정 변경은 창 재오픈 시 반영.
        let keybindings = self.focused_keybindings_or_disk();
        let gpu = match self.create_gpu_state(window.clone(), &appearance) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("failed to init GPU for preset window: {e}");
                return;
            }
        };

        let store = std::sync::Arc::clone(&self.core.preset_store);
        // 편집기 kind 소스 = main engine 의 공유 surface_registry(부재 시 None →
        // 빈 catalog → 정적 fallback). 모든 main window 가 같은 Arc 를 공유한다.
        let registry = self.any_main_engine().map(|e| e.surface_registry.clone());
        let window_id = window.id();
        let mut preset = view::PresetView::new(gpu, window, store, registry, keybindings);
        #[cfg(windows)]
        {
            use crate::view::ui::View as _;
            preset.render();
        }
        #[cfg(not(windows))]
        {
            use crate::view::ui::View as _;
            preset.mark_dirty();
        }
        self.view.views.insert(window_id, Box::new(preset));
        self.preset_view_id = Some(window_id);
        tracing::info!("opened preset window {:?}", window_id);
    }

    /// PresetView 신규 윈도우 생성용 `WindowAttributes` 조립.
    fn preset_window_attributes() -> winit::window::WindowAttributes {
        use winit::window::WindowAttributes;
        let mut attrs = WindowAttributes::default()
            .with_title(crate::i18n::t("preset.window.title"))
            .with_inner_size(winit::dpi::LogicalSize::new(960, 640))
            .with_min_inner_size(winit::dpi::LogicalSize::new(760, 480))
            .with_visible(false);
        if let Some(icon) = crate::app_icon::winit_window_icon() {
            attrs = attrs.with_window_icon(Some(icon));
        }
        attrs
    }

    /// focused main window 의 appearance 를 clone(부재 시 디스크에서 로드).
    fn focused_appearance_or_disk(&self) -> crate::settings::AppearanceSettings {
        self.focused_window()
            .map(|w| w.core_state.settings.appearance.clone())
            .unwrap_or_else(|| crate::settings::Settings::load().appearance)
    }

    /// focused main window 의 keybindings 를 clone(부재 시 디스크에서 로드).
    fn focused_keybindings_or_disk(&self) -> crate::settings::KeybindingSettings {
        self.focused_window()
            .map(|w| w.core_state.settings.keybindings.clone())
            .unwrap_or_else(|| crate::settings::Settings::load().keybindings)
    }

    /// winit 윈도우 생성. 실패 시 warn 로그 후 `None`.
    fn create_window_or_warn(
        event_loop: &winit::event_loop::ActiveEventLoop,
        attrs: winit::window::WindowAttributes,
    ) -> Option<Arc<winit::window::Window>> {
        match event_loop.create_window(attrs) {
            Ok(w) => Some(Arc::new(w)),
            Err(e) => {
                tracing::warn!("failed to create preset window: {e}");
                None
            }
        }
    }

    /// PresetView close 시 정리. store 는 Arc<Mutex<>> 공유라 별도 회수 불필요.
    pub(crate) fn on_preset_window_closed(&mut self, window_id: WindowId) {
        if self.preset_view_id != Some(window_id) {
            return;
        }
        self.preset_view_id = None;
        self.view.views.remove(&window_id);
    }

    /// 도구 메뉴 클릭 / Intent::SavePreset 후속 — PresetView 열기 + (있다면) selection.
    /// preset 저장/적용 자체는 Intent 핸들러 (`src/intent/preset.rs`) 에서 처리.
    pub(crate) fn process_pending_open_preset_window(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) {
        let mut request_open = false;
        let mut pending_selection: Option<(tasty_presets::PresetKind, String)> = None;
        for w in self.main_windows_iter_mut() {
            if w.state.dialogs.pending_open_preset_window {
                w.state.dialogs.pending_open_preset_window = false;
                request_open = true;
            }
            if let Some(sel) = w.state.dialogs.pending_preset_window_selection.take() {
                pending_selection = Some(sel);
                // selection 이 있으면 open 도 암묵적으로 요청.
                request_open = true;
            }
        }
        if !request_open {
            return;
        }
        self.open_preset_window(event_loop);
        if let Some((kind, name)) = pending_selection
            && let Some(pwid) = self.preset_view_id
            && let Some(pw) = self
                .view
                .views
                .get_mut(&pwid)
                .and_then(|w| w.as_any_mut().downcast_mut::<view::PresetView>())
        {
            pw.select(kind, name);
        }
    }
}
