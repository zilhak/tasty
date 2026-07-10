use crate::model::{PhysicalPx, PhysicalRect};

use super::GpuState;

impl GpuState {
    /// Capture a texture of the given pixel dimensions to a PNG file.
    ///
    /// The texture must be a non-sRGB BGRA target (swapchain frame or offscreen
    /// surface texture) with `COPY_SRC` usage. `width`/`height` are the texture's
    /// pixel size — for the swapchain pass this is `self.size`, for an offscreen
    /// surface capture it is the surface's own pixel size.
    pub(super) fn capture_frame_to_png(
        &self,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
        path: &std::path::Path,
    ) {
        let bytes_per_pixel = 4u32;
        // wgpu requires rows to be aligned to 256 bytes
        let unpadded_bytes_per_row = width * bytes_per_pixel;
        let padded_bytes_per_row = (unpadded_bytes_per_row + 255) & !255;

        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screenshot_buffer"),
            size: (padded_bytes_per_row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("screenshot_encoder"),
            });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(std::iter::once(encoder.finish()));

        // Map the buffer and read pixels
        let buffer_slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            if let Err(e) = tx.send(result) {
                tracing::warn!("screenshot map_async result send failed: {e}");
            }
        });
        self.device.poll(wgpu::Maintain::Wait);

        if let Ok(Ok(())) = rx.recv() {
            let data = buffer_slice.get_mapped_range();

            // Convert BGRA -> RGB for PNG encoding
            let mut pixels = Vec::with_capacity((width * height * 3) as usize);
            for row in 0..height {
                let offset = (row * padded_bytes_per_row) as usize;
                for col in 0..width {
                    let px = offset + (col * bytes_per_pixel) as usize;
                    // BGRA → RGB
                    pixels.push(data[px + 2]); // R
                    pixels.push(data[px + 1]); // G
                    pixels.push(data[px]); // B
                }
            }
            drop(data);
            buffer.unmap();

            // Write as PNG
            if let Ok(file) = std::fs::File::create(path) {
                let writer = std::io::BufWriter::new(file);
                let mut encoder = png::Encoder::new(writer, width, height);
                encoder.set_color(png::ColorType::Rgb);
                encoder.set_depth(png::BitDepth::Eight);
                if let Ok(mut writer) = encoder.write_header() {
                    if let Err(e) = writer.write_image_data(&pixels) {
                        tracing::warn!("screenshot write_image_data failed: {e}");
                    } else {
                        tracing::info!("screenshot saved to {}", path.display());
                    }
                }
            }
        } else {
            tracing::warn!("failed to capture screenshot");
        }
    }

    /// Capture a single terminal surface to PNG via an offscreen render pass.
    ///
    /// The surface is drawn to a dedicated texture sized to its own terminal grid
    /// (`cols × rows` cells), independent of the swapchain, the visible tab, and
    /// focus — nothing about the on-screen frame or user state changes. This is
    /// the focus-independent primitive behind the agent `ui.screenshot`
    /// `{surface_id}` path.
    ///
    /// The shared renderer's projection uniform is briefly retargeted to the
    /// offscreen size and restored to `self.size` before returning, so the next
    /// visible frame renders unchanged.
    pub(super) fn capture_surface_to_png(
        &mut self,
        terminal: &tasty_terminal::Terminal,
        reverse_screen_enabled: bool,
        path: &std::path::Path,
    ) {
        let (cols, rows) = terminal.dimensions();
        let cw = self.renderer.cell_width();
        let ch = self.renderer.cell_height();
        let max_dim = self.device.limits().max_texture_dimension_2d;
        let width = ((cols as f32 * cw).ceil() as u32).clamp(1, max_dim);
        let height = ((rows as f32 * ch).ceil() as u32).clamp(1, max_dim);

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("surface_screenshot_target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Same format as the swapchain so the terminal pipelines and the
            // BGRA→RGB readback in `capture_frame_to_png` stay valid.
            format: self.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Retarget the projection uniform to the offscreen size (rect placed at
        // origin below). Restored to `self.size` at the end of this method.
        self.renderer.resize(&self.queue, width, height);

        let theme = crate::theme::theme();
        let term_surface = theme.surface("terminal");
        let ansi = theme.ansi_palette();
        let bg = term_surface.unfocused_bg.to_gpu_rgba();
        let fg = term_surface.unfocused_fg.to_gpu_rgba();
        let clear = theme.bg_panel().to_gpu_rgba();

        self.renderer.begin_frame();
        let rect = PhysicalRect {
            x: PhysicalPx(0.0),
            y: PhysicalPx(0.0),
            width: PhysicalPx(width as f32),
            height: PhysicalPx(height as f32),
        };
        self.renderer.append_terminal_viewport(
            terminal,
            &self.queue,
            &rect,
            &ansi,
            bg,
            fg,
            false, // no cursor overlay in a static capture
            None,  // selection
            None,  // vi cursor
            None,  // preedit
            None,  // link hover
            None,  // search highlights
            reverse_screen_enabled,
        );
        self.renderer.flush_buffers(&self.device, &self.queue);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("surface_screenshot_pass"),
            });
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("surface_screenshot_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear.r() as f64,
                            g: clear.g() as f64,
                            b: clear.b() as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.renderer.render_all(&mut render_pass, width, height);
        }
        self.queue.submit(std::iter::once(encoder.finish()));

        self.capture_frame_to_png(&texture, width, height, path);

        // Restore the projection uniform for the visible frame.
        self.renderer
            .resize(&self.queue, self.size.width, self.size.height);
    }
}
