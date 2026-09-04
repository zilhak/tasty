//! IPC 핸들러가 **대상으로 읽는 id 키**가 라우팅에서 인식되는가.
//!
//! `src/app/ipc/routing.rs` 는 요청의 주인 창을 못 찾으면 **포커스된 창**으로 보낸다.
//! 그래서 핸들러가 새 키로 대상을 받기 시작하면, 그 키를
//! `src/app/request_owner.rs` 가 모르는 동안 그 메서드는 조용히 포커스 의존이 된다 —
//! 에러가 아니라 "다른 창에서 not found" 로 나타나므로 원인이 라우팅이라는 것이
//! 드러나지 않는다. `docs/design/policies/focus.md` 의 "silent fallback 금지" 가
//! 막으려는 형태다.
//!
//! ## 왜 목록이 아니라 가드인가
//!
//! 이 인식 목록은 주석에 "이 키들이 다른 의미로 쓰이는 곳이 없음을 **확인했다**" 고
//! 적고 있었다. 그 확인은 일회성이었고 재검증 채널이 없어, 그 뒤에 들어온 키
//! (`tab_id` · `to_surface_id` · `target_pane_id` · `target_workspace_id` · `hook_id` ·
//! `observer_id` · `source_id`)가 전부 빠져 있었다. 키를 손으로 더하는 것으로는
//! 다음 누락을 막지 못한다.
//!
//! ## 모수 고정
//!
//! [ADR-0133](../../docs/adr/0133-guard-scan-population-is-pinned-not-enumerated.md) ③
//! 대로 **집합 동등**이다 — 핸들러가 읽는 id 키 집합에서 인식 키를 뺀 나머지가
//! [`NOT_A_ROUTING_TARGET`] 과 정확히 같아야 한다. 양방향이라 새 키가 들어오는 것과
//! 면제가 stale 이 되는 것을 둘 다 잡는다.
//!
//! id **모양**(`_id` 로 끝나거나 단독 `id`)인 키만 본다. `target` · `parent` · `surface`
//! 처럼 모양이 아닌 대상 키는 모수 밖이다 — 셋 다 이미 인식 목록에 있고, 모양으로
//! 좁히지 않으면 면제 목록이 핸들러의 **모든 파라미터**를 담게 되어 검토받을 수 있는
//! 크기를 넘는다. 새 대상 키를 모양 없이 지으면 이 가드가 못 잡는다.
//!
//! 파일 수 하한은 **연기 검사** 용도로만 둔다
//! (경로가 틀리면 예외가 아니라 조용한 0 이 되고, 0 인 모수는 집합 동등을 공짜로
//! 통과시킨다 — 양쪽이 빈 집합이 되기 때문이다).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::repo_root;

/// 핸들러 소스 트리. 개별 파일이 아니라 디렉터리다(ADR-0133 ①).
const HANDLER_DIR: &str = "src/adapters/ipc/handler";

/// 그 디렉터리와 짝인 모듈 루트. 공용 `require_*` 헬퍼가 여기 있다.
const HANDLER_ROOT: &str = "src/adapters/ipc/handler.rs";

/// 인식 목록이 사는 곳. 상수를 직접 못 쓰는 이유는 그 모듈이 `gui` feature 로
/// 게이트돼 있고 이 가드는 두 조합에서 다 돌기 때문이다 — 소스를 텍스트로 읽는다.
const ROUTING_SOURCE: &str = "src/app/request_owner.rs";

/// 스캔한 핸들러 `.rs` 파일 수의 하한 — **연기 검사**다.
/// 값의 근거: 2026-09-05 실측 **75 개**.
const MIN_HANDLER_FILES: usize = 50;

