use serde_json::json;

use crate::adapters::ipc::handler::params::{self, p_try};
use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

/// 화면 읽기 응답에 붙는 **진단 필드**. 호출자가 `lines` 로 요청한 수보다 적게 받았을 때
/// "그게 전부" 인지 "잘린 것" 인지 묻는 수단이다.
///
/// 이것이 필요한 이유: `--lines 200` 이 68 줄을 돌려줘도 그 자체로는 무엇도 알려주지
/// 않는다. 스크롤백이 비어 있으면 68 이 가진 전부이고(정상), 스크롤백이 있는데 68 이면
/// 그건 결함이다. **두 경우의 응답이 글자 그대로 같아서** 지금까지는 소스를 읽어야만
/// 갈렸다 — 에이전트가 1급 사용자인 인터페이스에서 그건 답이 아니다.
pub(crate) struct ScreenDiag {
    pub scrollback_len: usize,
    pub alt_screen: bool,
}

/// 진단 필드를 응답 객체에 붙인다.
///
/// **터미널이 없을 때 `0`/`false` 를 싣지 않는다.** 그러면 "스크롤백이 비었다" 와
/// "그런 터미널이 없다" 가 같은 값이 되어, 진단 필드가 정확히 자기 목적을 배신한다.
/// 없음은 `null` 로 내고 `is_terminal` 로 명시한다.
pub(crate) fn with_screen_diagnostics(
    mut obj: serde_json::Value,
    diag: Option<ScreenDiag>,
) -> serde_json::Value {
    let map = obj.as_object_mut().expect("응답은 객체다");
    match diag {
        Some(d) => {
            map.insert("is_terminal".into(), json!(true));
            map.insert("scrollback_len".into(), json!(d.scrollback_len));
            map.insert("alt_screen".into(), json!(d.alt_screen));
        }
        None => {
            map.insert("is_terminal".into(), json!(false));
            map.insert("scrollback_len".into(), serde_json::Value::Null);
            map.insert("alt_screen".into(), serde_json::Value::Null);
        }
    }
    obj
}

/// `surface.screen_text` — optional `lines`(**마지막 N 줄** — 하단 공백 행은 건너뛰고
/// 모자라면 스크롤백에서 채운다), `show_dim`(기본 false: dim/ghost-suggestion 셀
/// 제외 — 실제 입력된 텍스트만 반환).
///
/// 응답에는 `is_terminal` · `scrollback_len` · `alt_screen` 이 함께 실린다 —
/// 위 [`ScreenDiag`] 참조.
pub(crate) fn handle_screen_text(
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let lines = p_try!(params::opt_int::<usize>(params, "lines", &id));
    let show_dim = params
        .get("show_dim")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let found = engine.find_terminal_by_id(surface_id);
    let (text, diag) = match found {
        Some(t) => (
            match lines {
                Some(n) => t.screen_text_lines(n, show_dim),
                None => t.screen_text(show_dim),
            },
            Some(ScreenDiag {
                scrollback_len: t.scrollback_len(),
                alt_screen: t.is_alternate_screen(),
            }),
        ),
        None => (String::new(), None),
    };
    JsonRpcResponse::success(
        id,
        with_screen_diagnostics(json!({ "text": text, "surface_id": surface_id }), diag),
    )
}

