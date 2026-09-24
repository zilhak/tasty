use serde_json::json;

use crate::adapters::ipc::handler::params::{self, p_try};
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

/// 화면 읽기에서 요청보다 줄이 적게 반환됐을 때 보관량을 확인할 진단 정보.
pub(crate) struct ScreenDiag {
    pub scrollback_len: usize,
    pub alt_screen: bool,
}

/// 터미널이 없는 상태를 스크롤백 0과 구분해 is_terminal:false와 null로 반환한다.
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

/// 마지막 N줄을 읽는다. 하단 빈 줄은 건너뛰고 부족하면 스크롤백에서 채운다.
/// show_dim은 기본 false이며 dim 셀을 제외한다. 터미널 여부와 보관량도 반환한다.
pub(crate) fn handle_screen_text(
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

/// 터미널의 마우스 모드와 실제 클릭 처리에 적용하는 모드를 함께 반환한다.
/// hard 점유나 캡처 차단 설정 때문에 둘이 다를 수 있다.
///
/// | 필드 | 무엇을 답하나 |
/// |---|---|
/// | `terminal_mode` · `terminal_tracking` | 터미널 레지스터의 실효 레벨(DECSET 1000/1002/1003) |
/// | `sgr` | 1006(SGR 확장 좌표) 여부 |
/// | `effective_click_mode` · `effective_click_tracking` | 클릭 축에서 핸들러가 실제로 존중할 레벨 |
/// | `degraded_by` | 격하 사유들. 빈 배열이면 두 축이 일치한다 |
///
/// 개별 DECSET 플래그나 실제 PTY 전송 여부는 알리지 않는다.
/// effective_click은 휠에 적용되지 않아 캡처 차단 상태에서도 휠 보고는 가능하다.
pub(crate) fn handle_mouse_tracking(
    engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let Some(terminal) = engine.find_terminal_by_id(surface_id) else {
        return JsonRpcResponse::invalid_params(id, format!("Surface {surface_id} not found"));
    };
    JsonRpcResponse::success(
        id,
        mouse_tracking_report(
            surface_id,
            terminal.mouse_tracking(),
            terminal.sgr_mouse(),
            engine.attach.is_hard_occupied(surface_id),
            engine.is_surface_mouse_capture_disabled(surface_id),
        ),
    )
}

const NO_MOUSE_TRACKING: &str = "none";

/// 상태를 주입해 실제 모드와 적용 모드의 차이를 시험할 수 있도록 응답 조립을 분리한다.
fn mouse_tracking_report(
    surface_id: u32,
    terminal_mode: tasty_terminal::MouseTrackingMode,
    sgr: bool,
    hard_occupied: bool,
    capture_disabled: bool,
) -> serde_json::Value {
    let effective = crate::state::mouse::effective_click_tracking_decision(
        hard_occupied,
        capture_disabled,
        terminal_mode,
    );
    let mut degraded_by: Vec<&str> = Vec::new();
    if hard_occupied {
        degraded_by.push("hard_occupied");
    }
    if capture_disabled {
        degraded_by.push("mouse_capture_disabled");
    }
    let terminal_label = mouse_tracking_label(terminal_mode);
    let effective_label = mouse_tracking_label(effective);
    json!({
        "surface_id": surface_id,
        "terminal_mode": terminal_label,
        "terminal_tracking": terminal_label != NO_MOUSE_TRACKING,
        "sgr": sgr,
        "effective_click_mode": effective_label,
        "effective_click_tracking": effective_label != NO_MOUSE_TRACKING,
        "degraded_by": degraded_by,
    })
}

fn mouse_tracking_label(mode: tasty_terminal::MouseTrackingMode) -> &'static str {
    match mode {
        tasty_terminal::MouseTrackingMode::None => NO_MOUSE_TRACKING,
        tasty_terminal::MouseTrackingMode::Click => "click",
        tasty_terminal::MouseTrackingMode::CellMotion => "cell_motion",
        tasty_terminal::MouseTrackingMode::AllMotion => "all_motion",
    }
}

