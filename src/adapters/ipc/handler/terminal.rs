//! `terminal.*` IPC 핸들러 — 호스트 내재화된 child-terminal 관리 (ADR-0040 / occupancy-04).
//!
//! 에이전트가 자식 터미널 surface 를 spawn/tell/wait/kill 하는 **범용 기계**. 지금까지
//! codex/claude 플러그인에 중복 구현돼 있던 부분을 호스트 1급으로 끌어올린다. 에이전트
//! 특화(codex/claude 바이너리 command 빌더, hook/trust, telemetry)는 플러그인에 잔류
//! (05). 여기서는 호출자가 넘긴 **임의 command 문자열**을 그대로 터미널에 붙인다.
//!
//! 구현은 sibling 핸들러(`tab.create` / `surface.send` / `surface.close` /
//! `surface.locate` / `surface.respawn_terminal`)를 **in-process 재사용** 한다 — 플러그인이
//! `host.call(...)` 로 조합하던 것과 byte-for-byte 동형이되 IPC 왕복이 없다.
//!
//! **soft 점유 (03 소비)**: spawn 성공 시 child 를 `occupy_soft(child, parent)` 로 등록,
//! kill 시 `release_occupancy(child)` 로 해제 — 둘 다 in-process core 함수 호출이다
//! (`occupancy.*` IPC method 는 만들지 않는다, 03 경계).

use serde_json::{Value, json};

use crate::core::child_terminal::ChildEntry;
use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

use super::{surface, tab};

type Core = crate::core::Core;
type CoreState = crate::core::CoreState;

// ───── 파라미터 헬퍼 ─────

fn require_u32(params: &Value, key: &str, id: &Value) -> Result<u32, JsonRpcResponse> {
    params
        .get(key)
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), format!("missing '{key}'")))
}

fn optional_u32(params: &Value, key: &str) -> Option<u32> {
    params.get(key).and_then(|v| v.as_u64()).map(|v| v as u32)
}

fn optional_str(params: &Value, key: &str) -> Option<String> {
    params.get(key).and_then(|v| v.as_str()).map(String::from)
}

fn require_str(params: &Value, key: &str, id: &Value) -> Result<String, JsonRpcResponse> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), format!("missing '{key}'")))
}

/// `--surface`(parent) 명시가 없으면 유일 parent 로 폴백. 0 또는 2+ 면 에러(호출자 명시 요구).
fn resolve_parent(engine: &CoreState, params: &Value, id: &Value) -> Result<u32, JsonRpcResponse> {
    if let Some(p) = optional_u32(params, "surface") {
        return Ok(p);
    }
    engine.child_terminals.single_parent().ok_or_else(|| {
        JsonRpcResponse::invalid_params(
            id.clone(),
            "missing 'surface' parameter (0 or >1 parents — specify --surface)".to_string(),
        )
    })
}

/// in-process sibling 핸들러 응답을 `Result<Value, JsonRpcResponse>` 로 변환.
/// error 응답은 caller 의 id 를 유지한 채 그대로 전파한다.
fn unwrap_ok(resp: JsonRpcResponse, id: &Value) -> Result<Value, JsonRpcResponse> {
    match (resp.result, resp.error) {
        (Some(r), _) => Ok(r),
        (_, Some(e)) => Err(JsonRpcResponse::error(id.clone(), e.code, e.message)),
        _ => Err(JsonRpcResponse::internal_error(
            id.clone(),
            "sibling handler returned empty response".to_string(),
        )),
    }
}

/// tell/broadcast/spawn 이 PTY 로 보낼 본문. 멀티라인은 bracketed paste 로 감싼다
/// (개행 그대로 한 덩어리 paste). 제출 `\r` 은 포함하지 않는다 — 호출자가 별도
/// write 로 보낸다(길이 무관 결정적 제출). 플러그인 `build_tell_payload` 와 동형.
fn build_tell_payload(message: &str) -> String {
    if message.contains('\n') {
        format!("\u{1b}[200~{message}\u{1b}[201~")
    } else {
        message.to_string()
    }
}

/// broadcast 대상에 보낼 `(본문 payload, 제출 여부)`. 호출자가 넣은 trailing `\r`(제출
/// 의도)만 본문에서 분리한다 — 본문은 `build_tell_payload` 로 감싸 한 write, 제출 `\r` 은
/// 또 다른 write 로 보내 64-codepoint paste burst 를 피하고 길이 무관 결정적 제출을 보장한다
/// (tell 과 동형). trailing `\r` 이 없으면 제출 write 를 생략해 "sent as-is" 주입 계약을 보존.
fn build_broadcast_payload(text: &str) -> (String, bool) {
    let submit = text.ends_with('\r');
    let body = text.trim_end_matches('\r');
    (build_tell_payload(body), submit)
}

