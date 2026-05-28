//! Modal "shake" 애니메이션 — 모달이 닫히기를 거부할 때 시각적 피드백.

use crate::app::{App, ModalShake};

impl App {
    /// Start a modal shake animation. No-op if already shaking.
    pub(crate) fn trigger_modal_shake(&mut self) {
        if self.modal_shake.is_some() {
            return;
        }
        let modal_id = match self.view.active_modal_id {
            Some(id) => id,
            None => return,
        };
        let origin = match self.windows.get(&modal_id) {
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

    /// Advance the modal shake animation. Called from about_to_wait.
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
            // Animation done — restore original position
            let origin = shake.origin;
            let modal_id = self.view.active_modal_id;
            self.modal_shake = None;
            if let Some(id) = modal_id {
                if let Some(w) = self.windows.get(&id) {
                    w.base()
                        .winit
                        .set_outer_position(winit::dpi::PhysicalPosition::new(origin.x, origin.y));
                }
            }
            return;
        }

        // Damped sine wave: amplitude * sin(freq * t) * (1 - t)
        let t = elapsed_ms as f64 / SHAKE_DURATION_MS as f64;
        let offset_x = (SHAKE_AMPLITUDE
            * (t * SHAKE_FREQUENCY * 2.0 * std::f64::consts::PI).sin()
            * (1.0 - t)) as i32;
        let origin = shake.origin;
        if let Some(id) = self.view.active_modal_id {
            if let Some(w) = self.windows.get(&id) {
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
}
