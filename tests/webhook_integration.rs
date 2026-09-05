//! 인바운드 웹훅 서버 통합/회귀 테스트 (HOOK S16).
//!
//! research.md §5(로컬 실 HTTP 구동) 를 실 바이너리로 집행한다 — 단일 tasty 인스턴스를
//! 띄우고 IPC 로 웹훅을 등록한 뒤 **실 HTTP** 요청을 쏴 ACK·상태변화·lifetime·인증·
//! 남용차단을 관측하고, source 게이트/데이터흐름 분리/단방향 ACK 불변식을 확인한다.
//! CLI→IPC 매핑은 `--port-file` 로 붙인 실 CLI 바이너리로 검증한다.
//!
//! 윈도우 spawn(포커스 도난) 최소화를 위해 **단일 공유 인스턴스**에서 순차 실행한다
//! (기존 `e2e_tests.rs` 설계와 동일). 남용차단은 출처(127.0.0.1) 쿨다운을 유발하므로
//! integration 흐름의 **가장 마지막**에 둔다.
//!
//! 본 바이너리는 웹훅 패밀리 전체를 담는다 (테스트 다이어트 — spawn 4회→2회):
//! 1. restart 선등록: 영속/임시 웹훅 등록 + 실 HTTP 200 (abuse 쿨다운 **이전**이어야 함)
//! 2. integration 흐름: 기존 12 스텝 (abuse 마지막)
//! 3. hook env 흐름: 훅/디스패치 env 전파 — IPC 전용이라 쿨다운 무관. 핸들러는
//!    spawn-시 파일이 아니라 재작성 + `hook_handler.reload` 로 주입한다
//! 4. 재시작: 같은 TASTY_HOME/포트로 2차 인스턴스 → 영속 복원/임시 소멸 검증
//!    (새 프로세스라 abuse 쿨다운은 소멸 — in-memory)

// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다 — 전수 가드
// (`tests/let_underscore_documented.rs`)가 테스트 본문을 제외하므로, 여기서 나는
// `let_underscore_must_use` 경고는 정책상 조치 대상이 될 수 없다. 끄지 않으면
// 프로덕션의 진짜 신호가 그 안에 묻힌다 — `docs/dev-guide/error-handling.md`.
#![allow(clippy::let_underscore_must_use)]

mod marker_wait;
mod webhook_common;

use std::time::{Duration, Instant};

use marker_wait::wait_file_content;

use serde_json::{Value, json};
use webhook_common::{WebhookInstance, stdout_str};

/// 남용차단 임계치를 40 으로, 윈도우/쿨다운을 넉넉히 잡아 결정적으로 만든다. 이전
/// 스텝들이 만드는 소수의 404/405 실패로는 트립되지 않고(≪40), 마지막 남용 테스트만
/// 고의로 40+ 실패를 몰아 트립시킨다.
const ABUSE_THRESHOLD: u32 = 40;

/// 사용자 훅 핸들러 — source 게이트 테스트용. hook 전용 IpcSequence 와 hook 전용 셸
/// 핸들러 둘 다 웹훅 바인딩이 거부돼야 한다.
const HOOK_HANDLERS_TOML: &str = r#"
[[handler]]
id = "user/hookonly"
source = "hook"
priority = 50
[handler.action]
kind = "ipc_sequence"
calls = [{ method = "notification.create", params = { title = "HookOnly", body = "dispatched" } }]

[[handler]]
id = "user/shelltest"
source = "hook"
priority = 50
[handler.action]
kind = "shell_command"
command = "echo"
args = ["hi"]
"#;

