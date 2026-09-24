//! clippy-policy의 현재 워크스페이스 설정 표를 Cargo.toml의 workspace.lints 전체와 대조한다.
//! lint 이름·레벨이 한쪽에만 있거나 다르면 실패한다. clippy와 rust의 lint를 모두 포함한다.
//!
//! clippy.toml의 임계값, 표 밖의 산문, 개별 allow 예외와 멤버의 lint 상속 여부는 여기서 검사하지 않는다.
//! 표를 여러 줄로 나눠 깨뜨린 경우에는 일부 행을 놓칠 수 있어 Markdown 표 검사도 필요하다.
//! Cargo의 표 형식 레벨처럼 파서가 모르는 값은 건너뛰지 않고 실패시킨다.

use std::collections::BTreeMap;

const DOC: &str = "docs/dev-guide/clippy-policy.md";
const MANIFEST: &str = "Cargo.toml";

/// 검사할 표의 절 제목. 찾지 못하면 실패한다.
const SECTION: &str = "## 현재 워크스페이스 설정";

/// `[workspace.lints.<절>]` 의 절 머리 접두.
const LINTS_PREFIX: &str = "[workspace.lints.";

/// 표의 위치 칸이 매니페스트 절을 가리킬 때의 형태 — `` `Cargo.toml [workspace.lints.rust]` ``.
const LOCATION_PREFIX: &str = "Cargo.toml [workspace.lints.";

/// 2026-09-08 측정12개(clippy7/rust5)보다 낮게 둔 하한. 실제 감소와 파싱 실패를 확인한다.
const MIN_LINTS: usize = 8;

/// `(절, lint 이름)` → 레벨.
type Levels = BTreeMap<(String, String), String>;

fn read(rel: &str) -> String {
    let path = tasty_doc_guards::repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("읽을 수 없다: {} — {e}", path.display()))
}

/// cargo 가 받는 레벨 이름. 그 밖의 값은 판독 실패다.
fn is_level(v: &str) -> bool {
    matches!(v, "allow" | "warn" | "deny" | "forbid")
}

fn manifest_levels(toml: &str) -> Levels {
    let mut out = Levels::new();
    let mut section: Option<String> = None;
    for line in toml.lines() {
        // 주석의 절 이름을 읽지 않도록 줄 시작의 헤더만 인식한다.
        if line.starts_with('[') {
            section = line
                .trim_end()
                .strip_prefix(LINTS_PREFIX)
                .and_then(|s| s.strip_suffix(']'))
                .map(str::to_string);
            continue;
        }
        let Some(sec) = section.clone() else { continue };
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((name, value)) = trimmed.split_once('=') else {
            panic!("`[workspace.lints.{sec}]` 안에서 모르는 줄을 만났다: `{trimmed}`");
        };
        let level = value
            .split('#')
            .next()
            .unwrap_or_default()
            .trim()
            .trim_matches('"')
            .to_string();
        assert!(
            is_level(&level),
            "workspace.lints.{sec}의 {} 레벨을 못 읽었다: {level}. Cargo의 표 형식 값이라면 파서 지원을 추가한다. 모르는 항목을 건너뛰지 않는다.",
            name.trim()
        );
        out.insert((sec, name.trim().to_string()), level);
    }
    out
}

/// 지정한 절의 첫 표에서 lint와 레벨을 읽는다.
fn doc_levels(md: &str) -> Levels {
    let body = md
        .split_once(SECTION)
        .unwrap_or_else(|| panic!("`{SECTION}` 절을 못 찾았다 — 제목이 바뀌었으면 여기를 고쳐라"))
        .1;
    let body = body.split("\n## ").next().unwrap_or(body);

    let mut out = Levels::new();
    // `〃` 는 바로 위 행의 위치를 잇는다. 이어받을 것이 없으면 판독 실패다.
    let mut location: Option<String> = None;
    // 뒤에 다른 표가 추가돼도 섞이지 않도록 첫 연속 표 블록만 읽는다.
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
        // 세 칸 미만인 행은 건너뛴다. 표의 열 수 검사는 별도 가드가 맡는다.
        if cells.len() < 3 || cells[0] == "위치" || cells[0].starts_with("---") {
            continue;
        }
        let raw_location = cells[0].trim_matches('`');
        if raw_location != "〃" {
            location = Some(raw_location.to_string());
        }
        let here = location
            .clone()
            .unwrap_or_else(|| panic!("표 첫 행이 `〃` 다 — 이어받을 위치가 없다: `{trimmed}`"));

        // lint 레벨 외의 설정은 별도 소절에서 설명해야 한다.
        let Some(sec) = here
            .strip_prefix(LOCATION_PREFIX)
            .and_then(|s| s.strip_suffix(']'))
        else {
            panic!(
                "현재 워크스페이스 설정 표에 매니페스트 절을 안 가리키는 행이 있다: {here}. lint 레벨이 아닌 설정은 clippy.toml 소절로 옮긴다."
            );
        };

        let setting = cells[1].trim_matches('`');
        let Some((name, level)) = setting.split_once('=') else {
            panic!("{here} 행의 설정 칸에 레벨이 없다: {setting}");
        };
        out.insert(
            (sec.to_string(), name.trim().to_string()),
            level.trim().trim_matches('"').to_string(),
        );
    }
    out
}

