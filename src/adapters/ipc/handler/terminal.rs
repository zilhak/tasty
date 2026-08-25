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
use crate::core::state::child_liveness::ChildLiveness;
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

/// child 조회 실패 시 **왜** 못 찾았는지까지 담은 메시지. kill/release/respawn 공용.
///
/// `--child` 는 부모별 index(`next_index_for` 가 0부터 발급)지 `child_surface_id` 가
/// 아닌데, 둘 다 그냥 정수라 혼동하기 쉽다. 레지스트리는 이미 구분에 필요한 걸 다
/// 갖고 있으므로(`parent_of_child`), 사실만 알리지 말고 다음 행동을 지목한다.
///
/// 두 번호 공간은 실제로 겹칠 수 있어(새 인스턴스는 surface id 가 1,2,3…) 넘어온 값을
/// surface_id 로 **자동 해석하지는 않는다** — 안내만 하고 인자 의미는 index 로 고정한다.
fn child_not_found_message(
    reg: &crate::core::child_terminal::ChildTerminalRegistry,
    parent: u32,
    given: u32,
) -> String {
    let base = format!("child {given} not found under surface {parent}");

    // 넘어온 값이 실은 child_surface_id 인 경우 — 대응하는 index 를 짚어준다.
    if let Some(owner) = reg.parent_of_child(given)
        && let Some(index) = reg
            .list_children(owner)
            .iter()
            .find(|c| c.child_surface_id == given)
            .map(|c| c.index)
    {
        return if owner == parent {
            format!(
                "{base}. {given} is a child_surface_id, not a child index — use `--child {index}`"
            )
        } else {
            format!(
                "{base}. {given} is a child_surface_id under a different parent \
                 — use `--surface {owner} --child {index}`"
            )
        };
    }

    // 그 외(오타·범위 밖·이미 정리됨) — 유효한 index 를 제시한다.
    let mut indices: Vec<u32> = reg.list_children(parent).iter().map(|c| c.index).collect();
    if indices.is_empty() {
        return format!("{base} (no children registered under surface {parent})");
    }
    indices.sort_unstable();
    let count = indices.len();
    format!(
        "{base} (valid child indices: {}; {count} children)",
        format_index_ranges(&indices)
    )
}

/// 정렬된 index 목록을 사람이 읽을 범위 표기로 압축한다 — `0-42, 45, 48-57`.
/// kill 로 중간이 빠진 목록을 `0-57` 로 뭉뚱그리면 없는 index 를 있다고 말하게 된다.
fn format_index_ranges(sorted: &[u32]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let start = sorted[i];
        let mut end = start;
        while i + 1 < sorted.len() && sorted[i + 1] == end + 1 {
            i += 1;
            end = sorted[i];
        }
        parts.push(if start == end {
            start.to_string()
        } else {
            format!("{start}-{end}")
        });
        i += 1;
    }
    parts.join(", ")
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

/// [`send_text_to_surface_with_ack`] 실패 사유 — hard-occupied(다른 attach 가
/// 이미 점유해 서버측 write 가 막힘)와 surface 미존재는 원인이 달라 호출자가
/// 서로 다른 에러 메시지를 보여줄 수 있도록 구분한다.
enum SendTextError {
    HardOccupied,
    NotFound,
}

