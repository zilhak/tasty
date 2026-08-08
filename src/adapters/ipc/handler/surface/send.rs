use serde_json::json;

use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

/// Parse key combo strings like "ctrl+c", "ctrl+shift+c", "alt+x" into terminal bytes.
///
/// 왼쪽부터 `ctrl+`/`shift+`/`alt+` 프리픽스를 벗겨내고 남은 부분을 키 토큰으로 본다.
/// `split('+')`을 쓰지 않는 이유는 `"ctrl++"`(Ctrl+`+`)처럼 키와 구분자가 충돌하는
/// 경우를 올바르게 해석하기 위함. `"plus"`/`"minus"`/`"equals"` 같은 심볼 이름도
/// 허용한다.
fn parse_key_combo(input: &str) -> Option<Vec<u8>> {
    if input.is_empty() {
        return None;
    }

    let mut has_ctrl = false;
    // shift-only 시퀀스는 현재 미지원 — 프리픽스만 떼어내고 modifier로는 사용하지 않는다.
    let mut _has_shift = false;
    let mut has_alt = false;
    let mut rest = input;
    loop {
        let lower = rest.to_ascii_lowercase();
        if !has_ctrl && lower.starts_with("ctrl+") {
            has_ctrl = true;
            rest = &rest[5..];
        } else if !_has_shift && lower.starts_with("shift+") {
            _has_shift = true;
            rest = &rest[6..];
        } else if !has_alt && lower.starts_with("alt+") {
            has_alt = true;
            rest = &rest[4..];
        } else {
            break;
        }
    }

    if rest.is_empty() {
        return None;
    }
    if matches!(rest.to_ascii_lowercase().as_str(), "ctrl" | "shift" | "alt") {
        return None;
    }
    if !has_ctrl && !has_alt {
        return None;
    }

    // 심볼 이름을 단일 문자로 정규화.
    let key: &str = match rest.to_ascii_lowercase().as_str() {
        "plus" => "+",
        "minus" => "-",
        "equals" => "=",
        _ => rest,
    };

    let mut bytes = Vec::new();

    if has_ctrl && key.chars().count() == 1 {
        let ch = key.chars().next()?.to_ascii_lowercase();
        if ch.is_ascii_lowercase() {
            if has_alt {
                bytes.push(0x1B);
            }
            bytes.push(ch as u8 - b'a' + 1);
            return Some(bytes);
        } else if ch == '[' {
            bytes.push(0x1B);
            return Some(bytes);
        } else if ch == '\\' {
            bytes.push(0x1C);
            return Some(bytes);
        } else if ch == ']' {
            bytes.push(0x1D);
            return Some(bytes);
        }
    }

    if has_alt && !has_ctrl && key.chars().count() == 1 {
        bytes.push(0x1B);
        bytes.extend_from_slice(key.as_bytes());
        return Some(bytes);
    }

    None
}

/// [`dispatch_send`] 결과 — hard-occupied(attach 로 잠긴)와 "진짜 없음"을 구분한다.
/// 둘을 같은 "Surface not found" 메시지로 뭉뚱그리면 attach 로 점유된(하지만
/// `list`류엔 여전히 보이는) surface 를 존재하지 않는다고 오인하게 된다(Gate4
/// 판단필요 항목).
enum SendOutcome {
    Sent,
    HardOccupied,
    NotFound,
}

/// `SendOutcome::HardOccupied`/`NotFound` 를 각기 다른 메시지로. 호출부 전체가
/// 공유(문구 일관성).
fn send_fail_message(surface_id: u32, outcome: &SendOutcome) -> String {
    match outcome {
        SendOutcome::HardOccupied => format!(
            "Surface {surface_id} is hard-occupied (remote attach holds it) — \
             server-local input is blocked while attached"
        ),
        _ => format!("Surface {surface_id} not found"),
    }
}

/// 공용 dispatch — DomainIntent::SendToSurface 발화 후 결과 반환.
fn dispatch_send(
    core: &mut crate::core::Core,
    engine: &mut crate::core::CoreState,
    surface_id: u32,
    payload: crate::core::intent::SendPayload,
) -> SendOutcome {
    let intent = crate::core::intent::DomainIntent::SendToSurface {
        surface_id,
        payload,
    };
    let events = match core.apply(engine, intent) {
        Ok(e) => e,
        Err(_) => return SendOutcome::NotFound,
    };
    match events.into_iter().next() {
        Some(crate::core::intent::CoreEvent::SurfaceSent { sent: true, .. }) => SendOutcome::Sent,
        Some(crate::core::intent::CoreEvent::SurfaceSent {
            hard_occupied: true,
            ..
        }) => SendOutcome::HardOccupied,
        _ => SendOutcome::NotFound,
    }
}

pub(crate) fn handle_surface_send(
    core: &mut crate::core::Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let text = match params.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'text' parameter"),
    };
    match dispatch_send(
        core,
        engine,
        surface_id,
        crate::core::intent::SendPayload::Text(text),
    ) {
        SendOutcome::Sent => {
            JsonRpcResponse::success(id, json!({ "sent": true, "surface_id": surface_id }))
        }
        outcome => JsonRpcResponse::invalid_params(id, send_fail_message(surface_id, &outcome)),
    }
}

