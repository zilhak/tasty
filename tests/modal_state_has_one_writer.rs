//! 활성 모달의 ID·종류가 ViewRegistry 한 곳에만 있고 정해진 함수만 바꾸는지 소스로 확인한다.
//! 파일별 문자열 존재와 함수 이름 사이 구간을 비교하므로 실행 경로별 동작을 증명하지는 않는다.

use std::path::Path;

use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};

/// 소스 수집이 비어 위반도 없는 것으로 처리되지 않도록 공용 하한을 적용한다.
const SRC_FLOOR: Floor = Floor {
    min: 587,
    measured: tasty_doc_guards::floored_walk::populations::SRC_RS.measured,
    measured_on: tasty_doc_guards::floored_walk::populations::SRC_RS.measured_on,
    counted_on: tasty_doc_guards::floored_walk::populations::SRC_RS.counted_on,
    why_this_gap: "src의 파일 수는 공용 측정을 사용한다. 과거 fdca139c0..91ca7d37d 구간의 최대 감소 24개를 기준으로, 하한의 여유가 두 번의 감소를 감당하는지 비교했다. 크레이트 분리로 한꺼번에 이동하는 경우까지 보장하지는 않는다. 하한에 걸리면 실제 이동·삭제와 순회 누락을 구별해 다시 측정한다.",
};

fn sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let found: Vec<Walked> = walk_with_floor(
        &root.join("src"),
        root,
        &SRC_FLOOR,
        Descend::SkipBuildCaches,
        &|f| f.rel.ends_with(".rs"),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    found
        .into_iter()
        .map(|f| {
            let t = std::fs::read_to_string(&f.path)
                .unwrap_or_else(|e| panic!("read {}: {e}", f.path.display()));
            (f.rel, t)
        })
        .collect()
}

fn file<'a>(files: &'a [(String, String)], rel: &str) -> &'a str {
    files
        .iter()
        .find(|(r, _)| r == rel)
        .map(|(_, t)| t.as_str())
        .unwrap_or_else(|| panic!("`{rel}` 를 모수에서 못 찾았다 — 파일이 옮겨졌다"))
}

const REGISTRY: &str = "src/view/mod.rs";
const OWNER: &str = "src/app/modal.rs";
/// 최소화는 모달 View를 함께 버리므로 닫기 처리 없이 원본만 비운다.
const MINIMIZE: &str = "src/app/event_handler.rs";
const ROUTING: &str = "src/app/ipc/routing.rs";

/// 창별 AppState에 사본이 다시 생기면 parked 상태나 다른 창과 값이 갈라진다.
#[test]
fn no_per_window_state_keeps_a_modal_copy() {
    let offenders: Vec<String> = sources()
        .into_iter()
        .filter(|(_, t)| t.contains("state.active_modal"))
        .map(|(rel, _)| rel)
        .collect();
    assert!(
        offenders.is_empty(),
        "창별 상태의 모달 필드를 읽거나 쓰는 파일이다. 활성 모달은 ViewRegistry에서 읽는다: {offenders:?}"
    );
    let state = file(&sources(), "src/state.rs").to_string();
    assert!(
        !state.contains("active_modal"),
        "`src/state.rs` 에 모달 필드가 다시 생겼다. AppState는 모달 사본을 갖지 않는다"
    );
}

/// 필드가 비공개여야 아래의 호출 위치 검사가 모든 쓰기를 덮는다.
#[test]
fn the_registry_field_is_private_and_holds_id_and_kind_together() {
    let files = sources();
    let registry = file(&files, REGISTRY);
    assert!(
        registry.contains("\n    active_modal: Option<ActiveModal>,"),
        "ViewRegistry의 활성 모달이 ID·종류를 함께 담는 비공개 필드가 아니다"
    );
    assert!(
        !registry.contains("pub active_modal") && !registry.contains("pub(crate) active_modal:"),
        "활성 모달 필드가 공개됐다. 쓰기는 set_active_modal/take_active_modal만 사용한다"
    );
}

fn callers_of(files: &[(String, String)], call: &str) -> Vec<String> {
    files
        .iter()
        .filter(|(rel, t)| rel != REGISTRY && t.contains(call))
        .map(|(rel, _)| rel.clone())
        .collect()
}

#[test]
fn only_open_modal_sets_the_active_modal() {
    let files = sources();
    assert_eq!(
        callers_of(&files, "set_active_modal("),
        vec![OWNER.to_string()],
        "활성 모달을 세우는 곳은 `App::open_modal` 뿐이어야 한다"
    );
    let owner = file(&files, OWNER);
    let open_at = owner
        .find("fn open_modal(")
        .expect("`open_modal` 을 못 찾았다");
    let close_at = owner
        .find("fn close_active_modal(")
        .expect("`close_active_modal` 을 못 찾았다");
    assert!(
        open_at < close_at,
        "두 함수의 순서가 바뀌었다 — 구간 판정이 무의미해진다"
    );
    assert_eq!(owner.matches("set_active_modal(").count(), 1);
    assert!(
        owner[open_at..close_at].contains("self.view.set_active_modal(window_id, kind)"),
        "모달을 여는 함수가 ID·종류를 세우지 않는다 — `ui.state` 는 모달이 떠도 없다고 말한다"
    );
    assert!(
        owner[close_at..].contains("self.view.take_active_modal()"),
        "모달을 닫는 함수가 활성 모달을 지우지 않는다 — 닫힌 뒤에도 입력이 막힌다"
    );
}

#[test]
fn only_close_and_minimize_clear_the_active_modal() {
    let files = sources();
    let mut callers = callers_of(&files, "take_active_modal(");
    callers.sort();
    let mut expected = vec![OWNER.to_string(), MINIMIZE.to_string()];
    expected.sort();
    assert_eq!(
        callers, expected,
        "활성 모달을 지우는 곳이 늘거나 줄었다. 닫기 처리를 건너뛰는 경로인지 확인한다"
    );
    let minimize = file(&files, MINIMIZE);
    let at = minimize
        .find("fn handle_minimize(")
        .expect("`handle_minimize` 를 못 찾았다");
    assert_eq!(minimize.matches("take_active_modal(").count(), 1);
    assert!(
        minimize[at..].contains("take_active_modal("),
        "`{MINIMIZE}` 의 활성 모달 지우기가 최소화 처리 밖에 있다"
    );
}

/// 창과 parked 상태 어느 쪽이 응답해도 모달 필드가 같은 원본에서 온다.
#[test]
fn every_routed_ui_state_reports_the_registry_modal() {
    let files = sources();
    let handler = file(&files, "src/adapters/ipc/handler/debug_state.rs");
    assert!(
        handler.contains("\"modal_open\": false"),
        "handler 기본값이 모달 없음이 아니다. 헤드리스·원격 복제본은 모달을 갖지 않는다"
    );
    let routing = file(&files, ROUTING);
    let handled = routing.matches("handle_checked_request(").count();
    let projected = routing
        .matches("self.send_routed_response(cmd, response)")
        .count();
    assert!(handled > 0, "`{ROUTING}` 에서 handler 호출을 못 찾았다");
    assert_eq!(
        handled, projected,
        "handler 응답 중 모달 투영을 거치지 않고 보내는 경로가 있다"
    );
    assert!(
        routing.contains("self.project_active_modal(&cmd.request.method, response)"),
        "`send_routed_response` 가 모달 투영을 호출하지 않는다"
    );
}