/// [`send_body_then_submit`] 1단계 — `apply_send_to_surface`(core/mod.rs)와 동일한
/// attach 점유 체크 + surface 초기화를 거쳐 ack 가능한 방식으로 본문을 큐잉한다.
fn send_text_to_surface_with_ack(
    engine: &mut CoreState,
    surface_id: u32,
    text: &str,
) -> Result<tasty_terminal::WriteAck, SendTextError> {
    if engine.attach.is_hard_occupied(surface_id) {
        return Err(SendTextError::HardOccupied);
    }
    engine.ensure_surface_initialized(surface_id);
    engine
        .find_terminal_by_id_mut(surface_id)
        .map(|terminal| terminal.send_key_with_ack(text))
        .ok_or(SendTextError::NotFound)
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
    let ack = send_text_to_surface_with_ack(engine, surface_id, &body).map_err(|e| {
        let msg = match e {
            SendTextError::HardOccupied => format!(
                "Surface {surface_id} is attached elsewhere (hard-occupied) — release the \
                 attach or target a different surface"
            ),
            SendTextError::NotFound => format!("Surface {surface_id} not found"),
        };
        JsonRpcResponse::invalid_params(id.clone(), msg)
    })?;

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
///
/// 라우터 가드가 이 로직을 재사용하던 시절이 있어 `pub(super)` 였으나, 지금은
/// `terminal.spawn` 의 대상 판정이 최종 pane 기준으로 옮겨가(`spawn_target_guard`)
/// 이 파일 밖 호출자가 없다.
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

/// `resolve_workspace_id` 실패 시 메시지. 표시 이름(사용자가 UI 에서 보는 이름,
/// 예: "Workspace 1")과 실제 id 는 다를 수 있다(`resolve_workspace_id` 는 숫자 id
/// exact match 또는 정확한 `name` 필드 match 만 본다) — 표시 이름 매칭은 중복 이름
/// 시 모호성이 생기므로 추가하지 않고, 혼동을 줄이는 힌트만 덧붙인다.
fn workspace_not_found_message(workspace_param: &str) -> String {
    format!(
        "workspace '{workspace_param}' not found — pass the numeric workspace id \
         (see `tasty list workspaces`), not a displayed name (a name shown in the UI \
         may not match the underlying id)"
    )
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
        return JsonRpcResponse::invalid_params(id, workspace_not_found_message(&workspace_param));
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
    // 대상 판정은 **최종 pane 기준**이다. `pane` 오버라이드는 `workspace` 파라미터와
    // 다른 워크스페이스를 가리킬 수 있고, 실제로 tab 이 생기는 곳은 pane 쪽이다.
    // 아래 가드가 mirror/hard-occupied 를 여기서 잡아내지 못하면 그 뒤 `tab.create`
    // 가 원격으로 forward 되어 **로컬은 실패하는데 원격에는 탭이 남는** 고아가 된다.
    if let Some(denied) = super::spawn_target_guard(engine, pane_id, &id) {
        return denied;
    }
    // 응답의 workspace_id 도 같은 기준으로 맞춘다 — `workspace` 파라미터를 그대로
    // 돌려주면 오버라이드된 pane 이 다른 워크스페이스일 때 실제 생성 위치와 어긋난다.
    let ws_id = engine
        .find_workspace_index_for_pane(pane_id)
        .and_then(|i| engine.workspaces.get(i))
        .map(|w| w.id)
        .unwrap_or(ws_id);

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
    if clear_idle_for_new_prompt(&mut engine.child_terminals, surface_id) {
        engine.child_terminals.save();
    }
    JsonRpcResponse::success(id, json!({ "sent": true, "surface_id": surface_id }))
}

/// 새 프롬프트를 밀어넣은 직후 대상 자식의 `idle` 플래그를 내린다. 상태가 실제로
/// 바뀔 수 있는 대상이었으면 `true` — 호출자는 그때만 `save()` 한다.
///
/// **왜 필요한가**: 프롬프트를 방금 주입했는데 상태가 `idle` 이라는 건 어느
/// 소비자 관점에서도 거짓이다. `derive_child_state` 는 registry 가 보고한
/// `idle`/`needs_input` 을 우선순위 2 에서 그대로 통과시키므로
/// (`core/state/child_liveness.rs`), 이 플래그가 남아 있으면 `terminal.state` 를
/// 폴링하는 완료 판정 전략의 첫 tick 이 **직전 턴의 stale `idle`** 을 읽고 자식이
/// 일을 시작하기도 전에 노드를 완료 처리한다. 에이전트가 `active` 를 다시 밀어
/// 넣는 경로(hook `prompt-submit` → `terminal.set_state`)는 존재하지만 그게 도는
/// 데 걸리는 시간이 폴링 간격보다 길 수 있다 — 주입한 쪽이 그 자리에서 내린다.
///
/// `set_idle(_, false)` 는 `needs_input` 도 함께 내린다
/// (`core/child_terminal.rs`) — 프롬프트에 답한 경우까지 한 번에 맞춰진다.
///
/// **등록된 자식에만 적용한다**: `set_idle` 은 `HashMap::insert` 라 자식이 아닌
/// 일반 surface 에도 항목을 만들고, 그 항목은 surface 가 살아있는 한
/// `reconcile_with_live_surfaces` 도 지우지 않는다. 게다가 미등록 surface 의
/// `state_of` 는 이미 `"active"` 라 바꿀 상태 자체가 없다 — 남는 건 가짜
/// `last_state_report_at` 기록뿐이므로 손대지 않는 게 맞다.
fn clear_idle_for_new_prompt(
    registry: &mut crate::core::child_terminal::ChildTerminalRegistry,
    surface_id: u32,
) -> bool {
    if registry.parent_of_child(surface_id).is_none() {
        return false;
    }
    registry.set_idle(surface_id, false);
    true
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
    // 라이브 집합은 자식마다 다시 순회하지 않도록 여기서 한 번만 계산한다.
    let live = engine.live_surface_ids();
    let children: Vec<Value> = engine
        .child_terminals
        .list_children(parent)
        .iter()
        .map(|c| {
            let mut item =
                liveness_fields(engine.child_liveness_with_live(c.child_surface_id, &live));
            item.insert("index".into(), json!(c.index));
            item.insert("surface_id".into(), json!(c.child_surface_id));
            item.insert("role".into(), json!(c.role));
            item.insert("nickname".into(), json!(c.nickname));
            item.insert("cwd".into(), json!(c.cwd));
            Value::Object(item)
        })
        .collect();
    JsonRpcResponse::success(id, json!({ "children": children }))
}

/// 파생 판정 3 축(`state`/`evidence`/`confidence`)을 응답 필드로 펴는 **단일 지점**.
///
/// `terminal.children` 항목과 `terminal.state` 단건이 둘 다 여기를 거쳐 객체를 만들기
/// 때문에 키 집합과 값이 구조적으로 일치한다 — ADR-0072 가 판정을 한 헬퍼
/// (`CoreState::child_liveness*`)로 통일해 둔 것을 직렬화 단계에서 되돌리지 않는다.
/// 각 필드가 가질 수 있는 값과 조합은 `docs/features/child-terminal/index.md` 의
/// 판정 우선순위표가 SoT 다.
fn liveness_fields(liveness: ChildLiveness) -> serde_json::Map<String, Value> {
    let mut fields = serde_json::Map::new();
    fields.insert("state".into(), json!(liveness.state.as_str()));
    fields.insert("evidence".into(), json!(liveness.evidence.as_str()));
    fields.insert("confidence".into(), json!(liveness.confidence.as_str()));
    fields
}

/// 자식 단건 상태 조회. `handle_children`/`handle_parent`
/// 와 동형으로 대상 child surface 를 `surface` 파라미터로 직접 지정한다(포커스
/// 독립 — CLAUDE.md 원칙 3).
///
/// **결정 4**: `ChildTerminalRegistry::state_of` 자신은 미등록 surface 에
/// `"active"` fallback 계약을 그대로 유지한다(`src/core/child_terminal.rs` 의
/// `state_of` 와 그 테스트는 불변). 파생 판정은 그 위에서 라이브 surface 트리 ·
/// PTY 관측과 합성해 만들어진다 — registry 자체의 self-heal
/// (`reconcile_child_terminals`)이 이미 죽은 항목을 지웠더라도(관계가 사라져도) 그
/// surface 가 라이브인지는 독립적으로 판정해야 하므로 reconcile 결과에 기대지 않고
/// `live_surface_ids()` 를 직접 대조한다.
///
/// 판정 자체는 `handle_children` 과 **같은** `CoreState::child_liveness*` 헬퍼를
/// 쓴다 — 두 경로가 갈리면 목록과 단건 조회가 서로 다른 값을 보고한다.
pub(crate) fn handle_state(engine: &mut CoreState, id: Value, params: &Value) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let surface_id = match require_u32(params, "surface", &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mut out = liveness_fields(engine.child_liveness(surface_id));
    out.insert("surface_id".into(), json!(surface_id));
    JsonRpcResponse::success(id, Value::Object(out))
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
            child_not_found_message(&engine.child_terminals, parent, child_index),
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
            child_not_found_message(&engine.child_terminals, parent, child_index),
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
            child_not_found_message(&engine.child_terminals, parent, child_index),
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

/// 자식 하나에 broadcast 본문(+ 선택적 제출 Enter)을 밀어넣는다. 본문 write 가
/// 성공했으면 `true` — 호출자는 그때만 상태 플래그를 정리한다. 실패는 그 자식만
/// 건너뛰고 나머지 대상에는 계속 보낸다(부분 성공 허용).
///
/// 1) 본문(제출 `\r` 미포함, 멀티라인은 bracketed paste). 2) 호출자가 trailing `\r` 을
///    넣었을 때만 제출 Enter 를 별도 write 로 분리 — 길이 무관 결정적 제출.
#[allow(clippy::too_many_arguments)]
fn send_broadcast_to_child(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    id: &Value,
    sid: u32,
    body: &str,
    submit: bool,
) -> bool {
    let body_params = json!({ "surface_id": sid, "text": body });
    if let Err(e) = unwrap_ok(
        surface::handle_surface_send(core, state, engine, id.clone(), &body_params),
        id,
    ) {
        tracing::warn!("terminal.broadcast surface.send (sid={sid}) failed: {e:?}");
        return false;
    }
    if submit {
        let cr_params = json!({ "surface_id": sid, "text": "\r" });
        if let Err(e) = unwrap_ok(
            surface::handle_surface_send(core, state, engine, id.clone(), &cr_params),
            id,
        ) {
            tracing::warn!("terminal.broadcast submit (sid={sid}) failed: {e:?}");
        }
    }
    true
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
    let mut idle_cleared = false;
    for sid in targets {
        if send_broadcast_to_child(core, state, engine, &id, sid, &body, submit) {
            // tell 과 같은 이유로 여기서도 내린다 — 프롬프트를 밀어넣은 자식이
            // `idle` 로 남아 있으면 그 상태를 읽는 모든 소비자가 거짓을 본다.
            idle_cleared |= clear_idle_for_new_prompt(&mut engine.child_terminals, sid);
        }
        sent_ids.push(sid);
    }
    if idle_cleared {
        engine.child_terminals.save();
    }
    JsonRpcResponse::success(
        id,
        json!({ "sent_count": sent_ids.len(), "children": sent_ids }),
    )
}

/// 에이전트 hook 이 idle/needs_input 신호를 넣는 진입점. state ∈ {idle, needs_input,
/// active}. 05 에서 codex/claude hook 핸들러가 이 method 를 호출한다.
///
/// **파생 상태는 입력으로 받지 않는다 (출력 전용)** — `exited`/`stale` 은 호스트가
/// 라이브 트리·PTY 관측에서만 만들어내는 값이라(`core/state/child_liveness.rs`),
/// hook 이 그것을 registry 에 밀어넣을 수 있으면 관측 축이 다시 push 캐시로 퇴화한다.
/// 아래 `other` 분기가 그 두 값을 포함한 모든 비-hook 값을 거부한다.
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

    /// index 목록 압축 — 연속 구간은 `a-b`, 단독은 그대로, 빈 구간은 건너뛴다.
    #[test]
    fn index_ranges_compress_contiguous_runs() {
        assert_eq!(format_index_ranges(&[0, 1, 2, 3]), "0-3");
        assert_eq!(format_index_ranges(&[7]), "7");
        assert_eq!(format_index_ranges(&[0, 1, 2, 5, 8, 9]), "0-2, 5, 8-9");
        // kill 로 중간이 빠진 목록을 `0-57` 로 뭉뚱그리지 않는다.
        assert_eq!(format_index_ranges(&[0, 57]), "0, 57");
    }

    /// workspace not-found 메시지는 표시 이름 대신 숫자 id 를 쓰라는 힌트를 담는다
    /// — 표시 이름(name) 매칭을 추가하는 대신 택한 최소 침습적 개선.
    #[test]
    fn workspace_not_found_message_hints_numeric_id() {
        let msg = workspace_not_found_message("1");
        assert!(msg.contains("workspace '1' not found"), "{msg}");
        assert!(msg.contains("numeric workspace id"), "{msg}");
        assert!(msg.contains("tasty list workspaces"), "{msg}");
    }

    /// surface_id 를 `--child` 에 넣은 흔한 오용 — 같은 부모면 올바른 index 를 짚어준다.
    #[test]
    fn child_not_found_points_at_index_when_given_a_surface_id() {
        let mut reg = crate::core::child_terminal::ChildTerminalRegistry::default();
        reg.register_child(3157, child(3204, 31));
        let msg = child_not_found_message(&reg, 3157, 3204);
        assert!(msg.contains("child_surface_id, not a child index"), "{msg}");
        assert!(msg.contains("`--child 31`"), "{msg}");
    }

    /// 다른 부모에 속한 child_surface_id 면 부모까지 함께 제시한다.
    #[test]
    fn child_not_found_points_at_other_parent() {
        let mut reg = crate::core::child_terminal::ChildTerminalRegistry::default();
        reg.register_child(9000, child(3204, 4));
        let msg = child_not_found_message(&reg, 3157, 3204);
        assert!(msg.contains("under a different parent"), "{msg}");
        assert!(msg.contains("`--surface 9000 --child 4`"), "{msg}");
    }

    /// surface_id 도 아닌 값(오타/범위 밖)이면 유효 index 범위를 제시한다.
    #[test]
    fn child_not_found_lists_valid_indices() {
        let mut reg = crate::core::child_terminal::ChildTerminalRegistry::default();
        for i in 0..3 {
            reg.register_child(3157, child(100 + i, i));
        }
        let msg = child_not_found_message(&reg, 3157, 999);
        assert!(msg.contains("valid child indices: 0-2"), "{msg}");
        assert!(msg.contains("3 children"), "{msg}");
    }

    /// child 가 하나도 없는 부모는 범위 대신 그 사실을 알린다.
    #[test]
    fn child_not_found_reports_empty_parent() {
        let reg = crate::core::child_terminal::ChildTerminalRegistry::default();
        let msg = child_not_found_message(&reg, 3157, 0);
        assert!(msg.contains("no children registered"), "{msg}");
    }

    /// tell 직후 상태가 `idle` 로 남으면, `terminal.state` 를 폴링하는 완료 판정
    /// 전략의 첫 tick 이 직전 턴의 stale `idle` 을 읽고 자식이 답을 시작하기도
    /// 전에 노드를 완료 처리한다.
    #[test]
    fn tell_clears_idle_flag_on_target_child() {
        let mut reg = crate::core::child_terminal::ChildTerminalRegistry::default();
        reg.register_child(10, child(50, 0));
        reg.set_idle(50, true);
        assert_eq!(reg.state_of(50), "idle");

        assert!(clear_idle_for_new_prompt(&mut reg, 50));
        assert_eq!(reg.state_of(50), "active");
    }

    /// `set_idle(_, false)` 는 `needs_input` 도 함께 내린다 — 권한 프롬프트에 tell
    /// 로 답한 자식이 계속 `needs_input` 으로 보이면 안 된다.
    #[test]
    fn tell_clears_needs_input_too() {
        let mut reg = crate::core::child_terminal::ChildTerminalRegistry::default();
        reg.register_child(10, child(50, 0));
        reg.set_needs_input(50, true);
        assert_eq!(reg.state_of(50), "needs_input");

        assert!(clear_idle_for_new_prompt(&mut reg, 50));
        assert_eq!(reg.state_of(50), "active");
    }

    /// 자식이 아닌 일반 surface 에 tell 하면 registry 를 건드리지 않는다 — 미등록
    /// surface 는 이미 `"active"` 라 바꿀 상태가 없고, 항목을 만들면 살아있는 동안
    /// reconcile 도 지우지 않는 가짜 상태 보고 이력만 남는다.
    #[test]
    fn tell_does_not_touch_registry_for_non_child_surface() {
        let mut reg = crate::core::child_terminal::ChildTerminalRegistry::default();
        assert!(!clear_idle_for_new_prompt(&mut reg, 777));
        assert_eq!(reg.state_of(777), "active");
        assert_eq!(reg.last_state_report_at(777), None);
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

    /// `resolve_parent` 폴백(단일 엔진 내부): parent 가 정확히 1개면 `--surface`
    /// 생략을 그대로 허용한다(하위 호환). 다중 윈도우 세션의 모호성 자체는
    /// `App::find_request_owner` 레벨(엔진 하나만으로는 판단 불가)에서 별도로
    /// 막는다 — `src/app/request_owner.rs` 의
    /// `ambiguous_parent_fallback_requires_surface` 참고.
    #[test]
    fn resolve_parent_omitted_surface_succeeds_with_single_parent() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        add_extra_surface(&mut e, 59010);
        let idx = e.child_terminals.next_index_for(parent);
        e.child_terminals.register_child(parent, child(59010, idx));

        let resp = handle_release(&mut e, json!(1), &json!({ "child": idx }));
        assert!(resp.error.is_none(), "{:?}", resp.error);
    }

    /// 같은 엔진에 parent 가 2개 이상 등록돼 있는데 `--surface` 를 생략하면,
    /// `single_parent()` 가 조용히 아무 하나를 고르지 않고 명시적 에러를 낸다.
    #[test]
    fn resolve_parent_omitted_surface_errors_with_multiple_parents_in_one_engine() {
        let mut e = engine();
        let parent1 = e.workspaces[0].all_surface_ids()[0];
        let parent2 = 59020u32;
        add_extra_surface(&mut e, 59030);
        add_extra_surface(&mut e, 59040);
        let idx1 = e.child_terminals.next_index_for(parent1);
        e.child_terminals
            .register_child(parent1, child(59030, idx1));
        let idx2 = e.child_terminals.next_index_for(parent2);
        e.child_terminals
            .register_child(parent2, child(59040, idx2));

        let resp = handle_release(&mut e, json!(1), &json!({ "child": idx1 }));
        assert!(resp.error.is_some());
    }

    /// hard-occupied surface 로의 write 시도는 "not found" 가 아니라 별도
    /// 사유(attach 점유)로 실패해야 한다 — 둘을 뭉뚱그리면 "존재하는데 왜 못
    /// 찾지" 라는 오진을 유발한다.
    #[test]
    fn send_text_with_ack_distinguishes_hard_occupied_from_not_found() {
        let mut e = engine();
        let target = 5801u32;
        add_extra_surface(&mut e, target);
        e.attach.acquire(target, 1).unwrap();

        let err = send_text_to_surface_with_ack(&mut e, target, "hi")
            .err()
            .expect("hard-occupied surface must fail");
        assert!(matches!(err, SendTextError::HardOccupied));
    }

    /// 존재하지 않는 surface 는 `NotFound` 로 구분된다(hard-occupied 아님).
    #[test]
    fn send_text_with_ack_reports_not_found_for_missing_surface() {
        let mut e = engine();
        let err = send_text_to_surface_with_ack(&mut e, 424_242, "hi")
            .err()
            .expect("missing surface must fail");
        assert!(matches!(err, SendTextError::NotFound));
    }

    /// 점유 없는 정상 surface 는 성공한다(회귀 방지 — 위 두 실패 케이스와 대비).
    /// `add_extra_surface` 는 layout 상 placeholder 만 만들 뿐 실제 `Terminal` 을
    /// `e.terminals` 에 등록하지 않으므로(deferred 아님 → `ensure_surface_initialized`
    /// 가 스킵), 기본 workspace 가 `CoreState::new` 시점에 이미 실제 PTY 로 spawn 해
    /// 등록해둔 첫 surface 를 그대로 쓴다.
    #[test]
    fn send_text_with_ack_succeeds_for_free_terminal() {
        let mut e = engine();
        let target = e.workspaces[0].all_surface_ids()[0];

        assert!(send_text_to_surface_with_ack(&mut e, target, "hi").is_ok());
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

    /// `children` 항목은 `state` 뿐 아니라 판정 근거 2 축을 함께 싣는다 — 이게 없으면
    /// 소비자가 확정 판정과 휴리스틱을 구분할 수 없다(ADR-0072 가 분리해 둔 축이
    /// 응답 단계에서 사라진다).
    #[test]
    fn children_item_carries_evidence_and_confidence() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let target = 5901u32;
        add_extra_surface(&mut e, target);
        e.child_terminals.register_child(parent, child(target, 0));

        let resp = ok(handle_children(
            &mut e,
            json!(1),
            &json!({ "surface": parent }),
        ));
        let item = &resp["children"].as_array().expect("children")[0];
        for key in ["state", "evidence", "confidence"] {
            assert!(
                item.get(key).and_then(|v| v.as_str()).is_some(),
                "children 항목에 '{key}' 가 문자열로 실려야 한다: {item}"
            );
        }
    }

    /// 목록과 단건이 **같은 판정 3 축**을 보고해야 한다. ADR-0072 가 판정 헬퍼를
    /// 하나로 합쳤어도 직렬화를 각자 하면 다시 갈릴 수 있어, 두 응답을 직접 대조해
    /// 고정한다.
    #[test]
    fn children_and_state_report_identical_liveness_fields() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let target = 5902u32;
        add_extra_surface(&mut e, target);
        e.child_terminals.register_child(parent, child(target, 0));

        let list = ok(handle_children(
            &mut e,
            json!(1),
            &json!({ "surface": parent }),
        ));
        let item = list["children"].as_array().expect("children")[0].clone();
        let single = ok(handle_state(
            &mut e,
            json!(2),
            &json!({ "surface": target }),
        ));

        for key in ["state", "evidence", "confidence"] {
            assert_eq!(
                item[key], single[key],
                "'{key}' 가 목록과 단건에서 갈렸다: list={item}, single={single}"
            );
        }
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
