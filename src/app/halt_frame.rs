//! 실행 중 journal이 halt된 뒤의 최소 프레임. halt 중에는 일반 프레임과 입력을 처리하지 않으므로,
//! 오른쪽 위 비포커스 안내만 그리고 닫을 수 있게 둔다.
//!
//! 그리는 프레임은 엔진을 읽기만 한다. 이 프레임이 만든 요청은 적용하지 않고 버린다. 포인터는
//! 안내 상자와 타이틀바 닫기 버튼 위에 있을 때만 egui에 넘기므로 다른 UI는 클릭을 받지 않는다.
//! 키 입력은 넘기지 않아 포커스는 halt 전 위치에 그대로 남는다.
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::App;

impl App {
    /// halt 알림을 알림 저장소와 안내 열로 옮기고, 새 안내가 생긴 창을 다시 그리게 한다.
    pub(crate) fn surface_halt_notice(&mut self) {
        self.deliver_recovery_notices();
        self.collect_restore_notice_acknowledgements();
        for view in self.view.views.values_mut() {
            if let Some(main) = view.as_main_mut()
                && main.state.restore_notices.has_undrawn()
            {
                crate::view::ui::View::mark_dirty(main);
                main.base.winit.request_redraw();
            }
        }
    }

    /// halt 중 egui의 즉시 repaint 요청. 안내 Area는 첫 프레임에 크기만 재고 다음 프레임에 보이므로
    /// 이 요청을 버리면 안내가 그려지지 않는다. 타이머가 돌지 않으므로 지연 요청은 받지 않는다.
    pub(crate) fn request_halted_repaint(&mut self, id: WindowId) {
        if let Some(main) = self
            .view
            .views
            .get_mut(&id)
            .and_then(|view| view.as_main_mut())
        {
            main.base.winit.request_redraw();
        }
    }

    /// halt 중 창 닫기. 종료 확인 창은 halt 중 입력을 받지 못하므로 마지막 창이면 확인 없이 종료한다.
    /// 창이 여럿이면 그 창만 닫는다.
    pub(crate) fn request_halted_close(&mut self, id: WindowId, event_loop: &ActiveEventLoop) {
        if self.state.preset_view_id == Some(id) || self.main_window_count() > 1 {
            self.request_close_window(id, event_loop);
        } else {
            tracing::info!(
                "window closed while the journal is halted; quitting without confirmation"
            );
            self.begin_shutdown(event_loop);
        }
    }

    /// halt 중 창 이벤트. `CloseRequested`는 호출자가 일반 경로로 보낸다.
    pub(crate) fn handle_halted_window_event(&mut self, id: WindowId, event: WindowEvent) {
        let Some(main) = self
            .view
            .views
            .get_mut(&id)
            .and_then(|view| view.as_main_mut())
        else {
            return;
        };
        let repaint = match &event {
            WindowEvent::RedrawRequested => {
                let Some(engine) = self.engines.window_read(id) else {
                    return;
                };
                let shown = main.state.restore_notices.len();
                main.base.state.dirty = true;
                main.render_if_dirty(&engine, None);
                let dropped = main.state.take_pending_intents();
                if !dropped.is_empty() {
                    tracing::debug!(
                        count = dropped.len(),
                        "halted frame: dropped requests from the frame"
                    );
                }
                // 이번 프레임에서 닫힌 안내는 다음 프레임에서 지워진다.
                main.state.restore_notices.len() != shown
            }
            WindowEvent::CursorMoved { position, .. } => {
                let ppp = main.base.gpu.egui_pixels_per_point();
                let pos = egui::pos2(position.x as f32 / ppp, position.y as f32 / ppp);
                let was = main.state.halt_pointer_inside;
                let inside = main.state.restore_notices.covers(pos)
                    || main
                        .state
                        .titlebar_close_rect
                        .is_some_and(|rect| rect.contains(pos));
                main.state.halt_pointer_inside = inside;
                match (was, inside) {
                    (_, true) => main.base.gpu.handle_egui_event(&main.base.winit, &event).1,
                    // 영역을 벗어나면 hover가 남지 않도록 포인터가 사라졌다고 알린다.
                    (true, false) => {
                        main.base.gpu.push_egui_pointer_gone();
                        true
                    }
                    (false, false) => false,
                }
            }
            WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. }
                if main.state.halt_pointer_inside =>
            {
                main.base.gpu.handle_egui_event(&main.base.winit, &event).1
            }
            WindowEvent::CursorLeft { .. } => {
                main.base.gpu.handle_egui_event(&main.base.winit, &event).1
            }
            _ => false,
        };
        if repaint {
            main.base.winit.request_redraw();
        }
    }
}
