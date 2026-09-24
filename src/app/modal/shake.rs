//! 닫기를 거부한 모달을 흔들어 사용자에게 알린다.

use crate::app::{App, ModalShake};

impl App {
    /// 창 자체가 움직이는 애니메이션이므로 모션 감소 설정이 켜져 있으면 시작하지 않는다.
    pub(crate) fn trigger_modal_shake(&mut self) {
        if self.modal_shake.is_some() || crate::theme::theme().reduced_motion {
            return;
        }
        let modal_id = match self.view.active_modal_id {
            Some(id) => id,
            None => return,
        };
        let origin = match self.view.views.get(&modal_id) {
            Some(w) => match w.base().winit.outer_position() {
                Ok(pos) => pos,
                Err(_) => return,
            },
            None => return,
        };
        self.modal_shake = Some(ModalShake {
            start: std::time::Instant::now(),
            origin,
        });
    }

    pub(crate) fn tick_modal_shake(&mut self) {
        const SHAKE_DURATION_MS: u128 = 300;
        const SHAKE_AMPLITUDE: f64 = 8.0;
        const SHAKE_FREQUENCY: f64 = 3.0; // full oscillations

        let shake = match &self.modal_shake {
            Some(s) => s,
            None => return,
        };
        let elapsed_ms = shake.start.elapsed().as_millis();
        if elapsed_ms >= SHAKE_DURATION_MS {
            let origin = shake.origin;
            let modal_id = self.view.active_modal_id;
            self.modal_shake = None;
            if let Some(id) = modal_id
                && let Some(w) = self.view.views.get(&id)
            {
                w.base()
                    .winit
                    .set_outer_position(winit::dpi::PhysicalPosition::new(origin.x, origin.y));
            }
            return;
        }

        let t = elapsed_ms as f64 / SHAKE_DURATION_MS as f64;
        let offset_x = (SHAKE_AMPLITUDE
            * (t * SHAKE_FREQUENCY * 2.0 * std::f64::consts::PI).sin()
            * (1.0 - t)) as i32;
        let origin = shake.origin;
        if let Some(id) = self.view.active_modal_id
            && let Some(w) = self.view.views.get(&id)
        {
            w.base()
                .winit
                .set_outer_position(winit::dpi::PhysicalPosition::new(
                    origin.x + offset_x,
                    origin.y,
                ));
            w.base().winit.request_redraw();
        }
    }
}
