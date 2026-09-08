//! [ADR-0123](../../../docs/adr/0123-layering-guard-excludes-cfg-test-modules.md) 의 재검토
//! 조건 **둘**에 발화 자리를 준다. 그 ADR 은 계층 가드가 **텍스트 검사**여야 하는 이유를
//! 두 사실 위에 세웠고, 둘 다 루트 `Cargo.toml` 이 답한다.
//!
//! # 좌변은 하나, 물음은 둘
//!
//! 파일 하나(루트 `Cargo.toml`)를 보지만 **시험은 둘**이다. 두 물음을 한 시험에 넣으면
//! 어느 쪽이 발화했는지 실패문이 답하지 못하고, 처방이 둘 중 하나로만 쓰인다.
//!
//! - `tasty-cli` 가 **프로덕션 의존**인가 — 아니면 프로덕션 참조가 컴파일 단계에서
//!   막히므로 텍스트 검사 자체가 대부분 불필요해진다.
//! - 루트 패키지에 **lib 타깃이 없는가** — 생기면 그 ADR 의 대안 B(면제 항목을 `tests/`
//!   로 옮기기)가 실현 가능해진다.
//!
//! # lib 타깃은 `[lib]` 절만으로 안 갈린다
//!
//! cargo 는 `src/lib.rs` 가 있으면 **절이 없어도** lib 타깃을 만든다. 그래서 좌변은 둘
//! 다다 — 절과 파일. 절만 보면 `src/lib.rs` 를 더하는 커밋이 조용히 지나간다.
//!
//! # 이 가드가 안 보는 것
//!
//! `[[bin]]` 추가·`autolib` 같은 다른 타깃 선언. 오늘 그 형태는 이 ADR 의 조건이 아니다.
//! 의존 절을 읽는 판정은 [`tasty_doc_guards::cargo_manifest`] 하나가 한다 — 사본을 두면
//! 한쪽만 고쳐진 날 답이 갈린다.

use std::fs;
use tasty_doc_guards::cargo_manifest::{declared_deps, sections_declaring};
use tasty_doc_guards::repo_root;

/// 루트 매니페스트가 선언하는 의존 수의 하한. 파서가 죽으면 아래 판정이 빈 집합을 본다.
/// 실측 2026-09-08: 아래 census 줄이 찍는다.
const MIN_ROOT_DEPS: usize = 60;

/// 이 ADR 의 전제가 걸려 있는 크레이트. 조각으로 두는 이유는 이 파일이 계층 가드의
/// 좌변("본체가 그 이름을 참조하는가")에 앉지 않게 하기 위함이다 —
/// 실측 2026-09-08 에 다른 가드에서 그 형태로 한 번 물렸다.
const CLI_CRATE: &str = concat!("tasty", "-cli");

fn root_manifest() -> String {
    let path = repo_root().join("Cargo.toml");
    fs::read_to_string(&path).unwrap_or_else(|why| {
        panic!(
            "루트 매니페스트를 못 읽었다 ({}): {why} — 못 읽은 채로 통과하면 이 시험은 \
             \"전제가 유지된다\" 가 아니라 \"안 봤다\" 가 된다",
            path.display()
        )
    })
}

#[test]
fn the_cli_crate_is_still_a_production_dependency_of_the_root_package() {
    let manifest = root_manifest();
    let all = declared_deps(&manifest);
    println!("[ADR-0123 좌변] 루트 매니페스트 의존 {} 개", all.len());
    assert!(
        all.len() >= MIN_ROOT_DEPS,
        "루트 매니페스트에서 의존을 {} 개만 읽었다(하한 {MIN_ROOT_DEPS}) — 절 파서가 죽으면 \
         아래 판정은 \"선언이 없다\" 를 \"옮겨졌다\" 로 읽는다",
        all.len()
    );

    let sections = sections_declaring(&manifest, CLI_CRATE);
    assert!(
        !sections.is_empty(),
        "루트 매니페스트가 `{CLI_CRATE}` 를 어느 절에서도 선언하지 않는다.\n\
         ★ 이것은 회귀가 아니라 **ADR-0123 의 재검토 조건이 발동한 것**이다 — 다만 위 \
         조건이 아니라 그 다음 것이다: \"본체가 CLI 파서를 더 이상 소유하지 않게 된다\". \
         그때는 `ALLOWED_PATHS` 를 포함해 `crates/tasty-doc-guards/tests/layering.rs` 전체의 \
         전제가 바뀐다.\n\
         ☞ 이 시험을 지워서 통과시키지 마라. 그 ADR 을 열어 결정을 다시 써라."
    );
    assert!(
        sections.iter().any(|s| s == "dependencies"),
        "`{CLI_CRATE}` 가 프로덕션 의존이 아니다 — 선언된 절: {sections:?}.\n\
         ★ 이것은 회귀가 아니라 **ADR-0123 의 첫째 재검토 조건이 발동한 것**이다 \
         (docs/adr/0123-layering-guard-excludes-cfg-test-modules.md).\n\
         그 ADR 은 계층 가드가 **텍스트 검사**여야 하는 이유로 \"프로덕션 참조가 컴파일 \
         단계에서 안 막힌다\" 를 들었다. 이 크레이트가 `dev-dependencies` 로 내려가면 \
         프로덕션 참조가 컴파일 에러가 되므로 그 텍스트 검사가 대부분 불필요해진다.\n\
         순서가 있다. (1) `layering.rs` 의 위반 목록이 실제로 비는지 돌려서 확인한다. \
         (2) 비면 그 가드의 범위를 줄이거나 지운다 — 남겨 두면 컴파일러와 가드가 같은 \
         물음에 답하는 판사 둘이 된다. (3) 그 ADR 의 결정을 다시 쓴다.\n\
         ☞ 이 시험을 고쳐서 통과시키지 마라 — 그러면 재검토 조건이 다시 문장이 된다."
    );
}

#[test]
fn the_root_package_still_has_no_lib_target() {
    let manifest = root_manifest();
    let has_lib_section = manifest
        .lines()
        .any(|l| l.trim() == "[lib]" || l.trim().starts_with("[lib."));
    // cargo 는 `src/lib.rs` 만으로도 lib 타깃을 만든다 — 절만 보면 이 축이 반만 덮인다.
    let inferred = repo_root().join("src").join("lib.rs").is_file();

    assert!(
        !has_lib_section && !inferred,
        "루트 패키지에 lib 타깃이 생겼다(`[lib]` 절 {has_lib_section} · `src/lib.rs` {inferred}).\n\
         ★ 이것은 회귀가 아니라 **ADR-0123 의 셋째 재검토 조건이 발동한 것**이다 \
         (docs/adr/0123-layering-guard-excludes-cfg-test-modules.md).\n\
         그 ADR 이 `TEST_ONLY_FILES` 를 **소스 안**에 두기로 한 근거가 \"tasty 는 lib 타깃이 \
         없는 바이너리 크레이트라 `tests/` 통합 테스트에서 핸들러·AppState 픽스처에 \
         닿을 수 없다\" 였다. lib 타깃이 생기면 그 근거가 사라지고 대안 B(면제 항목을 \
         `tests/` 로 옮기기)가 실현 가능해진다.\n\
         순서가 있다. (1) `crates/tasty-doc-guards/tests/layering.rs` 의 `TEST_ONLY_FILES` \
         항목이 `tests/` 로 옮겨질 수 있는지 본다. (2) 옮겨지면 그 목록이 줄고, 목록이 \
         비면 그 절 자체가 사라진다. (3) 그 ADR 의 결정을 다시 쓴다.\n\
         ☞ 이 시험을 지워서 통과시키지 마라."
    );
}
