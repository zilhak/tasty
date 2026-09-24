//! 핸들러에서 읽는 id·*_id 키가 라우팅 소스에도 있거나 비대상 사유로 등록됐는지 확인한다.
//! 대상 키가 빠지면 요청이 소유 창 대신 포커스 창으로 갈 수 있다.
//! 메서드마다 의미가 다른 키의 제한 범위는 routing_key_method_scope에서 별도로 확인한다.
//!
//! params.get 또는 params를 첫 인자로 넘긴 호출 형태만 수집한다. 이름이 id 형태가 아닌
//! 새 대상 키와 다른 읽기 문법은 놓칠 수 있다. 라우팅 소스의 문자열 존재를 대조할 뿐 실제 대상 선택을 실행하지 않는다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::repo_root;

const HANDLER_DIR: &str = "src/adapters/ipc/handler";

const HANDLER_ROOT: &str = "src/adapters/ipc/handler.rs";

/// 라우팅 키가 함수 내부 배열·리터럴로 있어 소스에서 읽는다.
const ROUTING_SOURCE: &str = "src/core/request_target.rs";

/// 2026-09-05 핸들러 파일 75개를 측정한 뒤 수집 누락을 찾도록 둔 하한이다.
const MIN_HANDLER_FILES: usize = 50;

/// id 형태이지만 요청의 라우팅 대상이 아닌 키와 그 사유.
pub(super) const NOT_A_ROUTING_TARGET: &[(&str, &str)] = &[
    ("agent_id", "session.* 의 agent 이름(문자열)"),
    ("banner_id", "debug 배너 식별자(문자열)"),
    ("caller_id", "audit 행의 호출자 표기(문자열)"),
    ("extension_id", "plugin extension 식별자(문자열)"),
    ("plan_id", "memory.plan 의 계획 이름(문자열)"),
    ("plugin_id", "plugin 매니페스트 id(문자열)"),
    ("popup_id", "popup contribute id(문자열)"),
    ("requester_id", "approval 요청자 표기(문자열)"),
    ("snapshot_id", "memory blackboard 스냅샷 이름(문자열)"),
    ("step_id", "memory.plan 의 단계 이름(문자열)"),
    ("trace_id", "debug plugin 추적 식별자(문자열)"),
    (
        "caller_surface_id",
        "요청을 보낸 surface다. 라우팅 대상이 아니라 자기 자신 닫기 보호 등에 쓰는 발신자 정보다.",
    ),
    (
        "from_surface_id",
        "message.send 의 발신자. 큐는 받는 쪽(`to_surface_id`)에 매여 있으므로 이쪽으로 \
         라우팅하면 읽는 쪽이 못 본다",
    ),
    (
        "client_id",
        "stream 핸드셰이크가 발급한 클라이언트 연결 식별자 — 창의 리소스가 아니다",
    ),
];

/// 여러 줄 호출도 같은 형태로 찾도록 공백을 제거한다.
fn flatten(src: &str) -> String {
    src.chars().filter(|c| !c.is_whitespace()).collect()
}

fn is_id_shaped(key: &str) -> bool {
    key == "id" || key.ends_with("_id")
}

/// 직접 읽기와 params를 첫 인자로 받는 헬퍼 호출의 키를 수집한다. 스캔 범위 안 헬퍼 정의도 읽는다.
fn id_keys_read_from_params(src: &str) -> BTreeSet<String> {
    let flat = flatten(src);
    let mut out = BTreeSet::new();
    collect_between(&flat, "params.get(\"", "\"", &mut out);
    // 헬퍼 이름 대신 첫 인자의 형태로 찾아 새 헬퍼의 누락을 줄인다.
    collect_between(&flat, "(params,\"", "\"", &mut out);
    out.retain(|k| is_id_shaped(k));
    out
}

fn collect_between(flat: &str, open: &str, close: &str, out: &mut BTreeSet<String>) {
    let mut rest = flat;
    while let Some(at) = rest.find(open) {
        let after = &rest[at + open.len()..];
        match after.find(close) {
            Some(end) => {
                let key = &after[..end];
                if key.chars().all(|c| c.is_ascii_lowercase() || c == '_') && !key.is_empty() {
                    out.insert(key.to_string());
                }
                rest = &after[end..];
            }
            None => break,
        }
    }
}

