//! Primary-surface acquisition and resize instrumentation.

use std::time::Instant;

use super::{GpuState, clamp_surface_dims};
use winit::dpi::PhysicalSize;

fn configure_dimensions(
    current: PhysicalSize<u32>,
    requested: PhysicalSize<u32>,
    max: u32,
    force: bool,
) -> Option<(u32, u32)> {
    if requested.width == 0 || requested.height == 0 {
        return None;
    }
    let dims = clamp_surface_dims(requested.width, requested.height, max);
    (force || dims != (current.width, current.height)).then_some(dims)
}

impl GpuState {
    /// Ordinary size notification. Scale, fonts and PTY geometry remain caller-owned.
    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        self.configure_surface(size, self.surface_invalid);
    }

    /// Recover a lost/outdated surface even when its dimensions did not change.
    pub fn reconfigure(&mut self, size: PhysicalSize<u32>) {
        self.configure_surface(size, true);
    }

    fn configure_surface(&mut self, requested: PhysicalSize<u32>, force: bool) {
        let max = self.device.limits().max_texture_dimension_2d;
        let Some((width, height)) = configure_dimensions(self.size, requested, max, force) else {
            if requested.width != 0 && requested.height != 0 {
                // Native placement can change at identical physical dimensions (DPI/move).
                self.follow_overlay_host();
            }
            return;
        };
        if (width, height) != (requested.width, requested.height) {
            tracing::warn!(
                req_w = requested.width,
                req_h = requested.height,
                max,
                "surface resize request exceeds max_texture_dimension_2d; clamping"
            );
        }
        self.size = PhysicalSize::new(width, height);
        self.config.width = width;
        self.config.height = height;
        let start = Instant::now();
        self.surface.configure(&self.device, &self.config);
        self.surface_invalid = false;
        tracing::debug!(target: "tasty::gpu::surface", phase = "configure", width, height, force,
            elapsed_us = start.elapsed().as_micros() as u64);
        self.resize_overlay_surface(width, height);
        self.renderer.resize(&self.queue, width, height);
    }

    pub(super) fn submit_commands(&self, commands: impl IntoIterator<Item = wgpu::CommandBuffer>) {
        let start = Instant::now();
        self.queue.submit(commands);
        tracing::debug!(target: "tasty::gpu::surface", phase = "submit",
            elapsed_us = start.elapsed().as_micros() as u64);
    }

    pub(super) fn acquire_surface(&mut self) -> Result<wgpu::SurfaceTexture, wgpu::SurfaceError> {
        let start = Instant::now();
        let result = self.surface.get_current_texture();
        if matches!(
            result,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated)
        ) {
            self.surface_invalid = true;
        }
        tracing::debug!(target: "tasty::gpu::surface", phase = "acquire",
            elapsed_us = start.elapsed().as_micros() as u64, error = ?result.as_ref().err());
        result
    }

    pub(super) fn present_surface(&self, output: wgpu::SurfaceTexture) {
        let start = Instant::now();
        output.present();
        tracing::debug!(target: "tasty::gpu::surface", phase = "present",
            elapsed_us = start.elapsed().as_micros() as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unchanged_valid_dimensions_skip_but_recovery_forces_configure() {
        let size = PhysicalSize::new(1000, 700);
        assert_eq!(configure_dimensions(size, size, 4096, false), None);
        assert_eq!(
            configure_dimensions(size, size, 4096, true),
            Some((1000, 700))
        );
    }
    #[test]
    fn real_resize_and_clamped_equivalence_are_distinct() {
        let size = PhysicalSize::new(4096, 700);
        assert_eq!(
            configure_dimensions(size, PhysicalSize::new(65535, 700), 4096, false),
            None
        );
        assert_eq!(
            configure_dimensions(size, PhysicalSize::new(800, 600), 4096, false),
            Some((800, 600))
        );
    }
    #[test]
    fn zero_dimensions_never_configure_even_during_recovery() {
        for size in [PhysicalSize::new(0, 10), PhysicalSize::new(10, 0)] {
            assert_eq!(
                configure_dimensions(PhysicalSize::new(10, 10), size, 4096, true),
                None
            );
        }
    }
}
