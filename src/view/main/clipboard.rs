use super::MainView;
use crate::runtime::engine_read::EngineRead;
use crate::app::command::{DomainIntent, SendPayload};

/// bracketed paste 시작·본문·끝을 같은 큐 순서로 보낸다.
/// mirror 입력도 원격 PTY로 전달하므로 이미지 업로드 뒤 원격 경로 삽입에 재사용한다.
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

/// The complete paste shares one queued generation check, including both bracket markers.
pub(crate) fn dispatch_bound_paste(view:&mut MainView,surface_id:u32,generation:tasty_terminal::ResourceGeneration,bracketed:bool,text:String) {
    let mut bytes=Vec::new();
    if bracketed {bytes.extend_from_slice(b"\x1b[200~");}
    bytes.extend_from_slice(text.as_bytes());
    if bracketed {bytes.extend_from_slice(b"\x1b[201~");}
    view.state.dispatch_intent(DomainIntent::SendToSurface {surface_id,payload:SendPayload::Bound {generation,bytes}}.from_user_shortcut("paste"));
}

impl MainView {
    pub fn paste_to_terminal(&mut self, engine: &EngineRead<'_>) {
        let text = match &mut self.clipboard {
            Some(cb) => cb.get_text(),
            None => None,
        };
        if let Some(text) = text
            && !text.is_empty()
        {
            let surface_id = self.state.focused_surface_id(&*engine);
            let bracketed = self
                .state
                .focused_terminal(&engine.as_ref())
                .map(|t| t.bracketed_paste());
            if let (Some(sid), Some(bracketed)) = (surface_id, bracketed) {
                dispatch_paste(self, sid, bracketed, text);
                self.last_terminal_paste_at = Some(std::time::Instant::now());
            }
            return;
        }

        let image = match &mut self.clipboard {
            Some(cb) => cb.get_image(),
            None => None,
        };
        let Some(image) = image else {
            return;
        };
        let Some(sid) = self.state.focused_surface_id(&*engine) else {
            return;
        };
        let Some(bracketed) = self
            .state
            .focused_terminal(&engine.as_ref())
            .map(|t| t.bracketed_paste())
        else {
            return;
        };

        let Some(target)=crate::app::engine_action::SurfaceBinding::capture(engine,sid) else {return;};
        match encode_clipboard_image_as_png(&image) {
            Ok(png_bytes)=>{
                self.state.dispatch_intent(crate::intent::Intent::Engine(crate::app::engine_action::EngineAction::PasteImage {target,view:self.base.state.identity(),bracketed,file_name:clipboard_image_file_name(),png_bytes}).from_user_shortcut("paste"));
                self.last_terminal_paste_at=Some(std::time::Instant::now());
            },
            Err(error)=>tracing::warn!(%error,"clipboard image encoding failed"),
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
/// mirror 업로드는 파일을 거치지 않고 이 바이트를 bulk 파일 전송 채널로 바로 올린다.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// PNG로 인코딩한 크기·RGBA 픽셀이 원본과 같은지 확인한다.
    #[test]
    fn encode_clipboard_image_as_png_roundtrips_rgba() {
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

        assert_eq!(&png_bytes[..8], b"\x89PNG\r\n\x1a\n", "PNG 시그니처");

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
