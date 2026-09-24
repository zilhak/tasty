//! 제품 코드에서 락 poison을 보고 없이 복구하는 곳을 찾는다.
//! 판정 범위는 poison_recovery 모듈, 정책은 docs/dev-guide/error-handling.md의 락 poison 절에 있다.
//! 소스 텍스트를 읽으므로 본체 빌드 없이 검사할 수 있다.
//!
//! 파일·복구·cfg 제외·test-only 제외·보고 처리 수에 각각 하한을 둔다.
//! 하한 통과는 수집의 완전성을 보장하지 않는다. 값이 줄면 실제 코드와 판정 범위를 함께 확인한다.

use tasty_doc_guards::poison_recovery::census;
use tasty_doc_guards::repo_root;
use tasty_doc_guards::temp_scratch::Scratch;

const SCAN_ROOTS: &[&str] = &["src", "crates"];

// 2026-09-05 측정: files=1186, poison_sites=38, cfg_gated=21, test_only=6, reported=11.
// 하한은 정상적인 감소를 허용하도록 이보다 낮게 두었다.
const MIN_FILES: usize = 1000;
const MIN_POISON_SITES: usize = 30;
const MIN_CFG_GATED: usize = 15;
const MIN_TEST_ONLY: usize = 4;
const MIN_REPORTED: usize = 8;