/// 터미널 PTY의 전경(foreground) 프로세스 이름/PID 조회.
/// 플러그인이 `claude` 같은 자식 프로세스가 살아있는지 판단하기 위해 사용한다.
/// 터미널이 없으면 `name`/`pid`가 모두 `null`로 반환된다.
pub(crate) fn handle_foreground_process(
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

/// surface ID를 유지하면서 터미널을 교체한다. 기존 Terminal은 drop으로 정리한다.
/// cwd가 있으면 새 프로세스의 작업 폴더로 사용한다.
pub(crate) fn handle_surface_respawn_terminal(
    core: &mut crate::core::Core,
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

/// surface가 속한 pane과 트리에 존재하는지를 반환한다.
pub(crate) fn handle_surface_locate(
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

    #[test]
    fn a_missing_terminal_reports_null_not_zero() {
        let out = with_screen_diagnostics(json!({ "text": "", "surface_id": 7 }), None);
        assert_eq!(out["is_terminal"], json!(false));
        assert!(
            out["scrollback_len"].is_null(),
            "0 이 아니라 null 이어야 한다"
        );
        assert!(out["alt_screen"].is_null());
        assert_eq!(out["surface_id"], json!(7));
        assert_eq!(out["text"], json!(""));
    }

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

    // 적용 제한이 있을 때와 없을 때를 함께 검사해야 항상 None을 반환하는 오류도 잡는다.
    #[test]
    fn the_two_axes_split_when_the_handler_degrades_and_agree_when_it_does_not() {
        use tasty_terminal::MouseTrackingMode as M;

        let plain = mouse_tracking_report(7, M::AllMotion, true, false, false);
        assert_eq!(plain["terminal_mode"], json!("all_motion"));
        assert_eq!(plain["effective_click_mode"], json!("all_motion"));
        assert_eq!(plain["effective_click_tracking"], json!(true));
        assert_eq!(plain["degraded_by"], json!([]), "격하가 없으면 사유도 없다");

        let blacklisted = mouse_tracking_report(7, M::AllMotion, true, false, true);
        assert_eq!(
            blacklisted["terminal_mode"],
            json!("all_motion"),
            "호스트의 입력 제한이 터미널의 트래킹 모드를 바꾸면 안 된다"
        );
        assert_eq!(
            blacklisted["effective_click_mode"],
            json!("none"),
            "호스트가 입력을 제한할 때 실제 적용 모드를 구분해 반환해야 한다"
        );
        assert_eq!(blacklisted["effective_click_tracking"], json!(false));
        assert_eq!(
            blacklisted["degraded_by"],
            json!(["mouse_capture_disabled"])
        );

        let occupied = mouse_tracking_report(7, M::CellMotion, false, true, false);
        assert_eq!(occupied["terminal_mode"], json!("cell_motion"));
        assert_eq!(occupied["effective_click_mode"], json!("none"));
        assert_eq!(
            occupied["degraded_by"],
            json!(["hard_occupied"]),
            "두 입력 제한 사유를 구분할 수 있어야 한다"
        );

        let both = mouse_tracking_report(7, M::Click, false, true, true);
        assert_eq!(
            both["degraded_by"],
            json!(["hard_occupied", "mouse_capture_disabled"])
        );

        // 원래 tracking이 꺼져 있으면 제한 사유가 있어도 두 모드는 같다.
        let off = mouse_tracking_report(7, M::None, false, false, true);
        assert_eq!(off["terminal_mode"], json!("none"));
        assert_eq!(off["effective_click_mode"], json!("none"));
        assert_eq!(off["terminal_tracking"], json!(false));
        assert_eq!(off["effective_click_tracking"], json!(false));
    }

    // 모든 모드 이름이 다르고 None만 tracking=false여야 한다.
    #[test]
    fn every_tracking_level_reports_a_name_of_its_own() {
        use tasty_terminal::MouseTrackingMode as M;
        let all = [M::None, M::Click, M::CellMotion, M::AllMotion];
        let labels: Vec<&str> = all.iter().copied().map(mouse_tracking_label).collect();

        let mut distinct = labels.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            all.len(),
            "트래킹 모드 네 종류의 표시 이름이 모두 달라야 한다: {labels:?}"
        );

        assert_eq!(mouse_tracking_label(M::None), NO_MOUSE_TRACKING);
        for mode in [M::Click, M::CellMotion, M::AllMotion] {
            assert_ne!(
                mouse_tracking_label(mode),
                NO_MOUSE_TRACKING,
                "{mode:?} 가 트래킹 꺼짐으로 보고된다"
            );
        }
    }
}
