//! 계층 검사의 두 전제가 여전히 맞는지 확인한다.
//!
//! 루트가 CLI 크레이트를 제품 의존성으로 갖는 동안에는 잘못된 참조도 컴파일될 수 있어
//! 소스 검사가 필요하다. 또 예외 파일이 쓰는 픽스처가 cfg(test) 전용이면 lib가 있어도
//! 외부 통합 테스트에서 사용할 수 없다.
//!
//! 두 전제는 각각 manifest와 공용 제품 포함 여부 분석기로 확인한다.
//! 별도 픽스처 크레이트를 만드는 등 이 검사가 직접 발견하지 못하는 변경은 리뷰한다.
//! 정책은 `docs/adr/0048-source-guards-and-exemptions.md`와
//! `docs/dev-guide/guard-population.md`의 검사 범위·예외 관리 절을 따른다.

use std::fs;
use tasty_doc_guards::cargo_manifest::{declared_deps, sections_declaring};
use tasty_doc_guards::repo_root;
use tasty_doc_guards::shipping_scope::test_only_files;
use tasty_doc_guards::source_text::rust_sources;

/// 루트 매니페스트 의존 수집의 하한. --nocapture 출력에서 실제 수를 확인할 수 있다.
const MIN_ROOT_DEPS: usize = 60;

/// 계층 검사에서 사용하는 크레이트 이름. 검색 대상 문자열과 구별하도록 조립한다.
const CLI_CRATE: &str = concat!("tasty", "-cli");

fn root_manifest() -> String {
    let path = repo_root().join("Cargo.toml");
    fs::read_to_string(&path)
        .unwrap_or_else(|why| panic!("루트 매니페스트를 읽지 못했다({}): {why}", path.display()))
}

#[test]
fn the_cli_crate_is_still_a_production_dependency_of_the_root_package() {
    let manifest = root_manifest();
    let all = declared_deps(&manifest);
    println!("[ADR-0048 좌변] 루트 매니페스트 의존 {} 개", all.len());
    assert!(
        all.len() >= MIN_ROOT_DEPS,
        "루트 매니페스트 의존을 {}개만 읽었다(하한 {MIN_ROOT_DEPS}). 파서와 의존 목록을 확인한다.",
        all.len()
    );

    let sections = sections_declaring(&manifest, CLI_CRATE);
    assert!(
        !sections.is_empty(),
        "루트 매니페스트에 {CLI_CRATE} 의존 선언이 없다. 본체가 CLI 파서를 소유한다는 전제가 바뀌었다면 ADR-0048과 layering 검사의 예외·범위를 함께 재검토한다."
    );
    assert!(
        sections.iter().any(|s| s == "dependencies"),
        "{CLI_CRATE}가 제품 의존성에 없다(선언 절: {sections:?}). 컴파일러가 제품 참조를 막게 됐다면 layering 검사의 필요성과 범위를 ADR-0048에 따라 재검토한다."
    );
}

/// 외부 통합 테스트가 사용하려면 제품 lib에 포함돼야 하는 현재 테스트 전용 픽스처.
const FIXTURES_ALTERNATIVE_B_NEEDS: &[&str] = &["src/state/tests.rs", "src/adapters/test/mod.rs"];

#[test]
fn the_root_lib_target_still_cannot_hand_the_fixtures_to_tests() {
    let root = repo_root();
    let sources = rust_sources(&root, &["src"]);
    assert!(
        sources.len() > 100,
        "src에서 {}개만 수집했다. 테스트 전용 여부를 판단하기 전에 수집 범위를 확인한다.",
        sources.len()
    );
    let test_only = test_only_files(&root, &sources);
    println!(
        "[ADR-0048 좌변] src 스캔 {} 파일 · 선언상 출하 안 되는 파일 {} 개",
        sources.len(),
        test_only.len()
    );

    for rel in FIXTURES_ALTERNATIVE_B_NEEDS {
        assert!(
            test_only.contains(std::path::Path::new(rel)),
            "{rel}이 더 이상 테스트 전용 파일로 분류되지 않는다. 외부 통합 테스트가 픽스처를 사용할 수 있는지 확인하고 layering의 TEST_ONLY_FILES와 ADR-0048의 근거를 재검토한다."
        );
    }
}