/// 본문 ack 대기 + 제출 `\r` 재주입에 쓰는 타임아웃.
///
/// 두 write(본문/제출 `\r`)가 지연 없이 연달아 PTY 에 들어가면 child TUI(Ink 기반
/// Claude Code/Codex CLI)의 다음 `read()` 가 둘을 한 번에 묶어 받을 수 있다 — 이
/// 경우 63자+ 단일라인처럼 paste 휴리스틱을 트리거하는 입력은 그 안에 섞인 `\r`
/// 을 제출이 아닌 paste 본문의 일부로 처리해 제출이 유실된다(실측: in-process
/// 연속 write 시 596자 메시지 8/8 재현 — 입력창에 텍스트만 남고 제출 안 됨).
///
/// 최초 수정은 고정 20ms sleep 이었으나 Gate 4 리뷰에서 두 가지가 지적됨: (a)
/// `handle_tell`/`handle_spawn` 은 winit 메인 스레드(`about_to_wait` → `process_ipc`)
/// 에서 동기 실행되므로 메인 스레드 sleep 은 그 시간만큼 렌더/입력 처리를 통째로
/// 멈춘다, (b) 20ms 는 "대충 다 썼겠지" 라는 타이밍 가정이라 writer 스레드가
/// 밀리면 깨질 수 있다. 재설계: 본문을 [`tasty_terminal::WriteAck`] 로 write 해
/// writer 스레드가 실제로 flush 완료했다는 사실 자체를 ack 로 받고, 그 ack 대기는
/// 새로 스폰한 스레드(메인 스레드 아님)에서 수행 — 완료되면 그 스레드가
/// `HostIpcInjector` 로 host 자신에게 `surface.send("\r")` 를 재주입한다. 메인
/// 스레드는 다음 `about_to_wait` 틱에서 이 재주입된 커맨드를 평소처럼(non-blocking)
/// 드레인한다. 이 타임아웃은 ack 가 끝내 오지 않을 때(writer 스레드 이상 등)의
/// 안전판 — 그 시점엔 최선 노력으로 `\r` 을 진행한다(완전 무제출보다 낫다).
const TELL_SUBMIT_ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// ack 완료 이후, 제출 `\r` 재주입 전에 추가로 두는 확정적 지연.
///
/// [`WriteAck`](tasty_terminal::WriteAck) 가 보장하는 건 writer 스레드의
/// `write_all`+`flush` 가 에러 없이 리턴했다는 사실뿐이다 — 즉 본문 바이트가
/// 커널 PTY 버퍼에 들어갔다는 것이지, child TUI 가 그 바이트를 이미 `read()` 로
/// 소비했다는 보장이 아니다. ack 이후 곧바로 `\r` 을 재주입하면, child 의 다음
/// `read()` 가 여전히 본문과 `\r` 을 한 번에 묶어 받을 가능성이 남는다(이 경우
/// paste 휴리스틱이 `\r` 을 제출이 아닌 본문 일부로 삼켜버린다 — 원래 버그와 동일
/// 증상). 실전에서는 ack 대기 → 스레드 깨어남 → `HostIpcInjector::dispatch` →
/// 메인 스레드의 다음 `about_to_wait` 틱, 이 여러 홉이 쌓는 자연 지연 덕에 대체로
/// 회피되지만, 이건 스레드 스케줄링 우연에 기대는 것이라 재주입 경로가 더
/// 빨라지도록 최적화되면 안전마진이 줄어들 수 있다. 20ms 는 1라운드 실측에서
/// (5ms/10ms 는 간헐 실패, 20ms 부터 일관 통과) 얻은 값을 그대로 재사용한 확정적
/// 하한 — 스레드 홉의 우연한 지연에만 기대지 않도록 명시적으로 보장한다. 반드시
/// 새로 스폰한 스레드 안에서만 sleep 한다 — 메인 스레드에서 쓰면 1라운드에서
/// 지적된 렌더/IPC 정지 문제가 재발한다.
const TELL_SUBMIT_EXTRA_SETTLE_DELAY: std::time::Duration = std::time::Duration::from_millis(20);

/// [`send_body_then_submit`] 1단계 — `apply_send_to_surface`(core/mod.rs)와 동일한
/// attach 점유 체크 + surface 초기화를 거쳐 ack 가능한 방식으로 본문을 큐잉한다.
/// `None` 은 surface 미존재 또는 hard-occupied(`handle_surface_send` 와 동일하게
/// 구분하지 않는다).
fn send_text_to_surface_with_ack(
    engine: &mut CoreState,
    surface_id: u32,
    text: &str,
) -> Option<tasty_terminal::WriteAck> {
    if engine.attach.is_hard_occupied(surface_id) {
        return None;
    }
    engine.ensure_surface_initialized(surface_id);
    engine
        .find_terminal_by_id_mut(surface_id)
        .map(|terminal| terminal.send_key_with_ack(text))
}

