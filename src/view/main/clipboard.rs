use super::MainView;
use crate::core::intent::{DomainIntent, SendPayload};

/// Bracketed paste 분기 후 SendPayload 묶음 발행. FIFO 순서 보장 (큐 push 순서
/// 그대로 Core::apply 가 처리하므로 200~ → text → 201~ 순서로 PTY write).
///
/// mirror surface 에 대해서도 그대로 쓸 수 있다 — `SendToSurface` 는 detached mirror
/// 터미널의 `input_sink` → forwarder → 원격 PTY stdin 으로 투명 전달되므로, 08 의 원격
/// 경로 삽입(비동기 업로드 완료 후)도 이 진입점을 재사용한다.
pub(crate) fn dispatch_paste(w: &mut MainView, surface_id: u32, bracketed: bool, text: String) {
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

        // Fall back to image.
        let image = match &mut self.clipboard {
            Some(cb) => cb.get_image(),
            None => None,
        };
        let Some(image) = image else {
            return;
        };
        let Some(sid) = self.state.focused_surface_id(&self.core_state) else {
            return;
        };
        let Some(bracketed) = self
            .state
            .focused_terminal(&self.core_state)
            .map(|t| t.bracketed_paste())
        else {
            return;
        };

        // (08) mirror 판정: focused surface 가 mirror workspace 소속이면 그 로컬 ws id.
        // 로컬 PNG 경로는 원격에서 무의미하므로, mirror 면 원격 업로드 후 원격 경로를
        // 삽입한다(비동기). 비-mirror 는 기존 로컬 경로 삽입을 그대로 유지.
        let mirror_ws_id = self
            .core_state
            .find_workspace_index_for_surface(sid)
            .and_then(|(idx, _)| self.core_state.workspaces.get(idx))
            .and_then(|ws| ws.mirror.then_some(ws.id));

        match mirror_ws_id {
            Some(ws_id) => {
                // mirror: PNG 바이트를 메모리에서 확보해 06 bulk 채널 업로드 트리거 큐에
                // 넣는다. 실제 업로드(블로킹)와 원격 경로 삽입은 App 이 백그라운드에서 처리.
                match encode_clipboard_image_as_png(&image) {
                    Ok(png_bytes) => {
                        self.core_state.pending_image_uploads.push(
                            crate::core::PendingImageUpload {
                                mirror_ws_id: ws_id,
                                surface_id: sid,
                                bracketed,
                                file_name: clipboard_image_file_name(),
                                png_bytes,
                            },
                        );
                        self.last_terminal_paste_at = Some(std::time::Instant::now());
                    }
                    Err(e) => {
                        tracing::warn!("Failed to encode clipboard image for mirror upload: {e}");
                    }
                }
            }
            None => {
                // 로컬(비-mirror): 기존 동작 — 로컬 temp PNG 저장 후 로컬 경로 삽입.
                match save_clipboard_image_as_png(&image) {
                    Ok(path) => {
                        dispatch_paste(self, sid, bracketed, path);
                        self.last_terminal_paste_at = Some(std::time::Instant::now());
                    }
                    Err(e) => {
                        tracing::warn!("Failed to save clipboard image: {}", e);
                    }
                }
            }
        }
    }
}

/// `paste-<ms>.png` 파일명 규약 — 로컬 temp 파일명과 원격 저장 basename 이 공유.
fn clipboard_image_file_name() -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("paste-{}.png", timestamp)
}

/// Encode clipboard image data (RGBA) as PNG into an in-memory byte buffer.
/// mirror 업로드는 파일을 거치지 않고 이 바이트를 06 채널로 바로 올린다.
fn encode_clipboard_image_as_png(image: &arboard::ImageData<'_>) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut buf, image.width as u32, image.height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&image.bytes)?;
        writer.finish()?;
    }
    Ok(buf)
}

/// Save clipboard image data as a PNG file in a temp directory.
/// Returns the absolute path to the saved file.
fn save_clipboard_image_as_png(image: &arboard::ImageData<'_>) -> anyhow::Result<String> {
    // 이유: 디렉터리는 의도된 공유다 — 격리는 파일명이 진다. 안에 쓰는 파일은
    // `clipboard_image_file_name()` 이 `paste-<millis>.png` 로 매번 다르게 짓는다.
    let dir = std::env::temp_dir().join("tasty-clipboard");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(clipboard_image_file_name());
    let bytes = encode_clipboard_image_as_png(image)?;
    std::fs::write(&path, &bytes)?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (08) mirror 업로드가 파일을 거치지 않고 06 채널에 실을 PNG 바이트가, 유효한
    /// PNG 시그니처를 갖고 원본 RGBA 픽셀·크기 그대로 라운드트립되는지 검증한다.
    #[test]
    fn encode_clipboard_image_as_png_roundtrips_rgba() {
        // 2x2 RGBA: 빨강/초록/파랑/투명.
        let rgba: Vec<u8> = vec![
            255, 0, 0, 255, // (0,0) red
            0, 255, 0, 255, // (1,0) green
            0, 0, 255, 255, // (0,1) blue
            0, 0, 0, 0, // (1,1) transparent
        ];
        let image = arboard::ImageData {
            width: 2,
            height: 2,
            bytes: std::borrow::Cow::Owned(rgba.clone()),
        };

        let png_bytes = encode_clipboard_image_as_png(&image).expect("encode");

        // PNG 매직 시그니처.
        assert_eq!(&png_bytes[..8], b"\x89PNG\r\n\x1a\n", "PNG 시그니처");

        // 디코드해 크기·픽셀이 원본과 동일한지.
        let decoder = png::Decoder::new(std::io::Cursor::new(&png_bytes));
        let mut reader = decoder.read_info().expect("read_info");
        let mut out = vec![0u8; rgba.len()];
        let info = reader.next_frame(&mut out).expect("next_frame");
        assert_eq!(info.width, 2);
        assert_eq!(info.height, 2);
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(out, rgba, "RGBA 픽셀 동일");
    }

    /// 파일명 규약이 `paste-` 접두 + `.png` 확장자를 유지하는지(원격 저장 basename 공유).
    #[test]
    fn clipboard_image_file_name_convention() {
        let name = clipboard_image_file_name();
        assert!(name.starts_with("paste-"), "paste- 접두: {name}");
        assert!(name.ends_with(".png"), ".png 확장자: {name}");
    }
}
