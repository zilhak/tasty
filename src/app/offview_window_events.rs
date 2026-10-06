//! View 밖 창(첫 실행 셸 설정, 부팅 오류)이 이벤트마다 할 일을 정한다. 이 창들은
//! dirty 표시와 렌더 예약을 받지 않으므로, 다시 그리기와 surface 재구성을 여기서 정한다.

use winit::event::WindowEvent;

/// View 밖 창이 렌더 외 이벤트 하나를 처리한 뒤 할 일.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OffviewEventEffect {
    /// 창 크기나 배율이 바뀌어 surface를 다시 구성한다.
    pub(crate) reconfigure_surface: bool,
    /// 다음 프레임을 그린다. 버튼 클릭은 렌더에서 처리되므로 입력 뒤에도 필요하다.
    pub(crate) redraw: bool,
    /// 창을 닫아 이벤트 루프를 끝낸다.
    pub(crate) exit: bool,
}

/// `egui_repaint`는 egui가 이 입력으로 화면이 바뀐다고 답한 값이다.
pub(crate) fn offview_event_effect(event: &WindowEvent, egui_repaint: bool) -> OffviewEventEffect {
    let reconfigure_surface = matches!(
        event,
        WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }
    );
    OffviewEventEffect {
        reconfigure_surface,
        redraw: reconfigure_surface || egui_repaint,
        exit: matches!(event, WindowEvent::CloseRequested),
    }
}

/// surface가 창과 어긋나 생긴 렌더 오류면 surface를 다시 구성하고 다음 프레임을 그린다.
pub(crate) fn render_error_reconfigures(error: &wgpu::SurfaceError) -> bool {
    matches!(
        error,
        wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::PhysicalSize;

    #[test]
    fn resize_reconfigures_the_surface_and_redraws() {
        let effect =
            offview_event_effect(&WindowEvent::Resized(PhysicalSize::new(800, 600)), false);
        assert!(effect.reconfigure_surface);
        assert!(effect.redraw);
        assert!(!effect.exit);
    }

    #[test]
    fn input_that_egui_repaints_requests_a_redraw() {
        let event = WindowEvent::CursorEntered {
            device_id: winit::event::DeviceId::dummy(),
        };
        assert!(offview_event_effect(&event, true).redraw);
        assert!(!offview_event_effect(&event, false).redraw);
        assert!(!offview_event_effect(&event, true).reconfigure_surface);
    }

    #[test]
    fn close_request_exits() {
        let effect = offview_event_effect(&WindowEvent::CloseRequested, false);
        assert!(effect.exit);
        assert!(!effect.redraw);
    }

    #[test]
    fn outdated_and_lost_surfaces_are_reconfigured() {
        assert!(render_error_reconfigures(&wgpu::SurfaceError::Outdated));
        assert!(render_error_reconfigures(&wgpu::SurfaceError::Lost));
        assert!(!render_error_reconfigures(&wgpu::SurfaceError::Timeout));
        assert!(!render_error_reconfigures(&wgpu::SurfaceError::OutOfMemory));
    }
}