#[test]
fn no_shipping_lock_is_recovered_without_a_report() {
    let root = repo_root();
    let c = census(&root, SCAN_ROOTS);

    assert!(
        c.files_scanned >= MIN_FILES,
        "파일을 {}개만 수집했다(하한 {MIN_FILES}). Git의 src/crates 추적 Rust 목록과 비교하고 SCAN_ROOTS 누락·빈 디렉터리를 확인한다. read_dir 실패는 별도로 panic한다. 실제 대상이 줄었다면 근거를 기록하고 하한을 갱신한다.",
        c.files_scanned
    );
    assert!(
        c.poison_sites >= MIN_POISON_SITES,
        "poison 복구를 {}곳만 찾았다(하한 {MIN_POISON_SITES}). poison_recovery 단위 테스트와 실제 변경 코드를 대조해 인식 실패인지 복구 코드 감소인지 확인한다. 하한을 바꾸면 측정 근거도 남긴다.",
        c.poison_sites
    );
    assert!(
        c.cfg_gated >= MIN_CFG_GATED,
        "cfg(test)로 제외한 복구가 {}곳뿐이다(하한 {MIN_CFG_GATED}). 줄 단위 cfg 판정과 실제 cfg 변경을 확인한다. 파일 단위 test_only와는 별도 분류이므로 두 목록도 비교한다.",
        c.cfg_gated
    );
    assert!(
        c.test_only_sites >= MIN_TEST_ONLY,
        "test-only 파일의 복구가 {}곳뿐이다(하한 {MIN_TEST_ONLY}). 공용 shipping_scope::test_only_files의 단위 테스트와 실제 모듈 선언을 확인한다. 대상이 줄었다면 근거를 기록하고 하한을 갱신한다.",
        c.test_only_sites
    );
    assert!(
        c.reported >= MIN_REPORTED,
        "보고를 포함한 복구가 {}곳뿐이다(하한 {MIN_REPORTED}). poison_recovery 단위 테스트와 실제 보고 코드를 확인한다. 보고 삭제·복구 삭제·분류 변경을 구별하고 하한 변경 시 근거를 남긴다.",
        c.reported
    );

    assert!(
        c.silent.is_empty(),
        "보고 없이 락 poison을 복구하는 곳이 {}개 있다. tasty_utils::poison::recover_* 헬퍼를 쓰거나 복구 분기에서 보고한다(docs/dev-guide/error-handling.md의 락 poison 절):\n{}",
        c.silent.len(),
        c.silent
            .iter()
            .map(|s| format!("  {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// test-only 범위를 너무 넓게 잡으면 실제 위반도 제외되고 제외 수의 하한은 오히려 충족된다.
/// 합성 트리에서 census 전체를 호출해 분류와 실제 수집 경로를 함께 검사한다.
#[test]
fn the_census_routes_and_counts_on_a_substituted_tree() {
    let probe = Scratch::new("poison-census");
    let dir = probe.path();
    std::fs::create_dir_all(dir.join("zone/nested")).expect("합성 트리를 만들지 못했다");

    std::fs::write(
        dir.join("zone/silent_one.rs"),
        "fn ship() {\n    let g = m.lock().unwrap_or_else(|p| p.into_inner());\n}\n",
    )
    .expect("합성 위반 파일 실패");
    std::fs::write(
        dir.join("zone/nested/deep_silent.rs"),
        "fn deep() {\n    let g = m.write().unwrap_or_else(|p| p.into_inner());\n}\n",
    )
    .expect("합성 하위 위반 파일 실패");
    std::fs::write(
        dir.join("zone/reported_one.rs"),
        "fn ship() {\n    m.lock().unwrap_or_else(|poisoned| {\n        \
         tracing::error!(\"zone lock poisoned\");\n        poisoned.into_inner()\n    });\n}\n",
    )
    .expect("합성 보고 파일 실패");
    std::fs::write(
        dir.join("zone/cfg_gated_one.rs"),
        "pub fn ship() {}\n\n#[cfg(test)]\nmod t {\n    #[test]\n    fn x() {\n        \
         let _g = m.lock().unwrap_or_else(|p| p.into_inner());\n    }\n}\n",
    )
    .expect("합성 cfg 파일 실패");
    std::fs::write(
        dir.join("zone/comment_only.rs"),
        "/// into_inner() 로 되돌리는 자리를 설명하는 주석이다.\nfn ship() {}\n",
    )
    .expect("합성 주석 파일 실패");

    let c = census(dir, &["zone"]);

    assert_eq!(c.files_scanned, 5, "걷은 파일 수가 다르다");
    assert_eq!(
        c.silent.len(),
        2,
        "조용한 복구를 {} 곳으로 셌다 — 기대 2. 목록: {:?}",
        c.silent.len(),
        c.silent
    );
    // 이 트리에는 test-only 선언이 없어 해당 분류가 0이어야 한다.
    assert_eq!(
        c.test_only_sites, 0,
        "test-only 선언이 없는 합성 트리에서 복구를 제외했다. 제외 수가 늘어나는 오류는 하한만으로 찾을 수 없다."
    );
    assert!(
        c.silent
            .iter()
            .any(|s| s.starts_with("zone/silent_one.rs:2:")),
        "위반 좌표가 `경로:1기반줄` 로 안 나온다 — 목록: {:?}",
        c.silent
    );
    assert!(
        c.silent.iter().any(|s| s.contains("nested/deep_silent.rs")),
        "하위 디렉토리로 안 내려갔다 — 목록: {:?}",
        c.silent
    );
    assert!(
        !c.silent.iter().any(|s| s.contains("reported_one.rs")),
        "보고가 붙은 복구를 위반으로 셌다"
    );
    assert!(
        !c.silent.iter().any(|s| s.contains("cfg_gated_one.rs")),
        "`#[cfg(test)]` 아래 복구를 위반으로 셌다"
    );
    assert!(
        !c.silent.iter().any(|s| s.contains("comment_only.rs")),
        "주석 안의 언급을 복구로 셌다"
    );
    assert!(c.reported >= 1, "보고로 통과한 자리를 하나도 못 셌다");
    assert!(c.cfg_gated >= 1, "cfg 로 뺀 자리를 하나도 못 셌다");
    assert_eq!(
        c.poison_sites,
        c.silent.len() + c.reported + c.cfg_gated + c.test_only_sites,
        "분류별 합이 전체 복구 수와 다르다. 누락이나 중복 분류를 확인한다."
    );
}
