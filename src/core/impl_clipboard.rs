//! OSC 52 클립보드 읽기 요청을 처리한다.

use super::*;

impl Core {
    /// 설정이 허용할 때만 로컬 클립보드를 읽고 답한다. 기본값은 거절이며 이때 응답 바이트도 보내지 않는다.
    pub(super) fn handle_clipboard_query(&mut self, engine: &mut crate::core::CoreState, sid: u32) {
        let allow = engine.settings.general.allow_clipboard_read;
        let clip = if allow {
            match self.clipboard.read_text() {
                Ok(t) => Some(t),
                Err(e) => {
                    tracing::warn!("OSC 52 clipboard read failed: {e}");
                    None
                }
            }
        } else {
            None
        };
        if let Some(reply) = osc52_clipboard_read_reply(allow, clip.as_deref())
            && let Some(terminal) = engine.find_terminal_by_id_mut(sid)
        {
            terminal.send_bytes(&reply);
        }
    }
}

/// 허용된 텍스트를 base64로 감싼 OSC 52 응답(BEL 종료)을 만든다. 거절 또는 텍스트 부재면 None이다.
fn osc52_clipboard_read_reply(allow: bool, clipboard_text: Option<&str>) -> Option<Vec<u8>> {
    if !allow {
        return None;
    }
    let text = clipboard_text?;
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    Some(format!("\x1b]52;c;{encoded}\x07").into_bytes())
}

#[cfg(test)]
mod osc52_clipboard_read_tests {
    use super::osc52_clipboard_read_reply;

    #[test]
    fn off_emits_no_bytes() {
        assert_eq!(osc52_clipboard_read_reply(false, Some("secret")), None);
        assert_eq!(osc52_clipboard_read_reply(false, None), None);
    }

    #[test]
    fn on_encodes_clipboard_as_osc52() {
        let reply = osc52_clipboard_read_reply(true, Some("hi")).expect("reply when allowed");
        assert_eq!(reply, b"\x1b]52;c;aGk=\x07".to_vec());
    }

    #[test]
    fn on_with_empty_clipboard_still_replies() {
        assert_eq!(osc52_clipboard_read_reply(true, None), None);
    }
}
