//! 시스템 클립보드 백그라운드 폴링 스레드.
//!
//! 변경 감지 시 `AppEvent::ClipboardChanged` 를 winit event loop 로 송신.
//! poll interval 은 부팅 시점의 settings 값 — runtime 변경은 재시작 후 반영된다.

use winit::event_loop::EventLoopProxy;

use crate::clipboard::encode::encode_clipboard_image;
use crate::{AppEvent, ClipboardData};

pub(crate) fn spawn(proxy: EventLoopProxy<AppEvent>) {
    let poll_interval_ms = crate::settings::Settings::load()
        .clipboard
        .poll_interval_ms
        .max(100);
    std::thread::spawn(move || {
        let interval = std::time::Duration::from_millis(poll_interval_ms);
        let mut last_text: Option<String> = None;
        loop {
            std::thread::sleep(interval);
            let Some(mut cb) = arboard::Clipboard::new().ok() else {
                continue;
            };
            // Try text first, then image
            if let Ok(text) = cb.get_text() {
                if !text.is_empty() {
                    let changed = last_text.as_ref() != Some(&text);
                    if changed {
                        last_text = Some(text.clone());
                        if proxy
                            .send_event(AppEvent::ClipboardChanged(ClipboardData::Text(text)))
                            .is_err()
                        {
                            break;
                        }
                    }
                    continue;
                }
            }
            if let Ok(img) = cb.get_image() {
                if let Some(data) = encode_clipboard_image(&img) {
                    last_text = None;
                    if proxy
                        .send_event(AppEvent::ClipboardChanged(ClipboardData::Image(data)))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
}