/// 첫 #[cfg(test)] 앞에서만 읽어 시험의 키 리터럴을 대조 목록에 섞지 않는다. 일반적인 cfg 범위 분석은 아니다.
fn recognised_routing_keys(src: &str) -> BTreeSet<String> {
    let production = super::strip_comments(src.split("#[cfg(test)]").next().unwrap_or(src));
    let mut out = BTreeSet::new();
    let mut rest = production.as_str();
    while let Some(at) = rest.find('"') {
        let after = &rest[at + 1..];
        match after.find('"') {
            Some(end) => {
                let key = &after[..end];
                if key.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                    && !key.is_empty()
                    && is_id_shaped(key)
                {
                    out.insert(key.to_string());
                }
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    out
}

fn handler_sources() -> Vec<PathBuf> {
    let root = repo_root();
    let mut out = Vec::new();
    gather_rs(&root.join(HANDLER_DIR), &mut out);
    out.push(root.join(HANDLER_ROOT));
    out.sort();
    out
}

fn gather_rs(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            gather_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{} 읽기 실패: {e}", path.display()))
}

fn handler_id_keys() -> BTreeMap<String, Vec<String>> {
    let files = handler_sources();
    assert!(
        files.len() >= MIN_HANDLER_FILES,
        "핸들러 파일을 {}개만 수집했다(하한 {MIN_HANDLER_FILES}, 2026-09-05 측정 75개). 경로와 순회 범위를 확인한다.",
        files.len()
    );
    let root = repo_root();
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        for key in id_keys_read_from_params(&read(path)) {
            out.entry(key).or_default().push(rel.clone());
        }
    }
    assert!(
        !out.is_empty(),
        "핸들러의 id 키를 찾지 못했다. 추출 형태를 확인한다."
    );
    out
}

#[test]
fn every_id_key_a_handler_reads_is_routed_or_exempt() {
    let found = handler_id_keys();
    let recognised = recognised_routing_keys(&read(&repo_root().join(ROUTING_SOURCE)));
    assert!(
        !recognised.is_empty(),
        "{ROUTING_SOURCE}에서 라우팅 키를 찾지 못했다. 추출 범위를 확인한다."
    );
    let exempt: BTreeSet<&str> = NOT_A_ROUTING_TARGET.iter().map(|(k, _)| *k).collect();

    let unrouted: Vec<String> = found
        .iter()
        .filter(|(k, _)| !recognised.contains(k.as_str()) && !exempt.contains(k.as_str()))
        .map(|(k, files)| format!("  {k} — {}", files.join(", ")))
        .collect();
    let both: Vec<&str> = exempt
        .iter()
        .copied()
        .filter(|k| recognised.contains(*k))
        .collect();
    assert!(
        both.is_empty(),
        "라우팅과 비대상 예외에 동시에 있는 키다. 실제 용도를 확인해 한쪽 목록에서 제거한다: {both:?}"
    );
    let stale: Vec<&str> = exempt
        .iter()
        .copied()
        .filter(|k| !found.contains_key(*k))
        .collect();

    assert!(
        unrouted.is_empty() && stale.is_empty(),
        "키 분류가 핸들러 읽기와 다르다.\n  라우팅·예외에 없는 키:\n{}\n  예외에만 있는 키: {stale:?}\n창의 리소스 ID면 request_target.rs에 등록하고 필요하면 메서드로 제한한다. 라우팅 대상이 아니면 NOT_A_ROUTING_TARGET에 사유를 적는다.",
        unrouted.join("\n")
    );
}

#[test]
fn the_extractor_sees_param_reads_and_not_lookalikes() {
    let fixture = concat!(
        "let a = params.get(\"surface_id\").and_then(|v| v.as_u64());\n",
        "let b = params\n    .get(\"target_pane_id\")\n    .and_then(|v| v.as_u64());\n",
        "let c = require_u32(params, \"tab_id\", &id);\n",
        "let d = require_str(params, \"plan_id\", &id);\n",
        "let e = resp.get(\"other_id\");\n",
        "let f = params.get(\"kind\").and_then(|v| v.as_str());\n",
    );
    let got: Vec<String> = id_keys_read_from_params(fixture).into_iter().collect();
    assert_eq!(
        got,
        vec![
            "plan_id".to_string(),
            "surface_id".to_string(),
            "tab_id".to_string(),
            "target_pane_id".to_string()
        ],
        "줄바꿈된 params 읽기는 수집하고 다른 값의 get과 id 형태가 아닌 키는 제외해야 한다"
    );
}

#[test]
fn the_recognised_set_ignores_the_fixtures_in_its_own_tests() {
    let fake = concat!(
        "const KEYS: &[&str] = &[\"surface_id\"];\n",
        "#[cfg(test)]\n",
        "mod tests {\n    let p = json!({ \"invented_id\": 1 });\n}\n",
    );
    let got = recognised_routing_keys(fake);
    assert!(got.contains("surface_id"));
    assert!(
        !got.contains("invented_id"),
        "테스트 픽스처의 키가 인식 집합에 섞였다"
    );
}

#[test]
fn the_scan_root_does_not_contain_this_guard() {
    let me = std::path::Path::new(file!());
    assert!(
        !me.starts_with(HANDLER_DIR),
        "이 가드({}) 가 스캔 루트({HANDLER_DIR}) 안에 있다 — 자기 목록을 실제 키로 센다",
        me.display()
    );
}