/// notification.list 의 어떤 알림 body 가 `needle` 를 **포함**하는가(대기 없이 즉시).
///
/// tasty 는 연속 알림을 하나로 병합(coalesce)해 body 를 개행 결합하므로 exact-match
/// 가 아니라 substring 으로 관측한다.
fn has_notification(inst: &WebhookInstance, needle: &str) -> bool {
    let notifs = inst.call("notification.list", json!({}));
    notifs
        .as_array()
        .map(|arr| {
            arr.iter().any(|n| {
                n.get("body")
                    .and_then(|b| b.as_str())
                    .map(|b| b.contains(needle))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// 어떤 알림 body 가 `needle` 를 포함할 때까지 대기.
fn wait_notification(inst: &WebhookInstance, needle: &str, timeout: Duration) -> bool {
    let start = Instant::now();
    loop {
        if has_notification(inst, needle) {
            return true;
        }
        if start.elapsed() > timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// 인라인 시퀀스(notification.create, body=${body.message})로 웹훅을 등록하고
/// (id, url path) 를 반환.
fn register_notify_webhook(inst: &WebhookInstance, params_extra: Value) -> (String, String) {
    let mut params = json!({
        "methods": ["POST"],
        "sequence": [{
            "method": "notification.create",
            "params": { "title": "WH", "body": "${body.message}" }
        }]
    });
    // params_extra 의 키를 병합(lifetime/auth 등).
    if let (Some(obj), Some(extra)) = (params.as_object_mut(), params_extra.as_object()) {
        for (k, v) in extra {
            obj.insert(k.clone(), v.clone());
        }
    }
    let resp = inst.call("webhook.register", params);
    let id = resp["id"]
        .as_str()
        .expect("register returns id")
        .to_string();
    let url = resp["url"]
        .as_str()
        .expect("register returns url")
        .to_string();
    assert!(url.contains(&id), "url must contain id");
    (id, url)
}

#[allow(clippy::cognitive_complexity)] // complexity-exempt: 단일 공유 인스턴스에서 순차 e2e 스텝 나열(포커스 도난 최소화 설계) — 웹훅 재시작 시나리오는 상태를 물려받아야 해 갈 수 없다(docs/dev-guide/e2e-tests.md §1-1).
fn integration_flow(inst: &WebhookInstance) {
    // ========== 1) 등록 → 실 HTTP POST → ACK + 상태변화 + 페이로드 치환 ==========
    {
        let (id, _url) = register_notify_webhook(&inst, json!({}));
        // 등록 직후 조회 가능(포커스 독립, id 지정).
        let info = inst.call("webhook.info", json!({ "id": id }));
        assert_eq!(info["id"].as_str(), Some(id.as_str()));
        assert_eq!(info["methods"], json!(["POST"]));

        let (code, body) = inst.post(&id, r#"{"message":"MARKER_HELLO"}"#);
        assert_eq!(code, 200, "POST to registered webhook must ACK 200");
        // 단방향 ACK: 바디는 고정 문자열("received"), 실행 결과 미포함.
        assert_eq!(body, "received", "ACK body must be the fixed string");

        // IpcSequence 가 fire-and-forget 로 실행돼 notification 이 생기고, body 는
        // 페이로드 `message` 로 치환됐다.
        assert!(
            wait_notification(&inst, "MARKER_HELLO", Duration::from_secs(8)),
            "webhook IpcSequence must create a notification with the substituted body"
        );
        inst.call("webhook.unregister", json!({ "id": id }));
    }

    // ========== 2) 단방향 ACK 불변식 — 어떤 페이로드도 응답 바디를 못 바꾼다 ==========
    // ========== + 데이터/흐름 분리 — 페이로드의 method-like 값이 실행 대상을 못 바꾼다 ==========
    {
        let (id, _url) = register_notify_webhook(&inst, json!({}));

        // 페이로드에 method/internal-looking 필드를 심어도: ① ACK 바디는 고정,
        // ② 실행 method 는 owner 고정(notification.create) — system.shutdown 이 아님.
        let payloads = [
            r#"{"message":"P1","method":"system.shutdown"}"#,
            r#"{"message":"P2","params":{"method":"tab.close"}}"#,
            r#"{"message":"P3","body":{"nested":"x"}}"#,
        ];
        for p in payloads {
            let (code, body) = inst.post(&id, p);
            assert_eq!(code, 200, "payload {p} must still ACK 200");
            assert_eq!(
                body, "received",
                "ACK body must never carry payload/内부 data"
            );
        }
        // 실행 대상이 바뀌지 않았음의 결정적 증거: tasty 는 여전히 살아 IPC 응답한다
        // (페이로드의 system.shutdown 이 실행됐다면 여기서 연결이 끊긴다).
        let sysinfo = inst.call("system.info", json!({}));
        assert!(
            sysinfo.get("version").is_some(),
            "tasty must remain alive — payload method must not execute"
        );
        // 그리고 각 message 는 notification.create(고정 method)로 실행돼 알림이 생긴다.
        assert!(wait_notification(&inst, "P1", Duration::from_secs(8)));
        inst.call("webhook.unregister", json!({ "id": id }));
    }

    // ========== 3) 메서드 불일치 → 405, 카운트 미소모 ==========
    {
        let (id, _url) = register_notify_webhook(&inst, json!({}));
        let (code, body) = inst.http("GET", &id, "");
        assert_eq!(code, 405, "wrong method must be 405");
        assert_eq!(body, "method not allowed");
        inst.call("webhook.unregister", json!({ "id": id }));
    }

    // ========== 4) lifetime — 횟수제한 N → N 회 성공, N+1 소멸(404) ==========
    {
        let (id, _url) = register_notify_webhook(&inst, json!({ "count": 2 }));
        assert_eq!(inst.post(&id, r#"{"message":"C1"}"#).0, 200);
        // info 로 남은 카운트 확인.
        let info = inst.call("webhook.info", json!({ "id": id }));
        assert_eq!(info["lifetime"]["remaining"].as_u64(), Some(1));
        assert_eq!(inst.post(&id, r#"{"message":"C2"}"#).0, 200);
        // 소진 → 삭제 → 3번째는 404.
        assert_eq!(inst.post(&id, r#"{"message":"C3"}"#).0, 404);
        assert!(
            inst.call_raw("webhook.info", json!({ "id": id }))
                .get("error")
                .is_some(),
            "exhausted webhook must be gone from registry"
        );
    }

    // ========== 5) lifetime — 시간제한 만료 → 410 Gone + 삭제 ==========
    {
        let (id, _url) = register_notify_webhook(&inst, json!({ "ttl_secs": 1 }));
        std::thread::sleep(Duration::from_secs(2));
        let (code, body) = inst.post(&id, r#"{"message":"expired"}"#);
        assert_eq!(code, 410, "expired time-limited webhook must be 410 Gone");
        assert_eq!(body, "gone");
        // 만료 응답과 함께 삭제 → 이후 404.
        assert_eq!(inst.post(&id, r#"{"message":"again"}"#).0, 404);
    }

    // ========== 6) source 게이트 — hook 전용 핸들러는 웹훅 바인딩 거부(invalid_params) ==========
    {
        // hook 전용 IpcSequence.
        let resp = inst.call_raw("webhook.register", json!({ "handler": "user/hookonly" }));
        let err = resp
            .get("error")
            .expect("hook-only handler must be rejected");
        let msg = err["message"].as_str().unwrap_or("").to_lowercase();
        assert!(
            msg.contains("source") || msg.contains("hook"),
            "rejection must cite source/hook gate, got: {msg}"
        );

        // hook 전용 셸 핸들러(셸 웹훅 거부 불변식).
        let resp = inst.call_raw("webhook.register", json!({ "handler": "user/shelltest" }));
        assert!(
            resp.get("error").is_some(),
            "shell handler must never be webhook-bindable"
        );
    }

    // ========== 7) host default 핸들러(source=webhook)는 바인딩 성공 ==========
    {
        let resp = inst.call(
            "webhook.register",
            json!({ "handler": "host/webhook-notify" }),
        );
        let id = resp["id"].as_str().expect("host handler binds").to_string();
        let (code, _body) = inst.post(&id, r#"{"message":"HOSTDEFAULT"}"#);
        assert_eq!(code, 200);
        assert!(wait_notification(
            &inst,
            "HOSTDEFAULT",
            Duration::from_secs(8)
        ));
        inst.call("webhook.unregister", json!({ "id": id }));
    }

    // ========== 8) 선택적 인증 — 미제시 401, 일치 200, 무영향 정상흐름 ==========
    {
        let (id, _url) = register_notify_webhook(
            &inst,
            json!({ "auth": { "location": "query", "key": "tok", "token": "s3cr3t" } }),
        );
        // 토큰 미제시 → 401.
        let (code, body) = inst.post(&id, r#"{"message":"NOAUTH"}"#);
        assert_eq!(code, 401, "missing token must be 401");
        assert_eq!(body, "unauthorized");
        assert!(
            !has_notification(&inst, "NOAUTH"),
            "unauthorized must not execute"
        );

        // 올바른 토큰(쿼리) → 200 + 실행.
        let path = format!("{id}?tok=s3cr3t");
        let (code, _body) = inst.post(&path, r#"{"message":"AUTHED"}"#);
        assert_eq!(code, 200, "correct token must pass");
        assert!(wait_notification(&inst, "AUTHED", Duration::from_secs(8)));

        // info/list 응답은 위치/키만 노출하고 토큰은 절대 싣지 않는다.
        let info = inst.call("webhook.info", json!({ "id": id }));
        let info_str = serde_json::to_string(&info).unwrap();
        assert!(
            !info_str.contains("s3cr3t"),
            "token must never leak in info"
        );
        inst.call("webhook.unregister", json!({ "id": id }));
    }

    // ========== 9) unregister → path 회수 → 404 ==========
    {
        let (id, _url) = register_notify_webhook(&inst, json!({}));
        assert_eq!(inst.post(&id, r#"{"message":"live"}"#).0, 200);
        let removed = inst.call("webhook.unregister", json!({ "id": id }));
        assert_eq!(removed["unregistered"].as_bool(), Some(true));
        assert_eq!(
            inst.post(&id, r#"{"message":"dead"}"#).0,
            404,
            "unregistered path must 404"
        );
    }

    // ========== 10) CLI→IPC 매핑 — 실 CLI 바이너리로 register/list/info/unregister ==========
    {
        // register (inline sequence via CLI).
        let out = inst.cli(&[
            "webhook",
            "register",
            "--method",
            "POST",
            "--sequence",
            r#"[{"method":"notification.create","params":{"body":"CLI_MARKER"}}]"#,
        ]);
        assert!(
            out.status.success(),
            "cli register must succeed: {}",
            webhook_common::stderr_str(&out)
        );
        let reg: Value = serde_json::from_str(&stdout_str(&out)).expect("cli register prints JSON");
        let cli_id = reg["id"].as_str().expect("cli register id").to_string();

        // list 는 방금 등록한 id 를 포함(전 범위 조회).
        let out = inst.cli(&["webhook", "list"]);
        assert!(out.status.success());
        let list: Value = serde_json::from_str(&stdout_str(&out)).expect("cli list JSON");
        let found = list["webhooks"]
            .as_array()
            .map(|a| a.iter().any(|w| w["id"].as_str() == Some(cli_id.as_str())))
            .unwrap_or(false);
        assert!(found, "cli list must include the registered webhook");

        // info by id.
        let out = inst.cli(&["webhook", "info", "--id", &cli_id]);
        assert!(out.status.success());
        let info: Value = serde_json::from_str(&stdout_str(&out)).expect("cli info JSON");
        assert_eq!(info["id"].as_str(), Some(cli_id.as_str()));

        // 실제 HTTP 로도 CLI 등록 웹훅이 동작.
        let (code, _b) = inst.post(&cli_id, r#"{}"#);
        assert_eq!(code, 200);
        assert!(wait_notification(
            &inst,
            "CLI_MARKER",
            Duration::from_secs(8)
        ));

        // unregister via CLI.
        let out = inst.cli(&["webhook", "unregister", "--id", &cli_id]);
        assert!(out.status.success());
        assert_eq!(inst.post(&cli_id, r#"{}"#).0, 404);
    }

    // ========== 11) hook_handler.* — list + dispatch (실 CLI) ==========
    {
        // list 는 host default(webhook-notify) + user(hookonly/shelltest)를 포함.
        let out = inst.cli(&["hook-handler", "list"]);
        assert!(
            out.status.success(),
            "hook-handler list: {}",
            webhook_common::stderr_str(&out)
        );
        let list: Value = serde_json::from_str(&stdout_str(&out)).expect("hook-handler list JSON");
        let ids: Vec<String> = list["handlers"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|h| h["id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        assert!(
            ids.iter().any(|i| i == "host/webhook-notify"),
            "host default handler must be listed"
        );
        assert!(
            ids.iter().any(|i| i == "user/hookonly"),
            "user handler must be merged in"
        );

        // dispatch(수동 발화) → hookonly 가 IpcSequence 를 실행해 알림 생성.
        let out = inst.cli(&["hook-handler", "dispatch", "--id", "user/hookonly"]);
        assert!(
            out.status.success(),
            "dispatch: {}",
            webhook_common::stderr_str(&out)
        );
        assert!(
            wait_notification(&inst, "dispatched", Duration::from_secs(8)),
            "dispatched hook handler must execute its IpcSequence"
        );
    }

    // ========== 12) (마지막) 남용 차단 — 반복 404 임계치 초과 → 429 ==========
    {
        // 없는 path 로 반복 요청 → 임계치 초과 시 쿨다운 즉시거부(429). 정상 웹훅은
        // 별개(정상 매칭은 실패 집계 대상 아님)지만, 쿨다운은 출처(IP) 단위라 이
        // 스텝을 가장 마지막에 둔다.
        let mut saw_429 = false;
        for i in 0..(ABUSE_THRESHOLD + 25) {
            let (code, _b) = inst.post("no-such-webhook-abuse", "");
            if code == 429 {
                saw_429 = true;
                break;
            }
            assert_eq!(code, 404, "pre-cooldown misses must be 404 (req {i})");
        }
        assert!(
            saw_429,
            "repeated 404s from one source must trip the abuse cooldown (429)"
        );
    }
}

// ───────────────────────── hook env 흐름 (구 hook_env_integration.rs 이관) ─────────────────────────

/// hook env 검증용 핸들러 TOML + 마커 경로를 spawn **전에** 만든다.
/// (레지스트리가 user 설정을 자체 영속하므로 spawn 후 파일 재작성 + reload 는
/// 디바운스 저장과 레이스한다 — spawn-시 주입이 결정적.)
struct HookEnvSetup {
    handlers_toml: String,
    hook_marker: std::path::PathBuf,
    dispatch_marker: std::path::PathBuf,
}

fn hook_env_setup() -> HookEnvSetup {
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let hook_marker = std::env::temp_dir().join(format!("tasty-hookenv-{unique}.txt"));
    let dispatch_marker = std::env::temp_dir().join(format!("tasty-dispenv-{unique}.txt"));

    // cmd 함정: `>` 직전 문자가 숫자면 fd 리다이렉트로 파싱되므로(`42> f` = stderr)
    // 리다이렉트 앞에 공백을 두고 결과 trailing space 는 trim 으로 흡수한다.
    let (hook_cmd, dispatch_shell, dispatch_flag, dispatch_line) = if cfg!(windows) {
        (
            format!(
                "echo %TASTY_HOOK_EVENT%/%TASTY_HOOK_SOURCE%/%TASTY_HOOK_SURFACE_ID% > {}",
                hook_marker.display()
            ),
            "cmd",
            "/C",
            format!(
                "echo %TASTY_HOOK_EVENT%/%TASTY_HOOK_SOURCE%/%TASTY_HOOK_REPO% > {}",
                dispatch_marker.display()
            ),
        )
    } else {
        (
            format!(
                "echo \"$TASTY_HOOK_EVENT/$TASTY_HOOK_SOURCE/$TASTY_HOOK_SURFACE_ID\" > {}",
                hook_marker.display()
            ),
            "sh",
            "-c",
            format!(
                "echo \"$TASTY_HOOK_EVENT/$TASTY_HOOK_SOURCE/$TASTY_HOOK_REPO\" > {}",
                dispatch_marker.display()
            ),
        )
    };

    let handlers_toml = format!(
        r#"
[[handler]]
id = "user/envhook"
source = "hook"
priority = 50
[handler.action]
kind = "shell_command"
command = '{hook_cmd}'

[[handler]]
id = "user/envdispatch"
source = "hook"
priority = 50
[handler.action]
kind = "shell_command"
command = "{dispatch_shell}"
args = ["{dispatch_flag}", '{dispatch_line}']
"#
    );
    HookEnvSetup {
        handlers_toml,
        hook_marker,
        dispatch_marker,
    }
}

/// 훅/디스패치 셸 핸들러가 TASTY_HOOK_* env 를 받는지 검증한다. HTTP 를 쓰지
/// 않으므로(순수 IPC + 셸) integration 흐름의 abuse 쿨다운과 무관하다.
fn hook_env_flow(inst: &WebhookInstance, setup: &HookEnvSetup) {
    let hook_marker = &setup.hook_marker;
    let dispatch_marker = &setup.dispatch_marker;
    let sid = inst.first_surface_id();

    // ── 1. hook 트리거 경로 ──────────────────────────────────────────────
    inst.call(
        "hook.set",
        json!({ "surface_id": sid, "event": "bell", "handler": "user/envhook" }),
    );
    let fired = inst.call(
        "surface.fire_hook",
        json!({ "surface_id": sid, "event": "bell" }),
    );
    assert_eq!(fired["fired"].as_u64(), Some(1), "hook should fire once");
    let content = wait_file_content(hook_marker, Duration::from_secs(10));
    assert_eq!(content, format!("bell/hook/{sid}"));

    // ── 2. hook_handler.dispatch 수동 발화 (payload env) ────────────────
    let ack = inst.call(
        "hook_handler.dispatch",
        json!({ "id": "user/envdispatch", "body": { "repo": "tasty" } }),
    );
    assert_eq!(ack["accepted"].as_bool(), Some(true));
    let content = wait_file_content(dispatch_marker, Duration::from_secs(10));
    assert_eq!(content, "user/envdispatch/dispatch/tasty");

    // best-effort 정리 — temp 마커 잔류는 무해.
    std::fs::remove_file(hook_marker).ok();
    std::fs::remove_file(dispatch_marker).ok();
}

// ───────────────────────── 재시작 복원 흐름 (구 webhook_restart.rs 이관) ─────────────────────────

fn unique_home() -> std::path::PathBuf {
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    std::env::temp_dir().join(format!("tasty-wh-family-home-{unique}"))
}

fn register_persistent(inst: &WebhookInstance, persistent: bool) -> String {
    let resp = inst.call(
        "webhook.register",
        json!({
            "methods": ["POST"],
            "persistent": persistent,
            "sequence": [{
                "method": "notification.create",
                "params": { "body": "${body.message}" }
            }]
        }),
    );
    resp["id"].as_str().expect("register id").to_string()
}

fn list_ids(inst: &WebhookInstance) -> Vec<String> {
    inst.call("webhook.list", json!({}))["webhooks"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|w| w["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

// ───────────────────────── 오케스트레이션 (spawn 2회) ─────────────────────────

/// 웹훅 패밀리 전체 — 단일 1차 인스턴스에서 [restart 선등록 → integration 12스텝 →
/// hook env] 를 순차 실행하고, 같은 홈/포트로 2차 인스턴스를 띄워 재시작 복원을
/// 검증한다. 순서 불변식:
/// - restart 선등록의 실 HTTP 200 확인은 abuse 쿨다운(integration 마지막) **이전**
/// - hook env 흐름은 HTTP 무사용이라 쿨다운 이후 안전
/// - 2차 인스턴스는 새 프로세스라 쿨다운(in-memory) 이 소멸
#[test]
fn webhook_family() {
    // 예약을 붙든 채 빌더로 넘긴다 — 번호만 빼내 버리면 그 순간부터 spawn 까지
    // 아무도 이 포트를 지키지 않는다(TOCTOU).
    let lease = webhook_common::free_port();
    let home = unique_home();
    let hook_env = hook_env_setup();
    // integration 용 정적 핸들러 + hook env 용 동적 핸들러를 한 파일로 결합 주입.
    let combined_handlers = format!(
        "{HOOK_HANDLERS_TOML}
{}",
        hook_env.handlers_toml
    );
    let (persistent_id, persistent_url, temp_id);

    // ── 1차 인스턴스 ──
    {
        let inst = WebhookInstance::builder(lease)
            .home(home.clone())
            .env(
                "TASTY_WEBHOOK_ABUSE_THRESHOLD",
                &ABUSE_THRESHOLD.to_string(),
            )
            .env("TASTY_WEBHOOK_ABUSE_WINDOW_SECS", "3600")
            .env("TASTY_WEBHOOK_ABUSE_COOLDOWN_SECS", "60")
            .file("hook-handlers.toml", &combined_handlers)
            .spawn();
        inst.wait_webhook_ready();
        // 핸들러 레지스트리의 user 파일 lazy-load 를 **선행**시킨다 — 최초 접근이
        // 쓰기(webhook.register 의 핸들러 생성)면 파일 병합이 건너뛰어져
        // hook-handlers.toml 의 핸들러들이 유실된다 (읽기 1회로 결정화).

        // restart 선등록 (+ 쿨다운 전 실 HTTP 동작 확인)
        persistent_id = register_persistent(&inst, true);
        temp_id = register_persistent(&inst, false);
        persistent_url = inst.call("webhook.info", json!({ "id": persistent_id }))["url"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(inst.post(&persistent_id, r#"{"message":"a"}"#).0, 200);
        assert_eq!(inst.post(&temp_id, r#"{"message":"b"}"#).0, 200);

        integration_flow(&inst);
        hook_env_flow(&inst, &hook_env);
        // 인스턴스 Drop → shutdown → 프로세스 종료(홈은 유지: own_home=false).
    }

    // 포트 해제 여유.
    std::thread::sleep(Duration::from_millis(500));

    // ── 2차 인스턴스: 같은 TASTY_HOME + 같은 포트로 재시작 ──
    {
        // 같은 홈의 webhooks.toml 에 박힌 포트를 그대로 쓴다(URL 고정 검증이 목적이라
        // 하네스가 번호를 바꿀 수 없다) — 그래서 예약도 재시도도 없는 전용 진입점이다.
        let inst2 = WebhookInstance::builder_for_restart()
            .home(home.clone())
            .spawn();
        inst2.wait_webhook_ready();

        let ids = list_ids(&inst2);
        assert!(
            ids.iter().any(|i| i == &persistent_id),
            "persistent webhook must be restored after restart (ids: {ids:?})"
        );
        assert!(
            !ids.iter().any(|i| i == &temp_id),
            "temporary webhook must NOT survive restart"
        );

        // 같은 URL 로 복원.
        let restored_url = inst2.call("webhook.info", json!({ "id": persistent_id }))["url"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(
            restored_url, persistent_url,
            "restored webhook must keep the same URL (stable across restart)"
        );

        // 실 HTTP: 영속은 여전히 200, 임시는 404.
        assert_eq!(
            inst2.post(&persistent_id, r#"{"message":"c"}"#).0,
            200,
            "restored persistent webhook must serve requests"
        );
        assert_eq!(
            inst2.post(&temp_id, r#"{"message":"d"}"#).0,
            404,
            "temporary webhook path must be gone (404)"
        );
    }

    // 공유 홈 수동 정리.
    let _ = std::fs::remove_dir_all(&home);
}
