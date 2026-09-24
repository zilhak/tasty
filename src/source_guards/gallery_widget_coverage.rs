//! tasty-ui-widgets의 재수출 모듈마다 항목 하나 이상의 이름이 갤러리 코드에 있는지 확인한다.
//! use만 있어도 통과하므로 실제 그리기나 전용 카드 존재를 보장하지 않는다.
//! 반환 타입 등을 모두 직접 명명할 필요는 없어 항목별이 아닌 모듈별로 검사한다.
//! 다른 크레이트의 공용 위젯은 범위 밖이며, 사용하지 않는 import의 경고를 실패 보장으로 삼지 않는다.
//! 출력 표는 --nocapture로 볼 수 있다.

use tasty_doc_guards::source_text::mask_non_code;

const WIDGETS_LIB: &str = "crates/tasty-ui-widgets/src/lib.rs";

const GALLERY_SRC: &str = "crates/tasty-gallery/src";

/// pub use를 pub mod로 바꿔 재수출 검사를 피하지 못하도록 공개 모듈도 등록한다.
const PUB_MODULES: &[(&str, &str)] = &[
    ("brand", "브랜드 자산(로고 등) 모듈 — 그릴 위젯이 아니다"),
    (
        "file_handler",
        "파일 선택기의 말줄임·문자 예산을 계산하는 공용 함수다. 본체와 갤러리가 같은 모델 규칙을 쓰기 위해 공개한다.",
    ),
    (
        "crumb_alloc",
        "경로 표시줄의 폭 배분을 계산하는 공용 함수다. 본체와 갤러리가 같은 배분 규칙을 쓰기 위해 공개한다.",
    ),
    ("tokens", "레이아웃 상수 모듈 — 값이지 위젯이 아니다"),
];

/// 위젯이 아닌 재수출의 예외 사유. 예외를 추가할 때 검사에 기록한 수도 함께 검토한다.
const NOT_A_WIDGET: &[(&str, &str)] = &[];