fn mismatches(doc: &Levels, manifest: &Levels) -> Vec<String> {
    let mut out = Vec::new();
    for (key, level) in doc {
        match manifest.get(key) {
            None => out.push(format!(
                "  표에만 있다: `[workspace.lints.{}]` 의 `{}` — 문서가 없는 정책을 주장한다",
                key.0, key.1
            )),
            Some(actual) if actual != level => out.push(format!(
                "  레벨이 다르다: `[workspace.lints.{}]` 의 `{}` — 표는 \"{level}\", \
                 매니페스트는 \"{actual}\" (정본은 매니페스트다)",
                key.0, key.1
            )),
            Some(_) => {}
        }
    }
    for (key, level) in manifest {
        if !doc.contains_key(key) {
            out.push(format!(
                "  매니페스트에만 있다: `[workspace.lints.{}]` 의 `{}` = \"{level}\" — \
                 표가 부분 사본이다",
                key.0, key.1
            ));
        }
    }
    out
}

#[test]
fn the_table_and_the_manifest_hold_the_same_lint_levels() {
    let manifest = manifest_levels(&read(MANIFEST));
    let doc = doc_levels(&read(DOC));

    assert!(
        manifest.len() >= MIN_LINTS,
        "workspace.lints에서 {}개만 읽었다(하한 {MIN_LINTS}). 실제 설정과 파싱 범위를 확인한다.",
        manifest.len()
    );

    let wrong = mismatches(&doc, &manifest);
    assert!(
        wrong.is_empty(),
        "{DOC}의 현재 워크스페이스 설정 표와 {MANIFEST}의 workspace.lints가 다르다. lint를 바꿀 때 표도 갱신한다. 표{}행, 매니페스트{}항목:\n{}",
        doc.len(),
        manifest.len(),
        wrong.join("\n")
    );
}

mod parity_mutations {
    use super::*;

    fn real() -> (Levels, Levels) {
        (doc_levels(&read(DOC)), manifest_levels(&read(MANIFEST)))
    }

    #[test]
    fn the_real_pair_agrees() {
        let (doc, manifest) = real();
        assert_eq!(mismatches(&doc, &manifest), Vec::<String>::new());
    }

    #[test]
    fn a_changed_level_is_caught() {
        let (doc, manifest) = real();
        let mut mutated = doc.clone();
        let key = manifest
            .keys()
            .find(|k| manifest[*k] != "warn")
            .expect("모든 레벨이 warn 일 리 없다")
            .clone();
        mutated.insert(key, "warn".into());
        let found = mismatches(&mutated, &manifest);
        assert_eq!(found.len(), 1, "레벨 변이를 못 물었다: {found:?}");
        assert!(found[0].contains("레벨이 다르다"), "{found:?}");
    }

    #[test]
    fn a_row_deleted_from_the_table_is_caught() {
        let (doc, manifest) = real();
        let mut mutated = doc.clone();
        let key = doc.keys().next().expect("표가 비었다").clone();
        mutated.remove(&key);
        let found = mismatches(&mutated, &manifest);
        assert_eq!(found.len(), 1, "행 삭제를 못 물었다: {found:?}");
        assert!(found[0].contains("매니페스트에만 있다"), "{found:?}");
    }

