//! 테스트의 환경변수·현재 디렉터리 변경에 직렬화 근거가 있는지 확인한다(ADR-0045).
//! 함수 안의 락 참조, 직렬화 사유 주석 또는 단일 테스트 격리 표시를 인정한다.
//! 정확한 판정 범위와 한계는 env_isolation 모듈에 있다.
//!
//! 파일 수·변경 수·직렬화로 분류한 수의 하한을 따로 확인한다.
//! 작은 디렉터리의 수집 실패가 전체 수에 가려지지 않도록 디렉터리별 하한도 둔다.
//! 경로가 없거나 읽기에 실패하면 rust_sources가 panic한다. 빈 경로와 SCAN_ROOTS 누락은
//! 하한으로 찾지만, 그 통과가 일부 파일 누락까지 없음을 뜻하지는 않는다.
//!
//! 통과할 때도 측정 수를 보려면 아래처럼 실행한다.
//! ```text
//! cargo test -p tasty-doc-guards --test no_unserialized_env_mutation -- --nocapture
//! ```

use tasty_doc_guards::env_isolation::census;
use tasty_doc_guards::repo_root;

// src와 crates 외에 통합 테스트의 환경 변경을 찾도록 루트 tests도 포함한다.
const SCAN_ROOTS: &[&str] = &["src", "crates", "tests"];

// 2026-09-07 측정: files=1309, mutations=25, serialized=25, bare=0.
// serialized + bare = mutations 관계와 실제 코드를 함께 확인해 수집·분류 변경을 구별한다.
const MIN_FILES: usize = 1100;

// 2026-09-07 측정: src 598, crates 672, tests 39.
// 파일 정리를 허용하도록 src/crates는 약 3/4, 가드를 옮기는 중이던 tests는 약 절반으로 정했다.
// 디렉터리별 수집 실패를 찾는 하한이며 정상적인 파일 감소를 막는 값은 아니다.
const MIN_PER_ROOT: &[(&str, usize)] = &[("src", 450), ("crates", 480), ("tests", 20)];
const MIN_MUTATIONS: usize = 15;
const MIN_SERIALIZED: usize = 15;

#[test]
fn every_test_env_mutation_is_serialized() {
    let root = repo_root();
    let c = census(&root, SCAN_ROOTS);

    // 실패하더라도 수집량을 볼 수 있도록 단언 전에 출력한다.
    eprintln!(
        "[env-isolation] 파일 {} (뿌리별 {}) · 변형 {} · 직렬화 {} · 위반 {}",
        c.files_scanned,
        c.per_root
            .iter()
            .map(|(r, n)| format!("{r}={n}"))
            .collect::<Vec<_>>()
            .join(" "),
        c.mutations,
        c.serialized,
        c.bare.len()
    );

    assert!(
        c.files_scanned >= MIN_FILES,
        "파일을 {}개만 수집했다(하한 {MIN_FILES}). 출력된 디렉터리별 수와 SCAN_ROOTS를 확인한다. 총수만으로 작은 디렉터리의 누락을 찾을 수 없어 개별 하한도 유지해야 한다. 실제 대상이 줄었다면 측정 근거와 함께 갱신한다.",
        c.files_scanned
    );
    assert_eq!(
        c.per_root.len(),
        SCAN_ROOTS.len(),
        "디렉터리별 수가 {}개인데 스캔 루트는 {}개다. 수집 결과의 분류를 확인한다.",
        c.per_root.len(),
        SCAN_ROOTS.len()
    );
    assert_eq!(
        c.per_root.iter().map(|(_, n)| n).sum::<usize>(),
        c.files_scanned,
        "디렉터리별 합과 전체 파일 수가 다르다. 경로의 중복·누락과 접두 판정을 확인한다."
    );
    for &(root_name, floor) in MIN_PER_ROOT {
        let got = c
            .per_root
            .iter()
            .find(|(r, _)| r == root_name)
            .map(|(_, n)| *n);
        assert!(
            got.is_some(),
            "하한을 등록한 `{root_name}`이 census에 없다. SCAN_ROOTS와 MIN_PER_ROOT를 대조한다. 제외가 필요하다면 해당 디렉터리를 검사하지 않아도 되는 근거를 기록한다."
        );
        let got = got.unwrap_or(0);
        assert!(
            got >= floor,
            "`{root_name}`에서 {got}파일만 수집했다(하한 {floor}). SCAN_ROOTS와 해당 디렉터리의 Rust 파일을 확인한다. 경로 읽기 실패는 별도로 panic하며, 여기서는 비었거나 수집량이 줄어든 경우를 판단한다. 정상 감소라면 개별 하한의 측정 근거도 갱신한다."
        );
    }
    assert!(
        c.mutations >= MIN_MUTATIONS,
        "테스트의 env/cwd 변경을 {}곳만 찾았다(하한 {MIN_MUTATIONS}). env_isolation 단위 테스트와 실제 변경 코드를 대조해 변경 인식·테스트 범위 판정·실제 코드 감소를 구별한다. 하한만 먼저 낮추지 않는다.",
        c.mutations
    );
    assert!(
        c.serialized >= MIN_SERIALIZED,
        "직렬화로 분류한 변경이 {}곳뿐이다(하한 {MIN_SERIALIZED}). serialized + bare = mutations를 확인하고 락·사유 주석·단일 테스트 표시의 실제 변경을 대조한다. env_isolation 단위 테스트로 문자열 속 표시를 잘못 인정하지 않는지도 확인한다.",
        c.serialized
    );

    assert!(
        c.bare.is_empty(),
        "직렬화 근거가 없는 테스트의 env/cwd 변경이 {}곳 있다. 병렬 테스트가 상태를 덮어쓸 수 있으므로 함수에서 직렬화 락을 잡거나 RAII 가드의 호출자가 지킬 직렬화 조건을 주석에 적는다(ADR-0045):\n{}",
        c.bare.len(),
        c.bare
            .iter()
            .map(|s| format!("  {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
