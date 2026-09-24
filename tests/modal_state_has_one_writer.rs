//! View의 모달 ID와 AppState의 조회용 ID·종류를 함께 갱신하는지 소스로 확인한다.
//! 파일별 문자열 존재와 open·close 함수 이름 사이 구간을 비교하므로 실행 경로별 동기화를 증명하지는 않는다.

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

const OWNER: &str = "src/app/modal.rs";

fn writes_the_original(t: &str) -> bool {
    t.contains("view.active_modal_id = ") || t.contains("view.active_modal_id.take")
}

fn writes_the_mirror(t: &str) -> bool {
    t.contains("state.active_modal_id = ")
}

fn writes_the_kind(t: &str) -> bool {
    t.contains("state.active_modal_kind = ")
}

#[test]
fn every_file_that_moves_the_original_moves_the_mirror_too() {
    let offenders: Vec<String> = sources()
        .into_iter()
        .filter(|(_, t)| writes_the_original(t) && !writes_the_mirror(t))
        .map(|(rel, _)| rel)
        .collect();
    assert!(
        offenders.is_empty(),
        "View 모달 ID의 변경 표지가 있지만 AppState 사본의 변경 표지가 없는 파일이다. 실제 갱신 경로를 확인한다: {offenders:?}"
    );
}

#[test]
fn nobody_touches_the_mirror_alone() {
    let offenders: Vec<String> = sources()
        .into_iter()
        .filter(|(rel, t)| rel != "src/state.rs" && writes_the_mirror(t) && !writes_the_original(t))
        .map(|(rel, _)| rel)
        .collect();
    assert!(
        offenders.is_empty(),
        "AppState 사본의 변경 표지만 있는 파일이다. View 상태와 함께 갱신되는지 확인한다: {offenders:?}"
    );
}

#[test]
fn the_open_close_pair_still_lives_in_one_file() {
    let files = sources();
    let owner = files
        .iter()
        .find(|(rel, _)| rel == OWNER)
        .map(|(_, t)| t.as_str())
        .unwrap_or_else(|| panic!("`{OWNER}` 를 모수에서 못 찾았다 — 파일이 옮겨졌다"));
    let writes = owner.matches("state.active_modal_id = ").count();
    assert_eq!(
        writes, 2,
        "여닫는 파일이 사본을 두 번 써야 한다(열기 · 닫기). 지금 {writes} 곳이다."
    );
}

/// 함수 이름의 위치로 나눈 구간을 확인한다. 끝 구간은 파일 끝까지여서 함수 경계를 정확히 파싱하지는 않는다.
#[test]
fn each_mirror_write_sits_inside_the_function_that_moves_the_original() {
    let files = sources();
    let owner = files
        .iter()
        .find(|(rel, _)| rel == OWNER)
        .map(|(_, t)| t.as_str())
        .expect("owner 파일");
    let open_at = owner
        .find("fn open_modal(")
        .expect("`open_modal` 을 못 찾았다");
    let close_at = owner
        .find("fn close_active_modal(")
        .expect("`close_active_modal` 을 못 찾았다");
    assert!(
        open_at < close_at,
        "두 함수의 순서가 바뀌었다 — 아래 구간 판정이 무의미해진다"
    );

    let opening = &owner[open_at..close_at];
    let closing = &owner[close_at..];
    assert!(
        opening.contains("state.active_modal_id = Some("),
        "모달을 여는 함수가 사본을 안 세운다 — 그러면 `ui.state` 는 모달이 떠도 없다고 말한다"
    );
    assert!(
        closing.contains("state.active_modal_id = None"),
        "모달 닫기 구간에서 조회용 ID를 지우는 코드를 찾지 못했다"
    );
}

#[test]
fn the_kind_moves_with_the_id() {
    let offenders: Vec<String> = sources()
        .into_iter()
        .filter(|(rel, t)| rel != "src/state.rs" && (writes_the_mirror(t) != writes_the_kind(t)))
        .map(|(rel, _)| rel)
        .collect();
    assert!(
        offenders.is_empty(),
        "AppState 모달 ID와 종류 중 한쪽 변경 표지만 있는 파일이다. 두 상태의 갱신 경로를 확인한다: {offenders:?}"
    );
}

#[test]
fn each_kind_write_sits_inside_the_function_that_moves_the_original() {
    let files = sources();
    let owner = files
        .iter()
        .find(|(rel, _)| rel == OWNER)
        .map(|(_, t)| t.as_str())
        .expect("owner 파일");
    let open_at = owner
        .find("fn open_modal(")
        .expect("`open_modal` 을 못 찾았다");
    let close_at = owner
        .find("fn close_active_modal(")
        .expect("`close_active_modal` 을 못 찾았다");
    assert!(open_at < close_at, "두 함수의 순서가 바뀌었다");

    assert!(
        owner[open_at..close_at].contains("state.active_modal_kind = Some("),
        "모달을 여는 함수가 종류를 안 세운다 — `ui.state` 는 무언가 떠 있다고만 말하고 \
         무엇인지는 직전 값으로 답한다"
    );
    assert!(
        owner[close_at..].contains("state.active_modal_kind = None"),
        "모달을 닫는 함수가 종류를 안 지운다 — 닫힌 뒤에도 그 종류가 남는다"
    );
}
