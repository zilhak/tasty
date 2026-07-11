//! 인바운드 웹훅 서버 통합/회귀 테스트 (HOOK S16).
//!
//! research.md §5(로컬 실 HTTP 구동) 를 실 바이너리로 집행한다 — 단일 tasty 인스턴스를
//! 띄우고 IPC 로 웹훅을 등록한 뒤 **실 HTTP** 요청을 쏴 ACK·상태변화·lifetime·인증·
//! 남용차단을 관측하고, source 게이트/데이터흐름 분리/단방향 ACK 불변식을 확인한다.
//! CLI→IPC 매핑은 `--port-file` 로 붙인 실 CLI 바이너리로 검증한다.
//!
//! 윈도우 spawn(포커스 도난) 최소화를 위해 **단일 공유 인스턴스**에서 순차 실행한다
//! (기존 `e2e_tests.rs` 설계와 동일). 남용차단은 출처(127.0.0.1) 쿨다운을 유발하므로
//! **가장 마지막**에 둔다.

mod webhook_common;

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use webhook_common::{stdout_str, WebhookInstance};

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
    let id = resp["id"].as_str().expect("register returns id").to_string();
    let url = resp["url"].as_str().expect("register returns url").to_string();
    assert!(url.contains(&id), "url must contain id");
    (id, url)
}

#[test]
#[allow(clippy::cognitive_complexity)] // 단일 공유 인스턴스에서 순차 e2e 스텝 나열(포커스 도난 최소화 설계).
fn webhook_server_integration() {
    let port = webhook_common::free_port();
    let inst = WebhookInstance::builder(port)
        .env("TASTY_WEBHOOK_ABUSE_THRESHOLD", &ABUSE_THRESHOLD.to_string())
        .env("TASTY_WEBHOOK_ABUSE_WINDOW_SECS", "3600")
        .env("TASTY_WEBHOOK_ABUSE_COOLDOWN_SECS", "60")
        .file("hook-handlers.toml", HOOK_HANDLERS_TOML)
        .spawn();
    inst.wait_webhook_ready();

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
            assert_eq!(body, "received", "ACK body must never carry payload/内부 data");
        }
        // 실행 대상이 바뀌지 않았음의 결정적 증거: tasty 는 여전히 살아 IPC 응답한다
        // (페이로드의 system.shutdown 이 실행됐다면 여기서 연결이 끊긴다).
        let sysinfo = inst.call("system.info", json!({}));
        assert!(sysinfo.get("version").is_some(), "tasty must remain alive — payload method must not execute");
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
        let err = resp.get("error").expect("hook-only handler must be rejected");
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
        let resp = inst.call("webhook.register", json!({ "handler": "host/webhook-notify" }));
        let id = resp["id"].as_str().expect("host handler binds").to_string();
        let (code, _body) = inst.post(&id, r#"{"message":"HOSTDEFAULT"}"#);
        assert_eq!(code, 200);
        assert!(wait_notification(&inst, "HOSTDEFAULT", Duration::from_secs(8)));
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
        assert!(!has_notification(&inst, "NOAUTH"), "unauthorized must not execute");

        // 올바른 토큰(쿼리) → 200 + 실행.
        let path = format!("{id}?tok=s3cr3t");
        let (code, _body) = inst.post(&path, r#"{"message":"AUTHED"}"#);
        assert_eq!(code, 200, "correct token must pass");
        assert!(wait_notification(&inst, "AUTHED", Duration::from_secs(8)));

        // info/list 응답은 위치/키만 노출하고 토큰은 절대 싣지 않는다.
        let info = inst.call("webhook.info", json!({ "id": id }));
        let info_str = serde_json::to_string(&info).unwrap();
        assert!(!info_str.contains("s3cr3t"), "token must never leak in info");
        inst.call("webhook.unregister", json!({ "id": id }));
    }

    // ========== 9) unregister → path 회수 → 404 ==========
    {
        let (id, _url) = register_notify_webhook(&inst, json!({}));
        assert_eq!(inst.post(&id, r#"{"message":"live"}"#).0, 200);
        let removed = inst.call("webhook.unregister", json!({ "id": id }));
        assert_eq!(removed["unregistered"].as_bool(), Some(true));
        assert_eq!(inst.post(&id, r#"{"message":"dead"}"#).0, 404, "unregistered path must 404");
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
        assert!(out.status.success(), "cli register must succeed: {}", webhook_common::stderr_str(&out));
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
        assert!(wait_notification(&inst, "CLI_MARKER", Duration::from_secs(8)));

        // unregister via CLI.
        let out = inst.cli(&["webhook", "unregister", "--id", &cli_id]);
        assert!(out.status.success());
        assert_eq!(inst.post(&cli_id, r#"{}"#).0, 404);
    }

    // ========== 11) hook_handler.* — list + dispatch (실 CLI) ==========
    {
        // list 는 host default(webhook-notify) + user(hookonly/shelltest)를 포함.
        let out = inst.cli(&["hook-handler", "list"]);
        assert!(out.status.success(), "hook-handler list: {}", webhook_common::stderr_str(&out));
        let list: Value = serde_json::from_str(&stdout_str(&out)).expect("hook-handler list JSON");
        let ids: Vec<String> = list["handlers"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|h| h["id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        assert!(ids.iter().any(|i| i == "host/webhook-notify"), "host default handler must be listed");
        assert!(ids.iter().any(|i| i == "user/hookonly"), "user handler must be merged in");

        // dispatch(수동 발화) → hookonly 가 IpcSequence 를 실행해 알림 생성.
        let out = inst.cli(&["hook-handler", "dispatch", "--id", "user/hookonly"]);
        assert!(out.status.success(), "dispatch: {}", webhook_common::stderr_str(&out));
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
        assert!(saw_429, "repeated 404s from one source must trip the abuse cooldown (429)");
    }
}