/// 본문을 ack 가능한 방식으로 write 하고, 실제 PTY flush 확인(ack) 후에만 제출
/// `\r` 을 별도 스레드에서 host IPC 로 재주입한다. `handle_tell` / `handle_spawn`
/// 의 command 주입이 동형이라 공용으로 뺐다.
///
/// **메인 스레드는 절대 블로킹하지 않는다** — ack 대기(`WriteAck::wait`)와 `\r`
/// 재주입(`HostIpcInjector::dispatch`)은 전부 새로 스폰한 스레드에서 수행된다.
/// 그 스레드가 큐에 넣는 `surface.send` 커맨드는 메인 스레드가 다음
/// `about_to_wait` 틱에서 평소처럼 non-blocking 하게 드레인해 처리하므로, 제출
/// `\r` 도 기존 `handle_surface_send` 의 attach 점유 검증 등을 동일하게 통과한다.
fn send_body_then_submit(
    engine: &mut CoreState,
    core: &Core,
    id: &Value,
    surface_id: u32,
    body: String,
) -> Result<(), JsonRpcResponse> {
    let Some(ack) = send_text_to_surface_with_ack(engine, surface_id, &body) else {
        return Err(JsonRpcResponse::invalid_params(
            id.clone(),
            format!("Surface {surface_id} not found"),
        ));
    };

    let injector = core.host_ipc_injector_arc().get().cloned();
    std::thread::spawn(move || {
        ack.wait(TELL_SUBMIT_ACK_TIMEOUT);
        std::thread::sleep(TELL_SUBMIT_EXTRA_SETTLE_DELAY);
        let Some(injector) = injector else {
            tracing::warn!(
                "terminal tell/spawn: host_ipc_injector unavailable — \\r submit for surface {surface_id} dropped"
            );
            return;
        };
        let params = json!({ "surface_id": surface_id, "text": "\r" });
        if let Err(e) = injector.dispatch("surface.send", params, TELL_SUBMIT_ACK_TIMEOUT) {
            tracing::warn!(
                "terminal tell/spawn: submit \\r re-injection failed (surface={surface_id}): {e}"
            );
        }
    });
    Ok(())
}

/// workspace 를 ID 또는 이름으로 해석. 숫자면 ID 매칭 우선, 아니면 name 매칭.
fn resolve_workspace_id(engine: &CoreState, target: &str) -> Option<u32> {
    if let Ok(target_id) = target.parse::<u32>()
        && engine.workspaces.iter().any(|w| w.id == target_id)
    {
        return Some(target_id);
    }
    engine
        .workspaces
        .iter()
        .find(|w| w.name == target)
        .map(|w| w.id)
}

/// workspace 내 첫 pane 의 id. spawn 의 `--pane` 미지정 기본 대상.
fn first_pane_in_workspace(engine: &CoreState, ws_id: u32) -> Option<u32> {
    let idx = engine.find_workspace_index_for_id(ws_id)?;
    engine.workspaces[idx]
        .pane_layout()
        .all_pane_ids()
        .into_iter()
        .next()
}