pub(crate) fn handle_cursor_position(
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    if let Some(terminal) = engine.find_terminal_by_id(surface_id) {
        let (x, y) = terminal.cursor_position();
        JsonRpcResponse::success(id, json!({ "x": x, "y": y, "surface_id": surface_id }))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

/// 터미널 PTY의 전경(foreground) 프로세스 이름/PID 조회.
/// 플러그인이 `claude` 같은 자식 프로세스가 살아있는지 판단하기 위해 사용한다.
/// 터미널이 없으면 `name`/`pid`가 모두 `null`로 반환된다.
pub(crate) fn handle_foreground_process(
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let (name, pid) = engine
        .find_terminal_by_id(surface_id)
        .and_then(|t| t.foreground_process_info())
        .map(|fg| (Some(fg.name.clone()), Some(fg.pid)))
        .unwrap_or((None, None));
    JsonRpcResponse::success(
        id,
        json!({
            "surface_id": surface_id,
            "name": name,
            "pid": pid,
        }),
    )
}

/// 동일 surface_id를 유지한 채 PTY를 새 프로세스로 교체.
/// 호스트 `replace_terminal_by_id` 1:1 노출 — 기존 terminal은 drop되며 SIGHUP을
/// 보낸다. cwd가 주어지면 새 PTY의 working_dir로 지정.
///
/// 플러그인이 claude.respawn에서 사용. 그 핸들러는 본 IPC로 PTY를 갈아끼운 뒤
/// surface.send로 `claude` 명령을 재송신한다.
pub(crate) fn handle_surface_respawn_terminal(
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
    let cwd = params
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from);
    // 2 차 방어: 호스트가 absolute + valid 만 받는다는 contract 검증.
    if let Some(p) = &cwd
        && !p.is_dir()
    {
        return JsonRpcResponse::invalid_params(id, format!("cwd does not exist: {}", p.display()));
    }

    let intent = crate::core::intent::DomainIntent::RespawnTerminal { surface_id, cwd };
    let events = match core.apply(engine, intent) {
        Ok(e) => e,
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    };
    let Some(crate::core::intent::CoreEvent::TerminalRespawned { surface_id, error }) =
        events.into_iter().next()
    else {
        return JsonRpcResponse::internal_error(
            id,
            "Core::apply returned no TerminalRespawned event",
        );
    };
    match error {
        None => JsonRpcResponse::success(id, json!({ "ok": true, "surface_id": surface_id })),
        Some(e) => JsonRpcResponse::invalid_params(id, e),
    }
}

/// surface_id를 포함하는 pane을 찾아 `pane_id`와 존재 여부를 반환.
/// 플러그인이 자기 자식 surface를 죽이거나 wait할 때, 호스트 트리에 여전히
/// 살아있는지 확인하기 위해 사용한다.
pub(crate) fn handle_surface_locate(
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let pane_id = engine.find_pane_for_surface(surface_id);
    JsonRpcResponse::success(
        id,
        json!({
            "surface_id": surface_id,
            "pane_id": pane_id,
            "exists": pane_id.is_some(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 진단 필드의 값은 **없음과 0 을 갈라야** 쓸모가 있다. 터미널이 없을 때 `0` 을
    /// 실으면 "스크롤백이 비었다" 와 구별되지 않고, 그건 이 필드를 넣은 이유 자체를
    /// 없앤다.
    #[test]
    fn a_missing_terminal_reports_null_not_zero() {
        let out = with_screen_diagnostics(json!({ "text": "", "surface_id": 7 }), None);
        assert_eq!(out["is_terminal"], json!(false));
        assert!(
            out["scrollback_len"].is_null(),
            "0 이 아니라 null 이어야 한다"
        );
        assert!(out["alt_screen"].is_null());
        // 기존 필드는 그대로 남는다(추가만 한다).
        assert_eq!(out["surface_id"], json!(7));
        assert_eq!(out["text"], json!(""));
    }

    /// 스크롤백이 **실제로 비어 있는** 터미널은 `0` 이다 — 위의 `null` 과 다른 상태다.
    /// 이 둘이 갈리는 것이 이 필드의 전부다.
    #[test]
    fn an_empty_scrollback_reports_zero_which_is_not_null() {
        let out = with_screen_diagnostics(
            json!({ "text": "hi", "surface_id": 7 }),
            Some(ScreenDiag {
                scrollback_len: 0,
                alt_screen: true,
            }),
        );
        assert_eq!(out["is_terminal"], json!(true));
        assert_eq!(out["scrollback_len"], json!(0));
        assert!(!out["scrollback_len"].is_null());
        assert_eq!(out["alt_screen"], json!(true));
    }

    #[test]
    fn a_live_scrollback_is_reported_as_its_length() {
        let out = with_screen_diagnostics(
            json!({ "id": 3, "text": "x" }),
            Some(ScreenDiag {
                scrollback_len: 402,
                alt_screen: false,
            }),
        );
        assert_eq!(out["scrollback_len"], json!(402));
        assert_eq!(out["alt_screen"], json!(false));
        assert_eq!(out["id"], json!(3), "pty.read 의 키도 보존한다");
    }
}
