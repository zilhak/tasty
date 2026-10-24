//! Opt-in, per-window debug fault injection through the real MainView render path.

use super::GpuState;

#[derive(Default)]
pub(super) struct Probe {
    initialized: bool,
    started: Option<std::time::Instant>,
    after: std::time::Duration,
    remaining: u32,
    error: Option<wgpu::SurfaceError>,
}

impl GpuState {
    pub(super) fn debug_main_frame(
        &mut self,
        window_id: winit::window::WindowId,
    ) -> Result<(), wgpu::SurfaceError> {
        if !self.surface_probe.initialized {
            self.surface_probe.initialized = true;
            self.surface_probe.started = Some(std::time::Instant::now());
            self.surface_probe.after = std::time::Duration::from_millis(
                std::env::var("TASTY_DEBUG_GPU_FAULT_AFTER_MS")
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0)
                    .min(60000),
            );
            // Bounded work, enabled only in an explicitly configured debug process.
            let repeats = std::env::var("TASTY_DEBUG_GPU_RESIZE_REPEATS")
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0)
                .min(1000);
            for _ in 0..repeats {
                self.resize(self.size);
            }
            let fault = std::env::var("TASTY_DEBUG_GPU_FAULT").unwrap_or_default();
            if let Some((kind, count)) = fault.split_once(':') {
                self.surface_probe.remaining = count.parse::<u32>().unwrap_or(0).min(1000);
                self.surface_probe.error = match kind {
                    "timeout" => Some(wgpu::SurfaceError::Timeout),
                    "lost" => Some(wgpu::SurfaceError::Lost),
                    "outdated" => Some(wgpu::SurfaceError::Outdated),
                    "oom" => Some(wgpu::SurfaceError::OutOfMemory),
                    _ => None,
                };
            }
        }
        if self
            .surface_probe
            .started
            .is_some_and(|t| t.elapsed() < self.surface_probe.after)
        {
            return Ok(());
        }
        if self.surface_probe.remaining > 0
            && let Some(error) = self.surface_probe.error.clone()
        {
            self.surface_probe.remaining -= 1;
            if matches!(
                error,
                wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated
            ) {
                self.surface_invalid = true;
            }
            tracing::info!(target: "tasty::gpu::surface", phase = "injected_acquire",
                    window_id = u64::from(window_id), remaining = self.surface_probe.remaining, ?error);
            return Err(error);
        }
        Ok(())
    }
}
