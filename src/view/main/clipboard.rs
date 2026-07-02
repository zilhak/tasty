use super::MainView;
use crate::core::intent::{DomainIntent, SendPayload};

/// Bracketed paste 분기 후 SendPayload 묶음 발행. FIFO 순서 보장 (큐 push 순서
/// 그대로 Core::apply 가 처리하므로 200~ → text → 201~ 순서로 PTY write).
fn dispatch_paste(w: &mut MainView, surface_id: u32, bracketed: bool, text: String) {
    if text.is_empty() {
        return;
    }
    if bracketed {
        w.state.dispatch_intent(
            DomainIntent::SendToSurface {
                surface_id,
                payload: SendPayload::Bytes(b"\x1b[200~".to_vec()),
            }
            .from_user_shortcut("paste"),
        );
        w.state.dispatch_intent(
            DomainIntent::SendToSurface {
                surface_id,
                payload: SendPayload::Text(text),
            }
            .from_user_shortcut("paste"),
        );
        w.state.dispatch_intent(
            DomainIntent::SendToSurface {
                surface_id,
                payload: SendPayload::Bytes(b"\x1b[201~".to_vec()),
            }
            .from_user_shortcut("paste"),
        );
    } else {
        w.state.dispatch_intent(
            DomainIntent::SendToSurface {
                surface_id,
                payload: SendPayload::Text(text),
            }
            .from_user_shortcut("paste"),
        );
    }
}

impl MainView {
    pub fn paste_to_terminal(&mut self) {
        // Try text first
        let text = match &mut self.clipboard {
            Some(cb) => cb.get_text(),
            None => None,
        };
        if let Some(text) = text
            && !text.is_empty()
        {
            let surface_id = self.state.focused_surface_id(&self.core_state);
            let bracketed = self
                .state
                .focused_terminal(&self.core_state)
                .map(|t| t.bracketed_paste());
            if let (Some(sid), Some(bracketed)) = (surface_id, bracketed) {
                dispatch_paste(self, sid, bracketed, text);
                self.last_terminal_paste_at = Some(std::time::Instant::now());
            }
            return;
        }

        // Fall back to image: save as PNG and paste the file path
        let image = match &mut self.clipboard {
            Some(cb) => cb.get_image(),
            None => None,
        };
        if let Some(image) = image {
            match save_clipboard_image_as_png(&image) {
                Ok(path) => {
                    let surface_id = self.state.focused_surface_id(&self.core_state);
                    let bracketed = self
                        .state
                        .focused_terminal(&self.core_state)
                        .map(|t| t.bracketed_paste());
                    if let (Some(sid), Some(bracketed)) = (surface_id, bracketed) {
                        dispatch_paste(self, sid, bracketed, path);
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