/// id 처럼 생겼지만 **라우팅 대상이 아닌** 키. 각 항목에 왜 아닌지를 적는다.
///
/// 면제는 목록에 남아 검토받고 미등재는 안 남는다 — 그래서 "모르는 키" 를 조용히
/// 넘기지 않고 여기에 사유와 함께 적게 한다.
const NOT_A_ROUTING_TARGET: &[(&str, &str)] = &[
    // ── 문자열 id. 라우팅은 숫자만 본다(`as_u64`) ─────────────────────────────
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
    // ── 숫자지만 **대상이 아니다** ────────────────────────────────────────────
    (
        "caller_surface_id",
        "요청을 **보낸** surface. 대상이 아니라 발신자이고, 자기 자신 닫기 보호 같은 \
         판정에 쓰인다",
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

/// 공백을 없애 호출 형태를 한 줄로 만든다.
///
/// 줄바꿈뿐 아니라 구두점 주변 간격까지 없애는 이유: 실제 코드가
/// `params\n    .get("target_pane_id")` 처럼 줄을 나눠 쓰기도 하고 한 줄로 붙이기도 한다.
/// 두 형태가 같은 문자열이 되어야 마커 하나로 잡힌다 — 못 본 키는 위반이 아니라
/// **침묵**이라, 형태 하나를 놓치면 가드가 초록인 채로 비어 간다.
fn flatten(src: &str) -> String {
    src.chars().filter(|c| !c.is_whitespace()).collect()
}

/// id 처럼 생긴 키인가 — `_id` 로 끝나거나 단독 `id`.
///
/// `child` 처럼 `_id` 로 안 끝나는 것은 대상이 아니라 **인덱스**다(그 판정은
/// `app::request_owner` 의 `child_index_key_is_not_recognized` 가 든다).
fn is_id_shaped(key: &str) -> bool {
    key == "id" || key.ends_with("_id")
}

/// 한 소스가 `params` 에서 읽는 id 키.
///
/// `surface_id` 처럼 전용 헬퍼(`require_surface_id`)가 키를 안에 박아 둔 경우도 잡힌다 —
/// 그 헬퍼의 **본체**가 이 스캔 루트 안에 있고 거기서 `params.get("surface_id")` 를 하기
/// 때문이다. 그래서 호출부만으로는 안 보이는 키도 정의부에서 드러난다.
fn id_keys_read_from_params(src: &str) -> BTreeSet<String> {
    let flat = flatten(src);
    let mut out = BTreeSet::new();
    // 직접 읽기: `params.get("surface_id")`.
    collect_between(&flat, "params.get(\"", "\"", &mut out);
    // 헬퍼 경유: `require_str(params, "plan_id", &id)`. 헬퍼 이름을 열거하지 않는 이유는
    // 그 목록 자체가 다음 누락 지점이 되기 때문이다 — `(params,"` 로 형태만 본다.
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

/// 라우팅이 인식하는 id 키 — `request_owner.rs` 의 **테스트 밖** 부분에서 뽑는다.
///
/// 테스트 모듈을 자르는 것이 핵심이다. 그 안에는 `json!({ "hook_id": … })` 같은
/// 픽스처가 있어서, 통째로 읽으면 **테스트가 언급하기만 한 키가 인식된 것으로 잡힌다.**
/// 그러면 이 가드는 자기 대조군을 스스로 부풀려 언제나 초록이 된다.

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

/// 핸들러가 읽는 id 키 → 그 키를 읽는 파일들.
fn handler_id_keys() -> BTreeMap<String, Vec<String>> {
    let files = handler_sources();
    assert!(
        files.len() >= MIN_HANDLER_FILES,
        "핸들러 `.rs` 를 {} 개만 걷었다(하한 {MIN_HANDLER_FILES}, 2026-09-05 실측 75). \
         경로가 틀리면 예외가 아니라 조용한 0 이 되고, 모수가 비면 아래 집합 동등은 \
         양쪽이 빈 집합이라 그냥 통과한다",
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
        "핸들러에서 id 키를 하나도 못 뽑았다 — 추출기가 죽었다"
    );
    out
}

/// 핸들러가 대상으로 읽는 id 키는 **라우팅이 알거나 면제 목록에 있다.**
#[test]
fn every_id_key_a_handler_reads_is_routed_or_exempt() {
    let found = handler_id_keys();
    let recognised = recognised_routing_keys(&read(&repo_root().join(ROUTING_SOURCE)));
    assert!(
        !recognised.is_empty(),
        "{ROUTING_SOURCE} 에서 인식 키를 하나도 못 뽑았다 — 대조군이 죽었다"
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
        "라우팅이 인식하면서 동시에 면제된 키: {both:?} — 두 목록은 서로 배타여야 하며,          겹치면 면제 사유가 라우팅 코드와 모순이라는 뜻이다"
    );
    let stale: Vec<&str> = exempt
        .iter()
        .copied()
        .filter(|k| !found.contains_key(*k))
        .collect();

    assert!(
        unrouted.is_empty() && stale.is_empty(),
        "라우팅 키 목록이 핸들러가 읽는 키와 어긋난다.\n\
         \x20 라우팅도 면제도 모르는 키:\n{}\n\
         \x20 면제 목록에 있으나 아무 핸들러도 안 읽는 키: {stale:?}\n\
         앞의 것은 그 메서드가 조용히 **포커스된 창**으로 간다는 뜻이다. 창이 소유한 \
         리소스를 가리키면 `request_owner.rs` 에 인식시키고(키가 여러 뜻이면 메서드로 \
         한정), 아니면 NOT_A_ROUTING_TARGET 에 **사유와 함께** 적어라.",
        unrouted.join("\n")
    );
}

/// 추출기의 극성 — 무엇을 잡고 무엇을 안 잡는지.
///
/// 이것이 없으면 위 테스트는 "추출기가 아무것도 안 잡는다" 여도 통과한다(면제 목록이
/// stale 로 걸리긴 하지만, 면제가 비어 있던 시점에는 그것도 안 걸린다).
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
        "추출기의 극성이 달라졌다 — 줄바꿈된 `params\\n.get(..)` 를 잡고, `params` 가 \
         아닌 값의 `.get` 과 id 모양이 아닌 키는 안 잡아야 한다"
    );
}

/// 인식 키는 **테스트 밖**에서만 뽑는다.
///
/// `request_owner.rs` 의 테스트가 `json!({ "hook_id": … })` 로 키를 언급한다. 통째로
/// 읽으면 그 언급만으로 "인식됨" 이 되어, 라우팅이 실제로는 모르는 키가 통과한다.
/// 자기 대조군을 부풀리는 형태라 면제로 못 막는다 — 잘라내는 것으로 막고 여기서 못박는다.
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

/// 스캔 루트가 이 가드 자신을 포함하지 않는다.
///
/// 이 파일은 자기가 찾는 패턴(`"…_id"`)을 면제 목록과 픽스처로 **담고 있다.** 스캔
/// 루트가 넓어져 이 파일을 삼키면 자기 면제 목록을 "핸들러가 읽는 키" 로 세고, 그러면
/// 위 집합 동등이 자기 자신을 상대로 성립한다. 면제를 두지 않고 루트를 좁게 유지한다.
#[test]
fn the_scan_root_does_not_contain_this_guard() {
    let me = std::path::Path::new(file!());
    assert!(
        !me.starts_with(HANDLER_DIR),
        "이 가드({}) 가 스캔 루트({HANDLER_DIR}) 안에 있다 — 자기 목록을 실제 키로 센다",
        me.display()
    );
}