fn exported_modules(masked_lib: &str) -> Vec<(String, Vec<String>)> {
    let flat: String = masked_lib.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut rest = flat.as_str();
    while let Some(at) = rest.find("pub use ") {
        rest = &rest[at + "pub use ".len()..];
        let Some(end) = rest.find(';') else { break };
        let decl = &rest[..end];
        rest = &rest[end + 1..];
        let Some((module, tail)) = decl.split_once("::") else {
            continue;
        };
        let module = module.trim();
        if module.is_empty()
            || !module
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
        {
            continue;
        }
        let items: Vec<String> = tail
            .trim()
            .trim_start_matches('{')
            .trim_end_matches('}')
            .split(',')
            // 별칭으로 내보냈다면 외부 이름을 비교한다.
            .map(|s| s.rsplit(" as ").next().unwrap_or(s).trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if items.is_empty() {
            continue;
        }
        out.push((module.to_string(), items));
    }
    out
}

fn named(items: &[String], masked_gallery: &str) -> bool {
    items.iter().any(|i| contains_word(masked_gallery, i))
}

/// Button을 ButtonVariant와 혼동하지 않도록 식별자 경계를 확인한다.
fn contains_word(hay: &str, needle: &str) -> bool {
    let bytes = hay.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = hay[from..].find(needle) {
        let s = from + rel;
        let e = s + needle.len();
        let before_ok = s == 0 || !is_ident(bytes[s - 1]);
        let after_ok = e == bytes.len() || !is_ident(bytes[e]);
        if before_ok && after_ok {
            return true;
        }
        from = s + 1;
    }
    false
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn pub_modules(masked_lib: &str) -> Vec<String> {
    masked_lib
        .lines()
        .filter_map(|l| l.trim().strip_prefix("pub mod "))
        .filter_map(|r| r.split(';').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn masked(rel: &str) -> String {
    mask_non_code(&std::fs::read_to_string(super::repo_root().join(rel)).unwrap_or_default())
}

fn masked_gallery() -> String {
    let mut blob = String::new();
    let mut files = 0usize;
    for (rel, src) in super::rust_sources() {
        if !rel.starts_with(GALLERY_SRC) {
            continue;
        }
        blob.push_str(&mask_non_code(&src));
        blob.push('\n');
        files += 1;
    }
    assert!(
        files >= 80,
        "갤러리 소스를 {files}개만 수집했다(하한 80). `{GALLERY_SRC}` 경로와 수집 범위를 확인한다."
    );
    blob
}

#[test]
fn every_exported_widget_module_is_named_by_the_gallery() {
    let lib = masked(WIDGETS_LIB);
    let exported = exported_modules(&lib);

    assert!(
        exported.len() >= 20,
        "재수출 모듈을 {}개만 찾았다(하한 20). `{WIDGETS_LIB}` 읽기와 선언 추출을 확인한다.",
        exported.len()
    );
    assert!(
        exported.iter().any(|(m, _)| m == "button"),
        "재수출 목록에 대조 항목 `button` 이 없다 — 추출이 맞는지부터 의심하라"
    );

    let blob = masked_gallery();
    assert!(
        // 파일 수와 별도로 읽은 내용의 바이트 수도 확인한다. 측정 1.23MB의 절반 아래로 여유를 뒀으며 모든 부분 누락을 찾는 것은 아니다.
        blob.len() > 500_000,
        "갤러리 소스를 {} 바이트밖에 못 읽었다 — 경로가 옮겨졌으면 `{GALLERY_SRC}` 를 고쳐라",
        blob.len()
    );

    let mut missing = Vec::new();
    let mut listed = Vec::new();
    for (module, items) in &exported {
        if let Some((_, why)) = NOT_A_WIDGET.iter().find(|(m, _)| m == module) {
            listed.push(format!("  {module:<22} 위젯 아님 — {why}"));
            continue;
        }
        if named(items, &blob) {
            let hit = items
                .iter()
                .find(|i| contains_word(&blob, i))
                .cloned()
                .unwrap_or_default();
            listed.push(format!("  {module:<22} → {hit}"));
        } else {
            missing.push(format!("  {module:<22} 항목 {items:?}"));
        }
    }

    assert!(
        missing.is_empty(),
        "갤러리 코드에서 재수출 항목 이름을 찾지 못한 모듈이 {}개다:\n{}\nADR-0035에 따라 갤러리에 위젯을 추가한다. 위젯이 아니라면 NOT_A_WIDGET에 사유를 적는다. pub use를 pub mod로 바꿔 검사 대상에서 빼지 않는다.",
        missing.len(),
        missing.join("\n")
    );

    println!(
        "[widgets] 재수출 {} 모듈 · 항목 {}\n{}",
        exported.len(),
        exported.iter().map(|(_, i)| i.len()).sum::<usize>(),
        listed.join("\n")
    );
}

#[test]
fn the_pub_module_escape_hatch_stays_closed() {
    let lib = masked(WIDGETS_LIB);
    let mods = pub_modules(&lib);
    assert!(
        !mods.is_empty(),
        "공개 모듈을 찾지 못했다. 선언 추출을 확인한다."
    );
    let unlisted: Vec<&String> = mods
        .iter()
        .filter(|m| !PUB_MODULES.iter().any(|(k, _)| *k == m.as_str()))
        .collect();
    assert!(
        unlisted.is_empty(),
        "PUB_MODULES에 없는 공개 모듈이다: {unlisted:?}. 위젯은 pub use로 재수출하고, 위젯이 아니라면 사유와 함께 명부에 등록한다."
    );
    assert_eq!(
        mods.len(),
        PUB_MODULES.len(),
        "공개 모듈은 {}개인데 명부는 {}행이다. 없어진 공개 모듈의 기록을 제거한다.",
        mods.len(),
        PUB_MODULES.len()
    );
}

#[cfg(test)]
mod detector {
    use super::*;

    const LIB: &str = "\
pub mod brand;
pub use button::{Button, ButtonVariant};
pub use ghost::{GhostWidget};
";

    #[test]
    fn a_widget_the_gallery_never_names_is_reported() {
        let ex = exported_modules(LIB);
        let blob = "fn demo() { Button::new(); }";
        let miss: Vec<&String> = ex
            .iter()
            .filter(|(_, items)| !named(items, blob))
            .map(|(m, _)| m)
            .collect();
        assert_eq!(
            miss,
            vec!["ghost"],
            "항목 이름이 없는 모듈을 검출하지 못했다"
        );
    }

    #[test]
    fn a_widget_the_gallery_names_is_not_reported() {
        let ex = exported_modules(LIB);
        let blob = "fn demo() { Button::new(); GhostWidget::show(); }";
        assert!(
            ex.iter().all(|(_, items)| named(items, blob)),
            "모든 모듈의 항목 이름이 있는데 누락으로 판단했다"
        );
    }

    #[test]
    fn a_name_that_only_appears_in_a_comment_does_not_count() {
        let raw = "// GhostWidget 은 나중에 그린다\nlet s = \"GhostWidget\";\n";
        let blob = mask_non_code(raw);
        let ex = exported_modules(LIB);
        let ghost = ex
            .iter()
            .find(|(m, _)| m == "ghost")
            .expect("합성 입력이 깨졌다");
        assert!(
            !named(&ghost.1, &blob),
            "주석·문자열의 이름을 코드 사용으로 판단했다"
        );
        assert!(
            named(&ghost.1, raw),
            "원문에서 이름을 찾지 못해 마스킹 전후를 비교할 수 없다"
        );
    }

    #[test]
    fn an_alias_is_counted_by_the_name_it_exports() {
        let ex = exported_modules("pub use ghost::{Inner as GhostWidget};");
        assert_eq!(
            ex,
            vec![("ghost".to_string(), vec!["GhostWidget".to_string()])]
        );
        assert!(named(&ex[0].1, "fn demo() { GhostWidget::show(); }"));
    }

    #[test]
    fn a_prefix_is_not_a_name() {
        assert!(!contains_word("ButtonVariant::Ghost", "Button"));
        assert!(contains_word("let b = Button::new();", "Button"));
    }

    #[test]
    fn an_empty_left_side_yields_nothing_which_is_why_the_repo_test_has_a_floor() {
        assert!(exported_modules("").is_empty());
        assert!(pub_modules("").is_empty());
    }
}
