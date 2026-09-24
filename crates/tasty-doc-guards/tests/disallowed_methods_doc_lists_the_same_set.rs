//! clippy.toml의 disallowed-methods와 theme.md의 차단 함수 표를 대조한다.
//! 크레이트 경로를 생략한 문서 표기와 비교하려고 마지막 타입·메서드 두 부분을 사용한다.
//! 같은 이름의 메서드가 여러 타입에 있어 메서드 이름만으로 비교하면 안 된다.
//! 문서의 타입 생략과 중괄호 축약은 펼쳐 읽고, 해석하지 못하면 실패한다.
//! 지정한 절의 첫 표만 검사한다. 대체 방법·사유 설명과 실제 Clippy 실행 여부는 검사하지 않는다.

use std::collections::BTreeSet;

const MANIFEST: &str = "clippy.toml";
const DOC: &str = "docs/design/systems/theme.md";
const SECTION: &str = "\n### clippy 강제 — disallowed-methods";

/// 2026-09-08 실측6항목을 기준으로 둔다. 양쪽 수집이 함께 비어 집합 비교만 통과하지 않도록 확인한다.
const MIN_ENTRIES: usize = 4;

/// `(타입, 메서드)` — 두 표기가 만나는 좌표.
type Coord = (String, String);

fn read(rel: &str) -> String {
    let path = tasty_doc_guards::repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("읽을 수 없다: {} — {e}", path.display()))
}

/// 경로의 마지막 두 마디를 좌표로 만든다.
fn coord(path: &str) -> Coord {
    let seg: Vec<&str> = path.split("::").collect();
    assert!(
        seg.len() >= 2,
        "`{path}` 에 `::` 가 없다 — (타입, 메서드) 좌표를 만들 수 없다"
    );
    (
        seg[seg.len() - 2].to_string(),
        seg[seg.len() - 1].to_string(),
    )
}

/// clippy.toml의 disallowed-methods 배열에서 path 값을 읽는다.
fn manifest_coords(toml: &str) -> BTreeSet<Coord> {
    let body = toml
        .split_once("disallowed-methods = [")
        .unwrap_or_else(|| panic!("`{MANIFEST}` 에서 `disallowed-methods` 배열을 못 찾았다"))
        .1;
    let body = body
        .split_once("\n]")
        .unwrap_or_else(|| panic!("`{MANIFEST}` 의 `disallowed-methods` 배열이 안 닫혔다"))
        .0;
    let mut out = BTreeSet::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some(rest) = trimmed.split_once("path = \"") else {
            panic!("`disallowed-methods` 안에서 모르는 줄을 만났다: `{trimmed}`");
        };
        let path = rest
            .1
            .split_once('"')
            .unwrap_or_else(|| panic!("`path = \"` 가 안 닫혔다: `{trimmed}`"))
            .0;
        out.insert(coord(path));
    }
    out
}

/// 중괄호로 적은 여러 메서드를 펼친다.
fn expand_braces(spec: &str) -> Vec<String> {
    let Some((head, rest)) = spec.split_once('{') else {
        return vec![spec.to_string()];
    };
    let (inner, tail) = rest
        .split_once('}')
        .unwrap_or_else(|| panic!("중괄호가 안 닫혔다: `{spec}` — 축약을 못 펴면 판정 불가다"));
    inner
        .split(',')
        .map(|alt| format!("{head}{}{tail}", alt.trim()))
        .collect()
}

/// 지정된 절의 첫 표에서 차단 함수 열을 읽는다.
fn doc_coords(md: &str) -> BTreeSet<Coord> {
    let body = md
        .split_once(SECTION)
        .unwrap_or_else(|| panic!("`{SECTION}` 절을 못 찾았다 — 제목이 바뀌었으면 여기를 고쳐라"))
        .1;
    // ##·### 제목에서 대상 절을 끊는다.
    let body = body.split("\n## ").next().unwrap_or(body);
    let body = body.split("\n### ").next().unwrap_or(body);

    let mut out = BTreeSet::new();
    let mut started = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            if started {
                break;
            }
            continue;
        }
        started = true;
        let cells: Vec<&str> = trimmed
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.len() < 2 || cells[0] == "차단 함수" || cells[0].starts_with("---") {
            continue;
        }
        // 같은 셀에서 생략한 타입은 앞 메서드의 타입을 이어받는다.
        let mut carried: Option<String> = None;
        for span in cells[0].split('`').skip(1).step_by(2) {
            for spec in expand_braces(span) {
                let full = if spec.contains("::") {
                    let c = coord(&spec);
                    carried = Some(c.0.clone());
                    c
                } else {
                    let ty = carried.clone().unwrap_or_else(|| {
                        panic!("`{spec}` 이 타입 없이 나왔는데 이어받을 것이 없다: `{trimmed}`")
                    });
                    (ty, spec.clone())
                };
                out.insert(full);
            }
        }
    }
    out
}

fn mismatches(doc: &BTreeSet<Coord>, manifest: &BTreeSet<Coord>) -> Vec<String> {
    let mut out = Vec::new();
    for (ty, m) in manifest.difference(doc) {
        out.push(format!(
            "  `{MANIFEST}` 에만 있다: `{ty}::{m}` — 문서 표가 부분 사본이다"
        ));
    }
    for (ty, m) in doc.difference(manifest) {
        out.push(format!(
            "  `{DOC}` 에만 있다: `{ty}::{m}` — 차단되지 않는 함수를 차단된다고 적었다"
        ));
    }
    out
}

