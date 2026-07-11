//! 웹훅 재시작 복원 회귀 테스트 (HOOK S16, research §5.3 lifetime 6종 중 영속성 축).
//!
//! `Persistent` 웹훅은 `~/.tasty/webhooks.toml` 에 발급 id·시퀀스·lifetime 을 저장해
//! 재시작 후 **같은 URL** 로 복원되고, `Temporary` 웹훅은 재시작 시 소멸한다. 이를
//! 두 tasty 인스턴스가 **같은 TASTY_HOME 을 공유**하게 띄워 실 HTTP 로 검증한다.

mod webhook_common;

use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;
use webhook_common::WebhookInstance;

fn unique_home() -> PathBuf {
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    std::env::temp_dir().join(format!("tasty-wh-restart-home-{unique}"))
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

#[test]
fn persistent_webhook_survives_restart_temporary_does_not() {
    let port = webhook_common::free_port();
    let home = unique_home();

    let (persistent_id, persistent_url, temp_id);

    // ── 1차 인스턴스: 영속 + 임시 웹훅 등록 ──
    {
        let inst = WebhookInstance::builder(port).home(home.clone()).spawn();
        inst.wait_webhook_ready();

        persistent_id = register_persistent(&inst, true);
        temp_id = register_persistent(&inst, false);
        persistent_url = inst.call("webhook.info", json!({ "id": persistent_id }))["url"]
            .as_str()
            .unwrap()
            .to_string();

        // 둘 다 실 HTTP 로 동작.
        assert_eq!(inst.post(&persistent_id, r#"{"message":"a"}"#).0, 200);
        assert_eq!(inst.post(&temp_id, r#"{"message":"b"}"#).0, 200);
        // 인스턴스 Drop → shutdown → 프로세스 종료(홈은 유지: own_home=false).
    }

    // 포트 해제 여유.
    std::thread::sleep(Duration::from_millis(500));

    // ── 2차 인스턴스: 같은 TASTY_HOME + 같은 포트로 재시작 ──
    {
        let inst2 = WebhookInstance::builder(port).home(home.clone()).spawn();
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
