//! WebView2 엔진 편집 명령 실행.

use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;
use windows::core::{HSTRING, w};

/// WebView2에는 편집 명령 API가 없어 DevTools `Input.dispatchKeyEvent`의 `commands`로 엔진 편집
/// 명령을 실행한다. 페이지 스크립트 허용과 무관하게 포커스 frame에서 실행되며, 선택 없는 복사나
/// 읽기 전용 대상의 잘라내기·붙여넣기는 아무것도 바꾸지 않는다.
pub(super) fn run_editing_command(
    webview: &ICoreWebView2,
    action: super::super::WebViewEditAction,
    surface_id: u32,
) {
    use super::super::WebViewEditAction as A;
    let command = match action {
        A::Copy => "copy",
        A::Cut => "cut",
        A::Paste => "paste",
        A::SelectAll => "selectAll",
        A::Undo => "undo",
        A::Redo => "redo",
    };
    let params = HSTRING::from(format!(
        r#"{{"type":"rawKeyDown","commands":["{command}"]}}"#
    ));
    let done =
        CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, _json| {
            if let Err(e) = result {
                tracing::warn!("WebView surface {surface_id}: edit command {command} failed: {e}");
            }
            Ok(())
        }));
    // SAFETY: GUI 스레드에서 살아 있는 WebView에 호출하며 HSTRING은 호출 동안 유지된다.
    let sent =
        unsafe { webview.CallDevToolsProtocolMethod(w!("Input.dispatchKeyEvent"), &params, &done) };
    if let Err(e) = sent {
        tracing::warn!("WebView surface {surface_id}: edit command {command} failed: {e}");
    }
}
