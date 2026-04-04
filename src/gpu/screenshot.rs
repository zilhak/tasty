use super::GpuState;

impl GpuState {
    /// Capture the current frame texture to a PNG file.
    pub(super) fn capture_frame_to_png(&self, texture: &wgpu::Texture, path: &std::path::Path) {
        let width = self.size.width;
        let height = self.size.height;
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

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
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
            let _ = tx.send(result);
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
                    pixels.push(data[px]);     // B
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
                    let _ = writer.write_image_data(&pixels);
                    tracing::info!("screenshot saved to {}", path.display());
                }
            }
        } else {
            tracing::warn!("failed to capture screenshot");
        }
    }
}