// ───── 핸들러 ─────

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_spawn(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    engine.reconcile_child_terminals();

    let parent = match require_u32(params, "parent", &id) {
        Ok(p) => p,
        // `surface` alias 도 허용 — CLI 는 parent 를 caller surface 로 채운다.
        Err(_) => match require_u32(params, "surface", &id) {
            Ok(p) => p,
            Err(e) => return e,
        },
    };
    let workspace_param = match require_str(params, "workspace", &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    // command 는 optional. 지정되면 tab 생성 직후 그대로 붙여 제출한다. 생략되면
    // 아무것도 보내지 않고 tab 생성·registry 등록·soft 점유·surface_id 반환만 한다
    // — codex/claude plugin(05)이 이 2단계 spawn 을 소비한다: 먼저 command 없이
    // 호출해 host registry 에 자식을 등록하고 child_surface_id 를 받은 뒤, 그
    // surface_id 를 박은 에이전트 특화 command(TASTY_SURFACE_ID=... / session token
    // 등)를 `surface.send` 로 별도 전송한다.
    let command = optional_str(params, "command");
    let pane_override = optional_u32(params, "pane");
    let cwd = optional_str(params, "cwd");
    let role = optional_str(params, "role");
    let nickname = optional_str(params, "nickname");

    let Some(ws_id) = resolve_workspace_id(engine, &workspace_param) else {
        return JsonRpcResponse::invalid_params(
            id,
            format!("workspace '{workspace_param}' not found"),
        );
    };
    let pane_id = match pane_override {
        Some(p) => p,
        None => match first_pane_in_workspace(engine, ws_id) {
            Some(p) => p,
            None => {
                return JsonRpcResponse::invalid_params(
                    id,
                    format!("No panes in workspace {ws_id}"),
                );
            }
        },
    };

    // child index 먼저 확보해 탭 이름을 child{N} 으로 고정.
    let index = engine.child_terminals.next_index_for(parent);
    let tab_name = format!("child{index}");
    let mut tab_params = json!({
        "pane_id": pane_id,
        "type": "terminal",
        "name": tab_name,
    });
    if let Some(c) = &cwd {
        tab_params["cwd"] = Value::String(c.clone());
    }
    let tab_resp = tab::handle_tab_create(core, state, engine, id.clone(), &tab_params);
    let tab_val = match unwrap_ok(tab_resp, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(new_surface_id) = tab_val
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
    else {
        return JsonRpcResponse::internal_error(
            id,
            format!("tab.create response missing 'surface_id': {tab_val}"),
        );
    };

    // command 가 주어졌을 때만 붙여 제출한다(에이전트 특화 아님). 본문/제출 `\r` 을
    // 분리해 길이 무관 결정적 제출(멀티라인은 bracketed paste) — tell 과 동형(ack
    // 기반 재주입 포함, TELL_SUBMIT_ACK_TIMEOUT 참조). command 생략 시(plugin
    // 2단계 spawn) 전송을 건너뛰고 등록·점유만 수행한다.
    if let Some(command) = &command {
        let body = build_tell_payload(command);
        if let Err(e) = send_body_then_submit(engine, core, &id, new_surface_id, body) {
            return e;
        }
    }

    engine.child_terminals.register_child(
        parent,
        ChildEntry {
            child_surface_id: new_surface_id,
            index,
            cwd,
            role: role.clone(),
            nickname: nickname.clone(),
        },
    );
    engine.child_terminals.save();

    // soft 점유 등록(03 소비): 주체 = spawn 을 발동한 parent surface. 라벨은
    // nickname > role. in-process 호출(occupancy.* IPC 아님).
    let label = nickname.or(role);
    if let Err(e) = engine.occupy_soft(new_surface_id, parent, label) {
        tracing::warn!(
            "terminal.spawn occupy_soft(child={new_surface_id}, parent={parent}) failed: {e:?}"
        );
    }

    JsonRpcResponse::success(
        id,
        json!({
            "child_surface_id": new_surface_id,
            "child_index": index,
            "pane_id": pane_id,
            "workspace_id": ws_id,
        }),
    )
}

pub(crate) fn handle_tell(
    core: &mut Core,
    _state: &mut AppState,
    engine: &mut CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let surface_id = match require_u32(params, "surface", &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let text = match require_str(params, "text", &id) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let payload = build_tell_payload(&text);
    // 1) 본문(제출 `\r` 미포함) 을 ack 가능한 방식으로 write. 2) 실제 flush 확인
    // (ack) 후 별도 스레드에서 제출 Enter 를 재주입 — 길이 무관 결정적 제출
    // (근거는 TELL_SUBMIT_ACK_TIMEOUT 참조).
    if let Err(e) = send_body_then_submit(engine, core, &id, surface_id, payload) {
        return e;
    }
    JsonRpcResponse::success(id, json!({ "sent": true, "surface_id": surface_id }))
}

pub(crate) fn handle_children(
    engine: &mut CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    // 접근 시점 self-heal — 죽은 자식 surface 를 목록에서 즉시 정리(이벤트 구독 대체).
    engine.reconcile_child_terminals();
    let parent = match resolve_parent(engine, params, &id) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let children: Vec<Value> = engine
        .child_terminals
        .list_children(parent)
        .iter()
        .map(|c| {
            json!({
                "index": c.index,
                "surface_id": c.child_surface_id,
                "role": c.role,
                "nickname": c.nickname,
                "state": engine.child_terminals.state_of(c.child_surface_id),
                "cwd": c.cwd,
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!({ "children": children }))
}

/// 자식 단건 상태 조회 (TODO80 §E — 실증 소비자). `handle_children`/`handle_parent`
/// 와 동형으로 대상 child surface 를 `surface` 파라미터로 직접 지정한다(포커스
/// 독립 — CLAUDE.md 원칙 3).
///
/// **결정 4**: `ChildTerminalRegistry::state_of` 자신은 미등록 surface 에
/// `"active"` fallback 계약을 그대로 유지한다(`src/core/child_terminal.rs:196-209`
/// 와 그 테스트는 불변). 이 getter 는 그 위에서 라이브 surface 트리와 별도로
/// 대조해, 실제로 죽은 surface 만 `"exited"` 로 구분한다 — registry 자체의
/// self-heal(`reconcile_child_terminals`)이 이미 죽은 항목을 지웠더라도(관계가
/// 사라져도) 그 surface 가 라이브인지는 독립적으로 판정해야 하므로 reconcile
/// 결과에 기대지 않고 `live_surface_ids()` 를 직접 대조한다.
pub(crate) fn handle_state(engine: &mut CoreState, id: Value, params: &Value) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let surface_id = match require_u32(params, "surface", &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let state = if engine.live_surface_ids().contains(&surface_id) {
        engine.child_terminals.state_of(surface_id)
    } else {
        "exited"
    };
    JsonRpcResponse::success(id, json!({ "state": state, "surface_id": surface_id }))
}

pub(crate) fn handle_parent(engine: &mut CoreState, id: Value, params: &Value) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let child_surface = match require_u32(params, "surface", &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match engine.child_terminals.parent_of_child(child_surface) {
        Some(parent_id) => JsonRpcResponse::success(
            id,
            json!({ "parent_surface_id": parent_id, "status": "active" }),
        ),
        None => JsonRpcResponse::success(
            id,
            json!({ "parent_surface_id": Value::Null, "status": "none" }),
        ),
    }
}

pub(crate) fn handle_kill(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let parent = match resolve_parent(engine, params, &id) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let child_index = match require_u32(params, "child", &id) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(removed) = engine.child_terminals.remove_child(parent, child_index) else {
        return JsonRpcResponse::invalid_params(
            id,
            format!("child {child_index} not found under surface {parent}"),
        );
    };
    engine.child_terminals.save();

    // soft 점유 해제(03 소비): surface.close 이전에 명시적 release. tier 무관 강제
    // 해제(soft 이면 주체 검증 없이 clear). in-process 호출.
    engine.release_occupancy(removed.child_surface_id);

    let close_params = json!({ "surface_id": removed.child_surface_id });
    if let Err(e) = unwrap_ok(
        surface::handle_surface_close(core, state, engine, id.clone(), &close_params),
        &id,
    ) {
        return e;
    }
    JsonRpcResponse::success(
        id,
        json!({
            "killed_surface_id": removed.child_surface_id,
            "child_index": removed.index,
        }),
    )
}

/// child 를 닫지 않고 관계·soft 점유만 해제한다 — `docs/features/child-terminal/index.md`
/// ("release" 절). `handle_kill`(421-463행)과 동일한 `remove_child`+`save` 순서를 따르되, 점유
/// 해제에 tier-무관 `release_occupancy` 대신 주체 검증판 `release_soft_occupancy` 를
/// 쓰고(hard 점유는 구조적으로 손대지 않음), `surface.close` 호출을 생략한다 — 이 생략이
/// kill 과의 유일한 동작 차이다. `release_soft_occupancy` 가 desync(이미 점유만 풀린
/// 경우) 로 실패해도 registry 정리(관계 해제)는 그대로 성공 처리한다.
pub(crate) fn handle_release(engine: &mut CoreState, id: Value, params: &Value) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let parent = match resolve_parent(engine, params, &id) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let child_index = match require_u32(params, "child", &id) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(removed) = engine.child_terminals.remove_child(parent, child_index) else {
        return JsonRpcResponse::invalid_params(
            id,
            format!("child {child_index} not found under surface {parent}"),
        );
    };
    engine.child_terminals.save();

    if let Err(e) = engine.release_soft_occupancy(removed.child_surface_id, parent) {
        tracing::warn!(
            "terminal.release: soft occupancy release failed for surface {} \
             (parent {parent}): {e:?} — registry relationship removed anyway",
            removed.child_surface_id
        );
    }

    JsonRpcResponse::success(
        id,
        json!({
            "released_surface_id": removed.child_surface_id,
            "child_index": removed.index,
        }),
    )
}

/// 임의의 기존 surface 를 명시적으로 `parent` 의 child 로 등록(soft 점유) —
/// `docs/features/child-terminal/index.md`("adopt" 절). `handle_spawn`(317-336행)의
/// 관계등록+점유 블록과 동일 시퀀스를 "PTY 생성 없이, 호출자가 지정한 기존
/// surface_id" 에 대해 수행한다.
///
/// 연산 순서는 `handle_spawn` 과 **반대**다 — spawn 의 대상(방금 생성된 surface)은
/// 실전에서 거의 절대 이미 점유돼 있을 수 없어 `register_child` → `occupy_soft`
/// 순서에 `occupy_soft` 실패를 관용해도 무해하지만, adopt 의 대상은 임의의 기존
/// surface 라 이미 점유돼 있을 확률이 훨씬 높다. 순서를 뒤집어 `occupy_soft` 를
/// 먼저 시도하고, 실패하면 registry 를 전혀 건드리지 않은 채 즉시 에러 반환한다
/// ("children 목록 = 점유 목록" 동치성 보존).
pub(crate) fn handle_adopt(engine: &mut CoreState, id: Value, params: &Value) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let parent = match resolve_parent(engine, params, &id) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let target = match require_u32(params, "target", &id) {
        Ok(t) => t,
        Err(e) => return e,
    };

    if engine.find_surface_by_id(target).is_none() {
        return JsonRpcResponse::invalid_params(id, format!("surface {target} not found"));
    }
    if parent == target {
        return JsonRpcResponse::invalid_params(
            id,
            "cannot adopt a surface as its own child".to_string(),
        );
    }
    if let Some(existing_parent) = engine.child_terminals.parent_of_child(target) {
        return JsonRpcResponse::invalid_params(
            id,
            format!(
                "surface {target} is already a child of {existing_parent} \
                 (release it first — see 'tasty terminal release')"
            ),
        );
    }
    // hard 점유(원격 attach) 대상은 거부 — `occupy_soft` 자체는 hard lock 을 검사하지
    // 않으므로 여기서 명시적으로 막는다(hard 점유는 이 TODO 스코프 밖).
    if engine.attach.is_hard_occupied(target) {
        return JsonRpcResponse::invalid_params(
            id,
            format!("surface {target} is hard-occupied (remote attach) — cannot adopt"),
        );
    }

    let role = optional_str(params, "role");
    let nickname = optional_str(params, "nickname");
    let cwd = optional_str(params, "cwd");
    let label = nickname.clone().or_else(|| role.clone());
    // occupy_soft 를 먼저 시도 — 실패하면(다른 parent 가 이미 soft 점유 중)
    // registry 는 손대지 않고 바로 에러 반환.
    if let Err(e) = engine.occupy_soft(target, parent, label) {
        return JsonRpcResponse::error(id, -32020, format!("occupy_soft failed: {e:?}"));
    }

    let index = engine.child_terminals.next_index_for(parent);
    engine.child_terminals.register_child(
        parent,
        ChildEntry {
            child_surface_id: target,
            index,
            cwd,
            role,
            nickname,
        },
    );
    engine.child_terminals.save();

    JsonRpcResponse::success(
        id,
        json!({
            "child_surface_id": target,
            "child_index": index,
        }),
    )
}

pub(crate) fn handle_respawn(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let parent = match resolve_parent(engine, params, &id) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let child_index = match require_u32(params, "child", &id) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(entry) = engine
        .child_terminals
        .find_child(parent, child_index)
        .cloned()
    else {
        return JsonRpcResponse::invalid_params(
            id,
            format!("child {child_index} not found under surface {parent}"),
        );
    };
    let new_cwd = optional_str(params, "cwd");
    let command = optional_str(params, "command");

    // cwd 변경이 들어왔으면 PTY 자체를 새 working_dir 로 교체. 아니면 Ctrl-C 로 기존
    // 프로세스만 종료(호출자가 command 로 재실행).
    if new_cwd.is_some() {
        let mut respawn_params = json!({ "surface_id": entry.child_surface_id });
        if let Some(c) = new_cwd.as_deref() {
            respawn_params["cwd"] = Value::String(c.to_string());
        }
        if let Err(e) = unwrap_ok(
            surface::handle_surface_respawn_terminal(
                core,
                state,
                engine,
                id.clone(),
                &respawn_params,
            ),
            &id,
        ) {
            return e;
        }
    } else {
        let combo = json!({
            "surface_id": entry.child_surface_id,
            "key": "c",
            "modifiers": ["ctrl"],
        });
        if let Err(e) = unwrap_ok(
            surface::handle_surface_send_combo(core, state, engine, id.clone(), &combo),
            &id,
        ) {
            return e;
        }
    }
    if let Some(cmd) = &command {
        let send_params = json!({ "surface_id": entry.child_surface_id, "text": cmd });
        if let Err(e) = unwrap_ok(
            surface::handle_surface_send(core, state, engine, id.clone(), &send_params),
            &id,
        ) {
            return e;
        }
    }

    // role/nickname/cwd 갱신 + idle 초기화.
    let new_role = optional_str(params, "role");
    let new_nick = optional_str(params, "nickname");
    engine
        .child_terminals
        .update_child(parent, child_index, |e| {
            if let Some(r) = new_role {
                e.role = Some(r);
            }
            if let Some(n) = new_nick {
                e.nickname = Some(n);
            }
            if let Some(c) = new_cwd {
                e.cwd = Some(c);
            }
        });
    engine
        .child_terminals
        .set_idle(entry.child_surface_id, false);
    engine.child_terminals.save();
    JsonRpcResponse::success(
        id,
        json!({
            "child_surface_id": entry.child_surface_id,
            "child_index": entry.index,
        }),
    )
}

pub(crate) fn handle_broadcast(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let parent = match resolve_parent(engine, params, &id) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let text = match require_str(params, "text", &id) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let role_filter = optional_str(params, "role");

    // 대상 목록을 먼저 스냅샷(불변 빌림 해제 후 mutable send 반복).
    let targets: Vec<u32> = engine
        .child_terminals
        .list_children(parent)
        .iter()
        .filter(|c| match &role_filter {
            Some(r) => c.role.as_deref() == Some(r.as_str()),
            None => true,
        })
        .map(|c| c.child_surface_id)
        .collect();

    let (body, submit) = build_broadcast_payload(&text);
    let mut sent_ids: Vec<u32> = Vec::new();
    for sid in targets {
        // 1) 본문(제출 `\r` 미포함, 멀티라인은 bracketed paste). 2) 호출자가 trailing `\r` 을
        //    넣었을 때만 제출 Enter 를 별도 write 로 분리 — 길이 무관 결정적 제출.
        let body_params = json!({ "surface_id": sid, "text": body.clone() });
        if let Err(e) = unwrap_ok(
            surface::handle_surface_send(core, state, engine, id.clone(), &body_params),
            &id,
        ) {
            tracing::warn!("terminal.broadcast surface.send (sid={sid}) failed: {e:?}");
        } else if submit {
            let cr_params = json!({ "surface_id": sid, "text": "\r" });
            if let Err(e) = unwrap_ok(
                surface::handle_surface_send(core, state, engine, id.clone(), &cr_params),
                &id,
            ) {
                tracing::warn!("terminal.broadcast submit (sid={sid}) failed: {e:?}");
            }
        }
        sent_ids.push(sid);
    }
    JsonRpcResponse::success(
        id,
        json!({ "sent_count": sent_ids.len(), "children": sent_ids }),
    )
}

/// 에이전트 hook 이 idle/needs_input 신호를 넣는 진입점. state ∈ {idle, needs_input,
/// active}. 05 에서 codex/claude hook 핸들러가 이 method 를 호출한다.
pub(crate) fn handle_set_state(
    engine: &mut CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let surface_id = match require_u32(params, "surface", &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let new_state = match require_str(params, "state", &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match new_state.as_str() {
        "idle" => engine.child_terminals.set_idle(surface_id, true),
        "active" => engine.child_terminals.set_idle(surface_id, false),
        "needs_input" => engine.child_terminals.set_needs_input(surface_id, true),
        other => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("unknown state '{other}' (supported: idle, needs_input, active)"),
            );
        }
    }
    engine.child_terminals.save();
    JsonRpcResponse::success(
        id,
        json!({ "ok": true, "surface_id": surface_id, "state": new_state }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::attach::OccupancyTier;

    fn engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    fn child(sid: u32, index: u32) -> ChildEntry {
        ChildEntry {
            child_surface_id: sid,
            index,
            cwd: None,
            role: None,
            nickname: None,
        }
    }

    fn ok(resp: JsonRpcResponse) -> Value {
        resp.result.expect("expected success result")
    }

    /// `engine()`의 기본 workspace 는 surface 1개뿐이라, adopt 테스트가 필요로 하는
    /// "parent 와 별개인 이미 존재하는 surface"를 만들려면 두 번째 tab 을 직접
    /// 끼워 넣어야 한다. `find_surface_by_id`는 Surface 트리만 순회하므로(터미널
    /// 실제 PTY/`engine.terminals` 등록 여부와 무관) id-marker `TerminalSurface`
    /// 하나로 충분하다.
    fn add_extra_surface(e: &mut CoreState, surface_id: u32) {
        let pane_id = e.workspaces[0].pane_layout().all_pane_ids()[0];
        let tab = crate::model::Tab::new_with_surface(
            e.next_ids.next_tab(),
            "extra".to_string(),
            Box::new(crate::model::TerminalSurface { id: surface_id }),
        );
        e.workspaces[0]
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .expect("default pane")
            .tabs
            .push(tab);
    }

    #[test]
    fn spawn_kill_occupancy_wiring() {
        // handle_spawn/handle_kill 이 소비하는 03 경계(occupy_soft/release_occupancy)를
        // registry 등록/해제와 함께 CoreState 레벨에서 검증한다. tab.create/surface.send
        // GUI 파트는 이미 검증된 재사용 핸들러라 제외(그 경로 e2e 는 debug 인스턴스).
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let c = 5000u32;

        // spawn 경로: registry 등록 + soft 점유(주체=parent).
        let idx = e.child_terminals.next_index_for(parent);
        e.child_terminals.register_child(parent, child(c, idx));
        e.occupy_soft(c, parent, Some("worker".into())).unwrap();
        let occ = e.attach.occupancy_of(c).expect("soft occupancy present");
        assert_eq!(occ.tier, OccupancyTier::Soft);
        assert_eq!(occ.parent, Some(parent));
        // soft 는 hard 아님 → surface.list 의 "attached": false.
        assert!(!e.attach.is_hard_occupied(c));

        // kill 경로: release_occupancy + registry 제거 → 점유 없음.
        assert!(e.child_terminals.remove_child(parent, idx).is_some());
        e.release_occupancy(c);
        assert!(e.attach.occupancy_of(c).is_none());
    }

    #[test]
    fn adopt_registers_existing_surface_without_new_tab() {
        // child_terminals.save() 는 실제 `~/.tasty/child-terminals.json` 에 쓰므로,
        // 병렬 실행되는 다른 테스트와 같은 surface id 를 재사용하면 파일 경합으로
        // 서로 오염될 수 있다 — 이 모듈의 다른 테스트가 안 쓰는 값을 쓴다.
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let target = 6101u32; // 이미 존재하는(=spawn 아닌) surface
        add_extra_surface(&mut e, target);

        let resp = handle_adopt(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "target": target }),
        );
        assert!(resp.error.is_none());

        let occ = e
            .attach
            .occupancy_of(target)
            .expect("soft occupancy present");
        assert_eq!(occ.tier, OccupancyTier::Soft);
        assert_eq!(occ.parent, Some(parent));
        assert_eq!(e.child_terminals.parent_of_child(target), Some(parent));
    }

    #[test]
    fn adopt_rejects_already_registered_child() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let target = 6102u32;
        add_extra_surface(&mut e, target);

        let _ = handle_adopt(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "target": target }),
        );
        let resp2 = handle_adopt(
            &mut e,
            json!(2),
            &json!({ "surface": parent, "target": target }),
        );
        assert!(resp2.error.is_some()); // 중복 등록 거부
    }

    #[test]
    fn adopt_rejects_nonexistent_surface() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let resp = handle_adopt(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "target": 999999u32 }),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn adopt_rejects_self_adoption() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let resp = handle_adopt(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "target": parent }),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn adopt_rejects_hard_occupied_target_and_leaves_registry_unchanged() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let target = 6103u32;
        add_extra_surface(&mut e, target);
        e.attach
            .acquire(target, /* hard occupancy client id */ 1)
            .unwrap();

        let resp = handle_adopt(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "target": target }),
        );
        assert!(resp.error.is_some());
        // 실패 시 registry 에 아무 흔적도 남지 않아야 한다(연산 순서 검증).
        assert!(e.child_terminals.parent_of_child(target).is_none());
        assert!(e.attach.is_hard_occupied(target)); // 기존 hard 점유는 건드리지 않음
    }

    #[test]
    fn release_clears_registry_and_occupancy_but_keeps_surface() {
        // child_terminals.save() 는 실제 `~/.tasty/child-terminals.json` 에 쓰므로,
        // 다른 테스트(병렬 실행)와 같은 surface id 를 재사용하면 파일 경합으로
        // 서로 오염될 수 있다 — 이 모듈의 다른 테스트가 안 쓰는 값을 쓴다.
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let c = 5701u32;
        add_extra_surface(&mut e, c);

        let idx = e.child_terminals.next_index_for(parent);
        e.child_terminals.register_child(parent, child(c, idx));
        e.occupy_soft(c, parent, Some("worker".into())).unwrap();

        let resp = handle_release(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "child": idx }),
        );
        assert!(resp.error.is_none());

        // 관계·점유는 사라짐. surface.close 를 호출하지 않았다는 사실은
        // handle_release 구현에 그 호출이 없다는 코드 리뷰로 재확인.
        assert!(e.child_terminals.find_child(parent, idx).is_none());
        assert!(e.attach.occupancy_of(c).is_none());
    }

    #[test]
    fn release_rejects_unregistered_child_index() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let resp = handle_release(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "child": 999u32 }),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn release_does_not_touch_unrelated_hard_occupancy() {
        // release 가 tier-무관 release_occupancy 가 아니라 release_soft_occupancy 를
        // 쓴다는 핵심 불변식 — 대상이 우연히 hard 점유 중이어도 절대 손대지 않아야 한다.
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let c = 5702u32;
        add_extra_surface(&mut e, c);
        let idx = e.child_terminals.next_index_for(parent);
        e.child_terminals.register_child(parent, child(c, idx));
        e.occupy_soft(c, parent, None).unwrap();
        let other = 5703u32;
        e.attach.acquire(other, 1).unwrap();

        let resp = handle_release(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "child": idx }),
        );
        assert!(resp.error.is_none());
        assert!(e.attach.occupancy_of(c).is_none()); // soft 는 정상 해제
        assert!(e.attach.is_hard_occupied(other)); // 무관한 hard 점유는 그대로
    }

    #[test]
    fn children_reconcile_prunes_dead_child() {
        // 죽은 자식 surface 는 children 접근 시 self-heal reconcile 로 목록에서 제거.
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        e.child_terminals.register_child(parent, child(99999, 0));
        let resp = handle_children(&mut e, json!(1), &json!({ "surface": parent }));
        let n = ok(resp)["children"].as_array().unwrap().len();
        assert_eq!(n, 0);
    }

    #[test]
    fn parent_lookup_unregistered_is_none() {
        let mut e = engine();
        let resp = handle_parent(&mut e, json!(1), &json!({ "surface": 424242 }));
        assert_eq!(ok(resp)["status"], "none");
    }

    #[test]
    fn set_state_rejects_unknown() {
        let mut e = engine();
        let resp = handle_set_state(
            &mut e,
            json!(1),
            &json!({ "surface": 5000, "state": "bogus" }),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn set_state_updates_registry() {
        let mut e = engine();
        e.child_terminals.register_child(7, child(5000, 0));
        let _ = handle_set_state(
            &mut e,
            json!(1),
            &json!({ "surface": 5000, "state": "idle" }),
        );
        assert_eq!(e.child_terminals.state_of(5000), "idle");
        let _ = handle_set_state(
            &mut e,
            json!(1),
            &json!({ "surface": 5000, "state": "needs_input" }),
        );
        assert_eq!(e.child_terminals.state_of(5000), "needs_input");
        let _ = handle_set_state(
            &mut e,
            json!(1),
            &json!({ "surface": 5000, "state": "active" }),
        );
        assert_eq!(e.child_terminals.state_of(5000), "active");
    }

    #[test]
    fn broadcast_payload_splits_trailing_cr_for_submit() {
        // 63자+ 단일라인 + trailing `\r`: 본문엔 `\r` 이 섞이지 않고(64-codepoint burst 회피),
        // 제출 플래그만 켜진다 → 호출부가 별도 write 로 결정적 제출. (paste-threshold 회귀 가드)
        let body63 = "a".repeat(63);
        let (payload, submit) = build_broadcast_payload(&format!("{body63}\r"));
        assert_eq!(payload, body63, "본문에 제출 \\r 이 섞이면 안 됨");
        assert!(!payload.contains('\r'));
        assert!(submit);
    }

    #[test]
    fn broadcast_payload_no_trailing_cr_is_injection_only() {
        // trailing `\r` 이 없으면 "sent as-is" — 제출하지 않는다(빌트인 계약 보존).
        let (payload, submit) = build_broadcast_payload(&"b".repeat(80));
        assert_eq!(payload, "b".repeat(80));
        assert!(!submit);
    }

    #[test]
    fn broadcast_payload_multiline_wraps_bracketed_and_submits() {
        // 멀티라인 + trailing `\r`: bracketed paste 로 감싸고 제출 플래그 on,
        // 감싼 본문 안에 제출 `\r` 은 없다.
        let (payload, submit) = build_broadcast_payload("line1\nline2\r");
        assert_eq!(payload, "\u{1b}[200~line1\nline2\u{1b}[201~");
        assert!(!payload.contains('\r'));
        assert!(submit);
    }
}