pub(crate) fn handle_surface_send_key(
    core: &mut crate::core::Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let key = match params.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'key' parameter"),
    };

    let bytes: Vec<u8> = match key {
        "enter" => b"\r".to_vec(),
        "tab" => b"\t".to_vec(),
        "escape" | "esc" => b"\x1b".to_vec(),
        "backspace" => b"\x7f".to_vec(),
        "up" => b"\x1b[A".to_vec(),
        "down" => b"\x1b[B".to_vec(),
        "right" => b"\x1b[C".to_vec(),
        "left" => b"\x1b[D".to_vec(),
        "home" => b"\x1b[H".to_vec(),
        "end" => b"\x1b[F".to_vec(),
        "pageup" => b"\x1b[5~".to_vec(),
        "pagedown" => b"\x1b[6~".to_vec(),
        "delete" => b"\x1b[3~".to_vec(),
        "insert" => b"\x1b[2~".to_vec(),
        "f1" => b"\x1bOP".to_vec(),
        "f2" => b"\x1bOQ".to_vec(),
        "f3" => b"\x1bOR".to_vec(),
        "f4" => b"\x1bOS".to_vec(),
        "f5" => b"\x1b[15~".to_vec(),
        "f6" => b"\x1b[17~".to_vec(),
        "f7" => b"\x1b[18~".to_vec(),
        "f8" => b"\x1b[19~".to_vec(),
        "f9" => b"\x1b[20~".to_vec(),
        "f10" => b"\x1b[21~".to_vec(),
        "f11" => b"\x1b[23~".to_vec(),
        "f12" => b"\x1b[24~".to_vec(),
        other => {
            // Parse modifier+key combos like "ctrl+c", "alt+x"
            if let Some(combo_bytes) = parse_key_combo(other) {
                combo_bytes
            } else {
                // 알 수 없는 key 식별자 — raw text 로 fallback (옛 동작).
                match dispatch_send(
                    core,
                    engine,
                    surface_id,
                    crate::core::intent::SendPayload::Text(other.to_string()),
                ) {
                    SendOutcome::Sent => {
                        return JsonRpcResponse::success(
                            id,
                            json!({ "sent": true, "surface_id": surface_id }),
                        );
                    }
                    outcome => {
                        return JsonRpcResponse::invalid_params(
                            id,
                            send_fail_message(surface_id, &outcome),
                        );
                    }
                }
            }
        }
    };
    // sent 여부와 무관하게 response 는 success — 옛 동작 보존.
    let _outcome = dispatch_send(
        core,
        engine,
        surface_id,
        crate::core::intent::SendPayload::Bytes(bytes),
    );
    JsonRpcResponse::success(id, json!({ "sent": true, "surface_id": surface_id }))
}

/// Force-spawn the PTY of a deferred surface without sending any input.
///
/// Returns `{ "woke": true }` if this call spawned the PTY, `{ "woke": false }`
/// otherwise (already initialized or not a deferred surface). Returns
/// `invalid_params` if the surface_id refers to neither a live terminal nor a
/// deferred placeholder.
pub(crate) fn handle_surface_wake(
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let was_deferred = engine.is_surface_deferred(surface_id);
    let woke = engine.ensure_surface_initialized(surface_id);
    if !woke && !was_deferred && engine.find_terminal_by_id(surface_id).is_none() {
        return JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id));
    }
    JsonRpcResponse::success(
        id,
        json!({ "woke": woke, "surface_id": surface_id, "pty_ready": true }),
    )
}

pub(crate) fn handle_surface_send_combo(
    core: &mut crate::core::Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let key = match params.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'key' parameter"),
    };
    let modifiers = params
        .get("modifiers")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let has_ctrl = modifiers.iter().any(|m| m == "ctrl");
    let has_alt = modifiers.iter().any(|m| m == "alt");

    let mut bytes_to_send: Vec<u8> = Vec::new();

    if has_ctrl && key.len() == 1 {
        let ch = key.chars().next().unwrap().to_ascii_lowercase();
        if ch.is_ascii_lowercase() {
            bytes_to_send.push(ch as u8 - b'a' + 1);
        } else if ch == '[' {
            bytes_to_send.push(0x1B);
        } else if ch == '\\' {
            bytes_to_send.push(0x1C);
        } else if ch == ']' {
            bytes_to_send.push(0x1D);
        }
    } else {
        if has_alt {
            bytes_to_send.push(0x1B);
        }
        bytes_to_send.extend_from_slice(key.as_bytes());
    }

    match dispatch_send(
        core,
        engine,
        surface_id,
        crate::core::intent::SendPayload::Bytes(bytes_to_send),
    ) {
        SendOutcome::Sent => JsonRpcResponse::success(id, json!({ "sent": true })),
        SendOutcome::HardOccupied => JsonRpcResponse::invalid_params(
            id,
            send_fail_message(surface_id, &SendOutcome::HardOccupied),
        ),
        SendOutcome::NotFound => {
            JsonRpcResponse::internal_error(id, "No terminal found".to_string())
        }
    }
}

// handle_pane_focus / handle_surface_focus removed: focus is user-only.

pub(crate) fn handle_surface_send_to(
    core: &mut crate::core::Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let text = match params.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'text' parameter"),
    };
    let surface_id = match params.get("surface_id").and_then(|v| v.as_u64()) {
        Some(sid) => sid as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'surface_id' parameter"),
    };
    match dispatch_send(
        core,
        engine,
        surface_id,
        crate::core::intent::SendPayload::Text(text),
    ) {
        SendOutcome::Sent => JsonRpcResponse::success(id, json!({ "sent": true })),
        outcome => JsonRpcResponse::invalid_params(id, send_fail_message(surface_id, &outcome)),
    }
}
