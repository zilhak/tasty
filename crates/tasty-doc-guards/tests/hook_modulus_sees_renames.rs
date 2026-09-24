//! pre-commit의 staged 파일 필터가 이름 변경(R)을 포함하는지 소스에서 확인한다.
//! 문서만 바꾼 커밋은 Rust 파일이0개여도 정상이므로 개수 하한 대신 필터 문자열을 검사한다.
//! 파일을 여는 검사에 삭제된 경로를 넘기지 않도록 D는 요구하지 않는다. 삭제가 필요한 검사는 별도로 수집한다.
//! STAGED_RS가 같은 STAGED_ALL에서 파생되고 사용되는지도 문자열로 확인한다.

use tasty_doc_guards::repo_root;

const HOOK: &str = ".githooks/pre-commit";

const MODULUS_ASSIGN: &str = "STAGED_ALL=";

const DERIVED_ASSIGN: &str = "STAGED_RS=";

#[test]
fn the_staged_modulus_includes_renames() {
    let path = repo_root().join(HOOK);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("훅을 읽을 수 없다: {} — {e}", path.display()));

    let modulus_lines: Vec<&str> = src
        .lines()
        .filter(|l| l.trim_start().starts_with(MODULUS_ASSIGN))
        .collect();
    let derived_lines: Vec<&str> = src
        .lines()
        .filter(|l| l.trim_start().starts_with(DERIVED_ASSIGN))
        .collect();
    let consumers = src.matches("$STAGED_RS").count();
    let filter = modulus_lines
        .first()
        .and_then(|l| l.split("--diff-filter=").nth(1))
        .map(|rest| {
            rest.chars()
                .take_while(|c| c.is_ascii_uppercase())
                .collect::<String>()
        });

    eprintln!(
        "[hook-modulus] 훅 {} 줄 · 주 모수 대입 {} · 파생 대입 {} · 소비 자리 {} · 필터 {:?}",
        src.lines().count(),
        modulus_lines.len(),
        derived_lines.len(),
        consumers,
        filter.as_deref().unwrap_or("(못 찾음)")
    );

    assert_eq!(
        modulus_lines.len(),
        1,
        "{MODULUS_ASSIGN} 대입을 {}곳에서 찾았다. 한 번만 정의돼야 이 검사가 사용할 파일 목록을 특정할 수 있다.",
        modulus_lines.len()
    );
    assert_eq!(
        derived_lines.len(),
        1,
        "{DERIVED_ASSIGN} 대입을 {}곳에서 찾았다. Rust 목록을 정의하는 위치를 확인한다.",
        derived_lines.len()
    );
    assert!(
        derived_lines[0].contains("$STAGED_ALL"),
        "STAGED_RS가 STAGED_ALL에서 파생되지 않는다. 같은 수집 결과를 사용해야 한다.\n대입: {}",
        derived_lines[0].trim()
    );
    assert!(
        consumers >= 1,
        "STAGED_RS를 사용하는 표지를 찾지 못했다. 검사들이 어떤 파일 목록을 사용하는지 확인한다."
    );

    let filter = filter.expect("주 모수 줄에 `--diff-filter=` 가 없다 — 필터 없이 전부를 세면 이 시험의 물음 자체가 사라진다");
    for (letter, why) in [
        ('A', "새로 더한 파일"),
        ('C', "복사된 파일"),
        ('M', "고쳐진 파일"),
        ('R', "**이름이 바뀐 파일**"),
    ] {
        assert!(
            filter.contains(letter),
            "staged 필터 {filter}에 {why}({letter})이 빠졌다. 해당 변경만 있는 커밋도 검사에 포함해야 한다. 삭제된 파일을 여는 문제를 피하려고 D는 이 목록에서 요구하지 않는다."
        );
    }
}