#[test]
fn the_doc_table_lists_exactly_the_disallowed_methods() {
    let manifest = manifest_coords(&read(MANIFEST));
    let doc = doc_coords(&read(DOC));

    assert!(
        manifest.len() >= MIN_ENTRIES,
        "{MANIFEST}에서 {}개만 읽었다(하한 {MIN_ENTRIES}). 하한을 낮추기 전에 목록과 판독을 확인한다.",
        manifest.len()
    );

    let wrong = mismatches(&doc, &manifest);
    assert!(
        wrong.is_empty(),
        "{DOC}의 차단 함수 표가 {MANIFEST}의 disallowed-methods와 다르다. 목록을 바꿀 때 문서도 함께 갱신한다.\n문서 {}·매니페스트 {}\n{}",
        doc.len(),
        manifest.len(),
        wrong.join("\n")
    );
}

mod disallowed_mutations {
    use super::*;

    fn real() -> (BTreeSet<Coord>, BTreeSet<Coord>) {
        (doc_coords(&read(DOC)), manifest_coords(&read(MANIFEST)))
    }

    #[test]
    fn the_real_pair_agrees() {
        let (doc, manifest) = real();
        assert_eq!(mismatches(&doc, &manifest), Vec::<String>::new());
    }

    #[test]
    fn a_new_disallowed_method_missing_from_the_doc_is_caught() {
        let (doc, mut manifest) = real();
        manifest.insert(("Color32".into(), "from_additive".into()));
        let found = mismatches(&doc, &manifest);
        assert_eq!(
            found.len(),
            1,
            "매니페스트에 추가한 메서드를 검출하지 못했다: {found:?}"
        );
        assert!(found[0].contains("부분 사본"), "{found:?}");
    }

    #[test]
    fn a_row_deleted_from_the_doc_is_caught() {
        let (mut doc, manifest) = real();
        let key = doc.iter().next().expect("문서 표가 비었다").clone();
        doc.remove(&key);
        let found = mismatches(&doc, &manifest);
        assert_eq!(
            found.len(),
            1,
            "문서에서 빠진 메서드를 검출하지 못했다: {found:?}"
        );
    }

    #[test]
    fn a_doc_only_entry_is_caught() {
        let (mut doc, manifest) = real();
        doc.insert(("Color32".into(), "from_nothing".into()));
        let found = mismatches(&doc, &manifest);
        assert_eq!(
            found.len(),
            1,
            "문서에만 추가된 메서드를 검출하지 못했다: {found:?}"
        );
        assert!(found[0].contains("차단되지 않는"), "{found:?}");
    }

    #[test]
    fn the_method_name_alone_would_fold_six_into_five() {
        let (_, manifest) = real();
        let names: BTreeSet<&String> = manifest.iter().map(|(_, m)| m).collect();
        assert!(
            names.len() < manifest.len(),
            "메서드 이름만으로도 중복이 없다. 타입을 함께 비교해야 한다는 테스트 전제를 확인한다."
        );
    }

    // 파서 오류별 합성 입력에서 원인을 구별하는 진단 부분을 확인한다.

    #[test]
    #[should_panic(expected = "배열을 못 찾았다")]
    fn a_missing_array_says_the_key_moved() {
        manifest_coords("[lints]\n");
    }

    #[test]
    #[should_panic(expected = "배열이 안 닫혔다")]
    fn an_unclosed_array_says_so() {
        manifest_coords("disallowed-methods = [\n    { path = \"A::b\" },");
    }

    #[test]
    #[should_panic(expected = "모르는 줄을 만났다")]
    fn an_entry_without_a_path_key_is_not_silently_skipped() {
        manifest_coords("disallowed-methods = [\n    { paff = \"A::b\" },\n]\n");
    }

    #[test]
    #[should_panic(expected = "` 가 안 닫혔다")]
    fn an_unterminated_path_string_says_so() {
        manifest_coords("disallowed-methods = [\n    { path = \"A::b },\n]\n");
    }

    #[test]
    #[should_panic(expected = "`::` 가 없다")]
    fn a_path_without_a_type_segment_says_so() {
        coord("from_rgb");
    }

    #[test]
    #[should_panic(expected = "중괄호가 안 닫혔다")]
    fn an_unclosed_brace_shorthand_says_so() {
        expand_braces("A::from_rgba_{unmultiplied");
    }

    #[test]
    #[should_panic(expected = "절을 못 찾았다")]
    fn a_missing_doc_section_says_the_heading_moved() {
        doc_coords("## 다른 절\n\n| 차단 함수 | 대체 |\n");
    }

    #[test]
    fn braces_expand() {
        assert_eq!(
            expand_braces("egui::Color32::from_rgba_{unmultiplied,premultiplied}"),
            vec![
                "egui::Color32::from_rgba_unmultiplied".to_string(),
                "egui::Color32::from_rgba_premultiplied".to_string(),
            ]
        );
        assert_eq!(expand_braces("A::b"), vec!["A::b".to_string()]);
    }

    #[test]
    fn the_doc_reader_carries_the_type_across_a_slash() {
        let md = "## 색 생성 정책\n\n### clippy 강제 — disallowed-methods\n\n| 차단 함수 | 대체 |\n|---|---|\n\
                  | `HexColor::from_rgb` / `from_rgba` | x |\n\n### 다음\n";
        let got = doc_coords(md);
        assert!(
            got.contains(&("HexColor".into(), "from_rgba".into())),
            "{got:?}"
        );
    }

    #[test]
    #[should_panic(expected = "이어받을 것이 없다")]
    fn a_bare_method_with_no_type_to_carry_is_not_silently_skipped() {
        let md = "## 색 생성 정책\n\n### clippy 강제 — disallowed-methods\n\n| 차단 함수 | 대체 |\n|---|---|\n\
                  | `from_rgba` | x |\n\n### 다음\n";
        doc_coords(md);
    }
}
