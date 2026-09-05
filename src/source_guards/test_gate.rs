//! 테스트 게이트 판정 — "이 코드가 출하물인가" 를 묻는 소스 스캔 가드들의 공용 술어.
//!
//! 자리가 여럿인 것이 문제가 아니라 **판정기가 여럿인 것**이 문제다. 같은 물음에 사본을
//! 두면 갈린 쪽이 조용히 낡는다 — 이 모듈이 생긴 계기가 그것이다. 세 자리에서 실측으로
//! 드러난 구멍 둘을 여기서 한 번에 닫는다:
//!
//! 1. **합성 조건을 못 봤다.** `#[cfg(test)]` 를 문자열로 비교하면
//!    `#[cfg(all(test, feature = "gui"))]` 가 안 걸린다. 실측 자리는 `src/state.rs` 의
//!    `fullscreen_stage_tests` 이고, 그 파일의 선언들이 출하물로 셈됐다.
//! 2. **`lib.rs` 의 자식 모듈을 못 찾았다.** 자식이 사는 디렉토리를 `mod.rs` 만
//!    특례로 두면 `crates/*/src/lib.rs` 의 `#[cfg(test)] mod tests;` 가 가리키는
//!    형제 파일(그 크레이트 `src/` 밑의 `tests.rs`)을 못 지운다. `lib.rs` 와
//!    `main.rs` 도 `mod.rs` 와 같은 특례다.
//!
//! 둘 다 **거짓 양성**을 만든다 — 테스트 코드를 출하물로 세는 쪽이라, 그 상태의 가드는
//! 고칠 수 없는 위반을 계속 가리킨다.

use std::path::{Path, PathBuf};

use super::mask_non_code;

