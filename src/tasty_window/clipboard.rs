use super::TastyWindow;

impl TastyWindow {
    pub fn paste_to_terminal(&mut self) {
        // Try text first
        let text = match &mut self.clipboard {
            Some(cb) => cb.get_text(),
            None => None,
        };
        if let Some(text) = text {
            if !text.is_empty() {
                if let Some(terminal) = self.state.focused_terminal_mut() {
                    if terminal.bracketed_paste() {
                        terminal.send_bytes(b"\x1b[200~");
                        terminal.send_key(&text);
                        terminal.send_bytes(b"\x1b[201~");
                    } else {
                        terminal.send_key(&text);
                    }
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
                    if let Some(terminal) = self.state.focused_terminal_mut() {
                        if terminal.bracketed_paste() {
                            terminal.send_bytes(b"\x1b[200~");
                            terminal.send_key(&path);
                            terminal.send_bytes(b"\x1b[201~");
                        } else {
                            terminal.send_key(&path);
                        }
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
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), image.width as u32, image.height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&image.bytes)?;
    writer.finish()?;

    Ok(path.to_string_lossy().into_owned())
}
