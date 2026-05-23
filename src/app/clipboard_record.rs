//! 백그라운드 폴링 스레드가 감지한 클립보드 데이터를 모든 engine 의 history 에 기록.

use crate::app::App;

impl App {
    /// Record clipboard data from the background polling thread into all engines.
    pub(crate) fn record_clipboard_data(&mut self, data: crate::ClipboardData) {
        let source = crate::clipboard_history::ClipboardSource::System;
        let engines = self
            .windows
            .values_mut()
            .filter_map(|w| w.as_main_mut())
            .map(|m| &mut m.engine_state)
            .chain(self.parked_states.iter_mut().map(|(_, e)| e));
        for engine in engines {
            if !engine.settings.clipboard.history_enabled {
                continue;
            }
            match &data {
                crate::ClipboardData::Text(text) => {
                    engine.clipboard_history.record(text.clone(), source);
                }
                crate::ClipboardData::Image(img) => {
                    engine.clipboard_history.record_image(img.clone(), source);
                }
            }
        }
        // Event Bus 1.0: `clipboard.copied` 발화. 시스템 클립보드 변경은 OS 차원
        // 단일 이벤트라 모든 plugin에 1회만 broadcast. image의 base64는 전송 비용이
        // 커서 None으로 두고, plugin은 필요 시 `clipboard.get_*` IPC로 가져가게 한다.
        if let Some(mgr) = self.plugin_manager.as_mut() {
            use tasty_plugin_protocol::EventScope;
            use tasty_plugin_protocol::events::payloads::{ClipboardCopied, ClipboardKind};
            let timestamp_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let payload = match &data {
                crate::ClipboardData::Text(text) => ClipboardCopied {
                    kind: ClipboardKind::Text,
                    text: Some(text.clone()),
                    image_b64: None,
                    timestamp_ms,
                },
                crate::ClipboardData::Image(_) => ClipboardCopied {
                    kind: ClipboardKind::Image,
                    text: None,
                    image_b64: None,
                    timestamp_ms,
                },
            };
            mgr.emit_host_event("clipboard.copied", &payload, EventScope::System);
        }
    }
}
