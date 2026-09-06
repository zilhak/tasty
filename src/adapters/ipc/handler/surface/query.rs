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

/// 터미널이 지금 **마우스를 잡고 있는가**, 잡고 있다면 어느 레벨인가 — 그리고
/// **마우스 핸들러가 그것을 존중하는가.**
///
/// **축이 둘이라 값도 둘이다.** 터미널의 레지스터가 무엇인지와, 클릭·드래그·hover 가
/// 실제로 앱에 갈지는 **다른 물음**이다. 사이에 격하가 하나 있다 — hard 점유(readonly)
/// 이거나 전경 프로세스가 마우스 캡처 블랙리스트에 걸리면 핸들러는 실제 모드와 무관하게
/// `None` 으로 취급한다([`crate::state::mouse::effective_click_tracking_decision`]).
///
/// 그래서 한 값으로 뭉개지 않는다. 뭉개면 **낱말 하나가 두 축을 덮고**, 그 둘이 어긋나는
/// 기계에서 관측면이 거꾸로 읽힌다 — "터미널이 all_motion 인데 보고가 0 이다" 를 제품
/// 결함으로 읽게 되지만 사실은 격하가 정상 동작한 것이다. **둘이 어긋나는 것 자체가
/// 신호**라 둘 다 낸다.
///
/// | 필드 | 무엇을 답하나 |
/// |---|---|
/// | `terminal_mode` · `terminal_tracking` | 터미널 레지스터의 실효 레벨(DECSET 1000/1002/1003) |
/// | `sgr` | 1006(SGR 확장 좌표) 여부 |
/// | `effective_click_mode` · `effective_click_tracking` | 클릭 축에서 핸들러가 실제로 존중할 레벨 |
/// | `degraded_by` | 격하 사유들. 빈 배열이면 두 축이 일치한다 |
///
/// **이 값이 답하지 못하는 것**(R582):
/// - **개별 레지스터**(1000/1002/1003 각각)는 안 낸다. `terminal_mode` 는 실효 레벨이라
///   "1003 을 끄지 않은 채 1002 를 켰다" 는 이 값으로 구분되지 않는다 — 그 구분은
///   `tasty-terminal` 의 단위 시험이 갖고 있다.
/// - **휠은 이 격하의 대상이 아니다.** `effective_click_*` 는 이름 그대로 **클릭 축**이고,
///   캡처 블랙리스트에 걸린 surface 도 휠은 계속 앱에 보고한다. 즉
///   `effective_click_tracking == false` 여도 휠 보고는 살아 있을 수 있다.
/// - **보고가 실제로 PTY 로 나갔는지**는 안 낸다. 이 값은 결정의 입력이지 결과가 아니다.
pub(crate) fn handle_mouse_tracking(
    _state: &AppState,
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

/// `mode` 가 이 값일 때만 `tracking` 이 거짓이다. 두 필드가 같은 곳에서 나오도록
/// 상수를 하나 둔다 — 문자열을 양쪽에 손으로 적으면 한쪽만 고쳐도 컴파일된다.
const NO_MOUSE_TRACKING: &str = "none";

/// 응답 본문을 **순수 함수로** 만든다. 핸들러는 `CoreState` 를 요구해서 시험이 못 부르고,
/// 그러면 두 축이 실제로 갈리는지가 무대조로 남는다 — 이 채널이 존재하는 이유가 바로
/// 그 갈림이라 그건 답이 아니다.
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

/// 실효 레벨을 응답 문자열로. **순수 함수라 시험이 직접 부른다.**
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

    /// ★★ **두 축이 실제로 갈린다.** 이 채널의 존재 이유가 그 갈림인데, 오늘 이 기계에서는
    /// 두 블랙리스트가 비어 있어 두 값이 **우연히 일치**한다 — 그러면 한 축만 내는 구현도
    /// 모든 관측을 통과한다(실제로 첫 판이 그랬다). 그래서 격하 입력을 참으로 만든 자리에서
    /// 두 값이 갈리는지를 시험이 직접 만든다.
    ///
    /// 양방향인 이유: 갈리는 쪽만 보면 "언제나 none 으로 격하한다" 는 구현이 통과하고,
    /// 일치하는 쪽만 보면 격하를 아예 안 하는 구현이 통과한다. 둘 다 이 관측면을 무용하게
    /// 만든다.
    #[test]
    fn the_two_axes_split_when_the_handler_degrades_and_agree_when_it_does_not() {
        use tasty_terminal::MouseTrackingMode as M;

        // ① 격하 없음 — 두 축이 같고, 사유 목록이 비어 있다.
        let plain = mouse_tracking_report(7, M::AllMotion, true, false, false);
        assert_eq!(plain["terminal_mode"], json!("all_motion"));
        assert_eq!(plain["effective_click_mode"], json!("all_motion"));
        assert_eq!(plain["effective_click_tracking"], json!(true));
        assert_eq!(plain["degraded_by"], json!([]), "격하가 없으면 사유도 없다");

        // ② 캡처 블랙리스트 — 터미널은 그대로인데 핸들러가 안 존중한다.
        let blacklisted = mouse_tracking_report(7, M::AllMotion, true, false, true);
        assert_eq!(
            blacklisted["terminal_mode"],
            json!("all_motion"),
            "격하는 터미널 레지스터를 바꾸지 않는다 — 바꾸면 두 축을 낸 뜻이 없다"
        );
        assert_eq!(
            blacklisted["effective_click_mode"],
            json!("none"),
            "핸들러 축이 안 갈렸다 — 이 관측면은 격하를 못 보고, 그러면 보고 0 이 \
             제품 결함으로 거꾸로 읽힌다"
        );
        assert_eq!(blacklisted["effective_click_tracking"], json!(false));
        assert_eq!(
            blacklisted["degraded_by"],
            json!(["mouse_capture_disabled"])
        );

        // ③ hard 점유 — 다른 입력도 같은 갈림을 만들고, 사유가 그것을 구분한다.
        let occupied = mouse_tracking_report(7, M::CellMotion, false, true, false);
        assert_eq!(occupied["terminal_mode"], json!("cell_motion"));
        assert_eq!(occupied["effective_click_mode"], json!("none"));
        assert_eq!(
            occupied["degraded_by"],
            json!(["hard_occupied"]),
            "사유가 뭉개지면 두 격하가 같은 얼굴이 된다 — 처방이 다른데"
        );

        // ④ 둘 다 — 사유가 둘 다 실린다(or 로 뭉개지 않는다).
        let both = mouse_tracking_report(7, M::Click, false, true, true);
        assert_eq!(
            both["degraded_by"],
            json!(["hard_occupied", "mouse_capture_disabled"])
        );

        // ⑤ ★ 반대 방향 — 트래킹이 꺼져 있으면 격하가 참이어도 **사유만 남고** 두 축은
        //    여전히 같다. 이것이 없으면 "격하 입력이 참이면 무조건 갈린다" 는 잘못된
        //    읽기가 남는다.
        let off = mouse_tracking_report(7, M::None, false, false, true);
        assert_eq!(off["terminal_mode"], json!("none"));
        assert_eq!(off["effective_click_mode"], json!("none"));
        assert_eq!(off["terminal_tracking"], json!(false));
        assert_eq!(off["effective_click_tracking"], json!(false));
    }

    /// 네 레벨이 **서로 다른 이름**을 갖고, `tracking` 이 정확히 `None` 에서만 거짓이다.
    ///
    /// 양방향인 이유: 사상이 전부 같은 문자열을 돌려줘도(예: 실수로 `_ => "none"`)
    /// "None 이 none 이다" 만 보는 단정은 통과한다. 그러면 이 관측면은 늘 "트래킹 꺼짐"
    /// 을 보고하게 되고, **그것이 이 메서드가 존재하는 이유 자체를 배신한다** —
    /// 이 값은 "트래킹이 안 켜졌다" 와 "켜졌는데 보고가 없다" 를 가르려고 만든 것이다.
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
            "레벨 넷이 이름 {labels:?} 로 뭉갰다 — 뭉개진 레벨은 이 관측면에서 사라진다"
        );

        assert_eq!(mouse_tracking_label(M::None), NO_MOUSE_TRACKING);
        // ★ 반대 방향 — 나머지 셋은 하나도 그 값이 아니어야 한다. 이것이 없으면
        //   `tracking` 이 항상 거짓인 구현도 위 단정을 통과한다.
        for mode in [M::Click, M::CellMotion, M::AllMotion] {
            assert_ne!(
                mouse_tracking_label(mode),
                NO_MOUSE_TRACKING,
                "{mode:?} 가 트래킹 꺼짐으로 보고된다"
            );
        }
    }
}
