//! `PresetWindow` (modeless editor — engine 전역 단일 인스턴스) 라이프사이클.

use std::sync::Arc;

use winit::window::WindowId;

use crate::adapters::ui::window;
use crate::app::App;

impl App {
    /// PresetWindow 를 연다. 이미 열려 있으면 새 윈도우를 만들지 않고 기존 윈도우에
    /// 포커스만 옮긴다 (엔진 전역 단일 인스턴스).
    pub(crate) fn open_preset_window(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if let Some(id) = self.preset_window_id {
            if let Some(w) = self.windows.get(&id) {
                w.base().winit.focus_window();
                return;
            }
            self.preset_window_id = None;
        }

        use winit::window::WindowAttributes;
        let mut attrs = WindowAttributes::default()
            .with_title(crate::i18n::t("preset.window.title"))
            .with_inner_size(winit::dpi::LogicalSize::new(960, 640))
            .with_min_inner_size(winit::dpi::LogicalSize::new(760, 480))
            .with_visible(false);
        if let Some(icon) = crate::app_icon::winit_window_icon() {
            attrs = attrs.with_window_icon(Some(icon));
        }
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::warn!("failed to create preset window: {e}");
                return;
            }
        };

        let appearance = self
            .focused_window()
            .map(|w| w.engine_state.settings.appearance.clone())
            .unwrap_or_else(|| crate::settings::Settings::load().appearance);
        let gpu = match pollster::block_on(crate::gpu::GpuState::new(
            window.clone(),
            &appearance,
            self.view.proxy.clone(),
        )) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("failed to init GPU for preset window: {e}");
                return;
            }
        };

        let store = std::sync::Arc::clone(&self.core.preset_store);
        let window_id = window.id();
        let mut preset = window::PresetWindow::new(gpu, window, store);
        #[cfg(windows)]
        {
            use window::Window as _;
            preset.render();
        }
        #[cfg(not(windows))]
        {
            use window::Window as _;
            preset.mark_dirty();
        }
        self.windows.insert(window_id, Box::new(preset));
        self.preset_window_id = Some(window_id);
        tracing::info!("opened preset window {:?}", window_id);
    }

    /// PresetWindow close 시 정리. store 는 Arc<Mutex<>> 공유라 별도 회수 불필요.
    pub(crate) fn on_preset_window_closed(&mut self, window_id: WindowId) {
        if self.preset_window_id != Some(window_id) {
            return;
        }
        self.preset_window_id = None;
        self.windows.remove(&window_id);
    }

    /// 도구 메뉴 클릭 / Intent::SavePreset 후속 — PresetWindow 열기 + (있다면) selection.
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
        if let Some((kind, name)) = pending_selection {
            if let Some(pwid) = self.preset_window_id {
                if let Some(pw) = self
                    .windows
                    .get_mut(&pwid)
                    .and_then(|w| w.as_any_mut().downcast_mut::<window::PresetWindow>())
                {
                    pw.select(kind, name);
                }
            }
        }
    }
}
