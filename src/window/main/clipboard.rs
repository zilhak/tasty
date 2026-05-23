use egui::ColorImage;

use super::MainWindow;

impl MainWindow {
    /// Paste a clipboard image into the focused ImagePanel as a floating selection.
    /// Returns true if an image was pasted.
    pub fn paste_to_image(&mut self) -> bool {
        let engine = &mut self.engine_state;
        let image = match &mut self.clipboard {
            Some(cb) => cb.get_image(),
            None => return false,
        };
        let image = match image {
            Some(img) => img,
            None => return false,
        };

        // Convert arboard::ImageData → egui::ColorImage
        let w = image.width;
        let h = image.height;
        let pixels: Vec<egui::Color32> = image
            .bytes
            .chunks_exact(4)
            .map(|c| egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
            .collect();
        let color_image = ColorImage {
            size: [w, h],
            pixels,
        };

        // Borrow conflict workaround: temporarily extract image_views so we can hold
        // `&mut ImageView` from the store while resolving the focused panel from
        // `engine.workspaces`.
        let mut image_views = std::mem::take(&mut self.state.image_views);
        let pasted = if let Some(panel) = self.state.focused_image_mut(engine) {
            let view = image_views.get_or_init(panel);
            view.paste_image(color_image);
            true
        } else {
            false
        };
        self.state.image_views = image_views;
        pasted
    }

    pub fn paste_to_terminal(&mut self) {
        let engine = &mut self.engine_state;
        // Try text first
        let text = match &mut self.clipboard {
            Some(cb) => cb.get_text(),
            None => None,
        };
        if let Some(text) = text {
            if !text.is_empty() {
                if let Some(terminal) = self.state.focused_terminal_mut(engine) {
                    if terminal.bracketed_paste() {
                        terminal.send_bytes(b"\x1b[200~");
                        terminal.send_key(&text);
                        terminal.send_bytes(b"\x1b[201~");
                    } else {
                        terminal.send_key(&text);
                    }
                    self.last_terminal_paste_at = Some(std::time::Instant::now());
                }
                return;
            }
        }

        // Fall back to image: save as PNG and paste the file path
        let image = match &mut self.clipboard {
            Some(cb) => cb.get_image(),
            None => None,
        };
        if let Some(image) = image {
            match save_clipboard_image_as_png(&image) {
                Ok(path) => {
                    if let Some(terminal) = self.state.focused_terminal_mut(engine) {
                        if terminal.bracketed_paste() {
                            terminal.send_bytes(b"\x1b[200~");
                            terminal.send_key(&path);
                            terminal.send_bytes(b"\x1b[201~");
                        } else {
                            terminal.send_key(&path);
                        }
                        self.last_terminal_paste_at = Some(std::time::Instant::now());
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to save clipboard image: {}", e);
                }
            }
        }
    }
}

/// Save clipboard image data as a PNG file in a temp directory.
/// Returns the absolute path to the saved file.
fn save_clipboard_image_as_png(image: &arboard::ImageData<'_>) -> anyhow::Result<String> {
    let dir = std::env::temp_dir().join("tasty-clipboard");
    std::fs::create_dir_all(&dir)?;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let path = dir.join(format!("paste-{}.png", timestamp));

    let file = std::fs::File::create(&path)?;
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        image.width as u32,
        image.height as u32,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&image.bytes)?;
    writer.finish()?;

    Ok(path.to_string_lossy().into_owned())
}