/// 테스트 게이트 attribute 의 **끝 다음** 바이트 위치들. `#[cfg(test)]` 만이 아니라
/// `#[cfg(all(test, feature = "gui"))]` 같은 합성 조건도 센다 — 실측으로 후자가 있었고
/// (`src/state.rs` 의 `fullscreen_stage_tests`), 리터럴 비교만 하는 판정기는 그 파일을
/// 출하물로 셌다. 조건 안에 `test` 가 **토큰으로** 있으면 테스트 게이트로 본다.
pub(super) fn cfg_test_attr_ends(masked: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = masked[from..].find("#[cfg(") {
        let at = from + rel;
        let body_start = at + "#[cfg(".len();
        from = body_start;
        let mut depth = 1usize;
        let mut end = None;
        for (i, c) in masked[body_start..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(body_start + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = end else { continue };
        let cond = &masked[body_start..close];
        let is_test = cond
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .any(|t| t == "test");
        if is_test {
            // 닫는 `)` 다음의 `]` 까지 건너뛴다.
            let after = masked[close..]
                .char_indices()
                .find(|(_, c)| *c == ']')
                .map_or(close, |(i, c)| close + i + c.len_utf8());
            out.push(after);
        }
        from = close;
    }
    out
}

/// 테스트 게이트가 걸린 `mod ... { ... }` 블록을 줄 구조를 보존한 채 지운다. 중괄호가
/// 없는 `mod name;` 은 별도 파일이라 [`test_gated_modules`] 가 따로 뺀다.
pub(super) fn blank_test_modules(masked: &str) -> String {
    let bytes: Vec<char> = masked.chars().collect();
    let mut out: String = masked.to_string();
    for from in cfg_test_attr_ends(masked) {
        let Some(open) = masked[from..].find('{').map(|o| from + o) else {
            continue;
        };
        // 여는 중괄호 앞에 세미콜론이 있으면 그 attribute 는 블록이 아니다.
        if masked[from..open].contains(';') {
            continue;
        }
        let mut depth = 0usize;
        let mut close = None;
        for (i, c) in bytes
            .iter()
            .enumerate()
            .skip(masked[..open].chars().count())
        {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close_char) = close else { continue };
        let start_byte = open;
        let end_byte = masked
            .char_indices()
            .nth(close_char)
            .map_or(masked.len(), |(b, c)| b + c.len_utf8());
        let region: String = masked[start_byte..end_byte]
            .chars()
            .map(|c| if c == '\n' { '\n' } else { ' ' })
            .collect();
        out.replace_range(start_byte..end_byte, &region);
    }
    out
}

/// `#[cfg(test)] mod NAME;` 로 선언된 자식 모듈 이름들. 그 모듈의 파일 전체가 테스트다.
pub(super) fn test_gated_modules(masked: &str) -> Vec<String> {
    let mut out = Vec::new();
    for from in cfg_test_attr_ends(masked) {
        let rest = masked[from..].trim_start();
        let Some(rest) = rest.strip_prefix("mod ") else {
            continue;
        };
        let Some((name, _)) = rest.split_once(';') else {
            continue;
        };
        if name
            .trim()
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            out.push(name.trim().to_string());
        }
    }
    out
}

/// 자식 모듈이 사는 디렉토리. `mod.rs`·`lib.rs`·`main.rs` 는 자기 부모가 그 자리이고,
/// 그 밖의 `foo.rs` 는 `foo/` 다.
fn child_module_dir(rel: &Path) -> PathBuf {
    let is_root = rel
        .file_name()
        .is_some_and(|n| n == "mod.rs" || n == "lib.rs" || n == "main.rs");
    if is_root {
        rel.parent().unwrap_or(Path::new("")).to_path_buf()
    } else {
        rel.with_extension("")
    }
}

/// 테스트 게이트가 걸린 `mod NAME;` 이 가리키는 **파일 경로**들(레포 상대, `/` 구분).
///
/// 입력은 [`super::rust_sources`] 가 내는 (경로, 원문) 목록 그대로다.
pub(super) fn test_gated_files(files: &[(PathBuf, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (rel, text) in files {
        let dir = child_module_dir(rel).to_string_lossy().replace('\\', "/");
        for name in test_gated_modules(&mask_non_code(text)) {
            out.push(format!("{dir}/{name}.rs"));
            out.push(format!("{dir}/{name}/mod.rs"));
        }
    }
    out
}

#[cfg(test)]
mod detector {
    use super::*;

    #[test]
    fn it_names_test_gated_child_modules() {
        assert_eq!(
            test_gated_modules("mod a;\n#[cfg(test)]\nmod b;\nmod c;\n"),
            vec!["b".to_string()]
        );
    }

    /// 합성 조건도 테스트 게이트다. 문자열 비교로 판정하면 이 형태가 통째로 샌다 —
    /// 실측 자리는 `src/state.rs` 의 `fullscreen_stage_tests` 였다.
    #[test]
    fn a_composite_cfg_that_mentions_test_is_still_a_test_gate() {
        assert_eq!(
            test_gated_modules("#[cfg(all(test, feature = \"gui\"))]\nmod b;\n"),
            vec!["b".to_string()]
        );
        assert_eq!(
            blank_test_modules("#[cfg(all(test, feature = \"gui\"))]\nmod b {\n    X\n}\n")
                .contains('X'),
            false
        );
    }

    /// `test` 를 **토큰으로** 본다. 이름 안에 우연히 들어 있는 것은 게이트가 아니다.
    #[test]
    fn a_cfg_whose_name_merely_contains_test_is_not_a_gate() {
        assert!(test_gated_modules("#[cfg(feature = \"latest\")]\nmod b;\n").is_empty());
    }

    /// 자식 모듈의 집은 `mod.rs` 만이 아니다. `lib.rs` 를 특례에서 빼면 크레이트마다
    /// 있는 `#[cfg(test)] mod tests;` 가 출하물로 셈된다.
    #[test]
    fn a_crate_root_holds_its_children_beside_itself() {
        let files = vec![(
            PathBuf::from("crates/x/src/lib.rs"),
            "#[cfg(test)]\nmod tests;\n".to_owned(),
        )];
        assert_eq!(
            test_gated_files(&files),
            vec![
                "crates/x/src/tests.rs".to_owned(),
                "crates/x/src/tests/mod.rs".to_owned()
            ]
        );
    }
}