    #[test]
    fn a_lint_removed_from_the_manifest_is_caught() {
        let (doc, manifest) = real();
        let mut mutated = manifest.clone();
        let key = manifest.keys().next().expect("매니페스트가 비었다").clone();
        mutated.remove(&key);
        let found = mismatches(&doc, &mutated);
        assert_eq!(found.len(), 1, "우변 삭제를 못 물었다: {found:?}");
        assert!(found[0].contains("표에만 있다"), "{found:?}");
    }

    // 합성 입력으로 파싱 실패를 일으켜 입력 형식에 맞는 안내가 나오는지 확인한다.

    #[test]
    #[should_panic(expected = "모르는 줄을 만났다")]
    fn an_unparsable_line_in_the_lints_section_says_so() {
        manifest_levels("[workspace.lints.clippy]\nbogus\n");
    }

    #[test]
    #[should_panic(expected = "절을 못 찾았다")]
    fn a_missing_section_heading_says_the_heading_moved() {
        doc_levels("## 다른 절\n\n| 위치 | 설정 | 의미 |\n");
    }

    #[test]
    #[should_panic(expected = "이어받을 위치가 없다")]
    fn a_ditto_in_the_first_row_says_there_is_nothing_to_carry() {
        doc_levels(
            "## 현재 워크스페이스 설정\n\n| 위치 | 설정 | 의미 |\n|---|---|---|\n\
             | 〃 | `a = \"deny\"` | x |\n\n## 다음\n",
        );
    }

    #[test]
    #[should_panic(expected = "설정 칸에 레벨이 없다")]
    fn a_manifest_row_without_a_level_is_not_silently_skipped() {
        doc_levels(
            "## 현재 워크스페이스 설정\n\n| 위치 | 설정 | 의미 |\n|---|---|---|\n\
             | `Cargo.toml [workspace.lints.rust]` | `a` | x |\n\n## 다음\n",
        );
    }

    #[test]
    fn the_manifest_reader_does_not_read_commented_out_lints() {
        let toml = "[workspace.lints.clippy]\n# dead_code = \"deny\"\nfoo = \"warn\"\n";
        let got = manifest_levels(toml);
        assert_eq!(got.len(), 1, "주석을 항목으로 셌다: {got:?}");
    }

    #[test]
    fn the_manifest_reader_stops_at_the_next_section() {
        let toml = "[workspace.lints.rust]\nfoo = \"deny\"\n[package]\nname = \"x\"\n";
        let got = manifest_levels(toml);
        assert_eq!(got.len(), 1, "절 밖의 키를 읽었다: {got:?}");
    }

    #[test]
    #[should_panic(expected = "레벨을 못 읽었다")]
    fn a_table_form_level_is_not_silently_skipped() {
        manifest_levels("[workspace.lints.rust]\ndead_code = { level = \"deny\" }\n");
    }

    #[test]
    fn the_doc_reader_carries_the_ditto_mark() {
        let md = "## 현재 워크스페이스 설정\n\n| 위치 | 설정 | 의미 |\n|---|---|---|\n\
                  | `Cargo.toml [workspace.lints.rust]` | `a = \"deny\"` | x |\n\
                  | 〃 | `b = \"warn\"` | y |\n\n## 다음\n";
        let got = doc_levels(md);
        assert_eq!(got.len(), 2, "`〃` 를 못 이어받았다: {got:?}");
        assert_eq!(got[&("rust".into(), "b".into())], "warn");
    }

    #[test]
    #[should_panic(expected = "매니페스트 절을 안 가리키는 행")]
    fn a_row_outside_the_modulus_is_not_silently_skipped() {
        let md = "## 현재 워크스페이스 설정\n\n| 위치 | 설정 | 의미 |\n|---|---|---|\n\
                  | `clippy.toml` | `disallowed-methods` | x |\n\n## 다음\n";
        doc_levels(md);
    }

    #[test]
    fn the_doc_reader_stops_at_the_end_of_the_first_table() {
        let md = "## 현재 워크스페이스 설정\n\n| 위치 | 설정 | 의미 |\n|---|---|---|\n\
                  | `Cargo.toml [workspace.lints.rust]` | `a = \"deny\"` | x |\n\n\
                  본문 한 줄.\n\n| 위치 | 설정 | 의미 |\n|---|---|---|\n\
                  | `clippy.toml` | `b` | y |\n\n## 다음\n";
        let got = doc_levels(md);
        assert_eq!(got.len(), 1, "둘째 표까지 빨아들였다: {got:?}");
    }
}
