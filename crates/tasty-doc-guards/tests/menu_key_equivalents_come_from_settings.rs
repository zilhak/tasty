//! NSMenu의 setKeyEquivalent 인자가 빈 문자열이거나 설정 변환에서 왔는지 검사한다.
//! OS 표준 selector라도 단축키는 KeybindingSettings에서 가져와야 한다.
//!
//! 빈 NSString 표현식 또는 호출 이전 let의 우변에 FROM_SETTINGS가 나오는 이름을 인정한다.
//! 파일 텍스트에서 이름만 비교하므로 함수 범위·shadowing·변환 함수의 내부를 알지 못하며,
//! 어느 binding 필드를 썼는지도 확인하지 않는다. 이 검사는 macOS의 NSMenu API만 대상으로 한다.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tasty_doc_guards::temp_scratch::Scratch;

/// 설정에서 키를 뽑는 변환 함수. 이 이름을 거친 값만 "설정에서 왔다" 로 센다.
const FROM_SETTINGS: &str = "binding_to_nsmenu_key";

/// 호출을 찾지 못한 상태로 통과하지 않게 한다. 실제 메뉴가 줄었다면 호출 수를 다시 확인한다.
const MIN_SITES: usize = 4;

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Source {
    /// 빈 문자열 리터럴.
    Empty,
    /// 호출 이전 let에서 FROM_SETTINGS 이름을 사용해 얻은 값.
    Settings,
    /// 그 밖 — 위반 후보.
    Unknown,
}

/// 중첩 괄호를 세어 호출 인자를 추출한다.
fn argument_at(text: &str, open: usize) -> Option<&str> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[open + 1..i].trim());
                }
            }
            _ => {}
        }
    }
    None
}

/// 호출 이전 텍스트에 해당 이름과 FROM_SETTINGS를 쓰는 let이 있는지 본다.
fn bound_from_settings(before: &str, name: &str) -> bool {
    for (idx, _) in before.match_indices("let ") {
        let Some(eq) = before[idx..].find('=') else {
            continue;
        };
        let lhs = &before[idx..idx + eq];
        if !lhs
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .any(|t| t == name)
        {
            continue;
        }
        let rhs_end = before[idx + eq..]
            .find(';')
            .map_or(before.len(), |e| idx + eq + e);
        if before[idx + eq..rhs_end].contains(FROM_SETTINGS) {
            return true;
        }
    }
    false
}

fn sites_in(text: &str) -> Vec<(usize, Source)> {
    let needle = "setKeyEquivalent";
    let mut out = Vec::new();
    for (idx, _) in text.match_indices(needle) {
        let Some(open) = text[idx..].find('(').map(|o| idx + o) else {
            continue;
        };
        if text[idx + needle.len()..open]
            .chars()
            .any(|c| c.is_alphanumeric())
        {
            continue;
        }
        let Some(arg) = argument_at(text, open) else {
            continue;
        };
        let line = text[..idx].matches('\n').count() + 1;
        let bare = arg.trim_start_matches('&').trim();
        let source = if bare.replace(' ', "") == "NSString::from_str(\"\")" {
            Source::Empty
        } else if bare.chars().all(|c| c.is_alphanumeric() || c == '_')
            && bound_from_settings(&text[..idx], bare)
        {
            Source::Settings
        } else {
            Source::Unknown
        };
        out.push((line, source));
    }
    out
}

#[test]
fn every_menu_key_equivalent_is_empty_or_from_settings() {
    let root = repo_root();
    let mut files = Vec::new();
    rs_files(&root.join("src"), &mut files);
    rs_files(&root.join("crates/tasty-platform/src"), &mut files);

    let mut seen = 0usize;
    let mut violations = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let rel =
            tasty_doc_guards::source_text::repo_relative(file.strip_prefix(&root).unwrap_or(file))
                .display()
                .to_string();
        for (line, source) in sites_in(&text) {
            seen += 1;
            if source == Source::Unknown {
                violations.push(format!("  {rel}:{line}"));
            }
        }
    }

    assert!(
        seen >= MIN_SITES,
        "setKeyEquivalent 호출을 {seen}곳만 찾았다(하한 {MIN_SITES}). 스캔 경로와 API 호출 형태를 확인하고, 실제 메뉴가 줄었다면 다시 측정한 뒤 하한을 갱신한다."
    );

    assert!(
        violations.is_empty(),
        "NSMenu 단축키가 설정 변환이나 빈 문자열에서 오지 않는다:\n{}\n대응하는 KeybindingSettings 필드를 {}로 변환하거나 단축키를 비운다. OS 표준 selector도 예외가 아니다. 이 검사는 이름을 비교하며 함수 범위·변환 함수 내부·binding 필드의 정확성은 확인하지 않는다.",
        violations.join("\n"),
        FROM_SETTINGS
    );
}

#[test]
fn the_reader_answers_both_yes_and_no() {
    let settings = "let (quit_key, mods) = binding_to_nsmenu_key(b);\n\
                    item.setKeyEquivalent(&quit_key);\n";
    assert_eq!(sites_in(settings), vec![(2, Source::Settings)]);

    let empty = "item.setKeyEquivalent(&NSString::from_str(\"\"));\n";
    assert_eq!(sites_in(empty), vec![(1, Source::Empty)]);

    let hardcoded = "item.setKeyEquivalent(&NSString::from_str(\"q\"));\n";
    assert_eq!(sites_in(hardcoded), vec![(1, Source::Unknown)]);

    let unbound = "item.setKeyEquivalent(&some_key);\n";
    assert_eq!(sites_in(unbound), vec![(1, Source::Unknown)]);
}

#[test]
fn the_modifier_mask_setter_is_a_different_question() {
    let text = "item.setKeyEquivalentModifierMask(NSEventModifierFlags::empty());\n";
    assert!(sites_in(text).is_empty());
}

#[test]
fn a_binding_after_the_call_does_not_count() {
    let text = "item.setKeyEquivalent(&key);\n\
                let (key, _) = binding_to_nsmenu_key(b);\n";
    assert_eq!(sites_in(text), vec![(1, Source::Unknown)]);
}

#[test]
fn the_registration_sites_live_in_one_place() {
    let root = repo_root();
    let mut files = Vec::new();
    rs_files(&root.join("src"), &mut files);
    rs_files(&root.join("crates/tasty-platform/src"), &mut files);
    let mut owners = BTreeSet::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        if !sites_in(&text).is_empty() {
            // 플랫폼에 관계없이 같은 상대 경로로 비교한다.
            owners.insert(
                tasty_doc_guards::source_text::repo_relative(
                    file.strip_prefix(&root).unwrap_or(file),
                )
                .display()
                .to_string(),
            );
        }
    }
    assert!(
        owners.len() == 1 && owners.iter().all(|o| o.contains("macos")),
        "NSMenu key equivalent 호출이 macOS 한 파일 밖에도 있다: {owners:?}. 새 위치의 단축키 정책과 검사 범위를 확인하고 이 단정을 갱신한다."
    );
}

/// 확장자 필터가 넓어지면 문서의 API 인용을 코드로 오인하므로 합성 트리의 정확한 파일 목록을 비교한다.
#[test]
fn the_walk_recurses_and_takes_only_rust_files() {
    let probe = Scratch::new("menu-key-walk");
    let dir = probe.path();
    std::fs::create_dir_all(dir.join("deep/deeper")).expect("합성 트리를 만들지 못했다");

    std::fs::write(dir.join("top.rs"), "// zeta\n").expect("합성 .rs 실패");
    std::fs::write(dir.join("deep/mid.rs"), "// omega\n").expect("합성 하위 .rs 실패");
    std::fs::write(dir.join("deep/deeper/leaf.rs"), "// sigma\n").expect("합성 말단 .rs 실패");
    std::fs::write(
        dir.join("notes.md"),
        "setKeyEquivalent(\"x\") 를 설명하는 문서\n",
    )
    .expect("합성 문서 실패");
    std::fs::write(dir.join("deep/table.txt"), "setKeyEquivalent(\"y\")\n")
        .expect("합성 잡파일 실패");
    std::fs::write(dir.join("Makefile"), "all:\n").expect("합성 무확장자 실패");

    let mut found = Vec::new();
    rs_files(dir, &mut found);
    let mut rels: Vec<String> = found
        .iter()
        .map(|p| {
            tasty_doc_guards::source_text::repo_relative(p.strip_prefix(&dir).unwrap_or(p))
                .display()
                .to_string()
        })
        .collect();
    rels.sort();
    assert_eq!(
        rels,
        vec![
            "deep/deeper/leaf.rs".to_string(),
            "deep/mid.rs".to_string(),
            "top.rs".to_string(),
        ],
        "순회가 합성 트리에서 다른 답을 냈다"
    );
    assert!(
        !rels
            .iter()
            .any(|r| r.ends_with(".md") || r.ends_with(".txt")),
        "Rust 파일이 아닌 문서까지 수집했다. 문서의 API 인용을 코드 위반으로 오인할 수 있다."
    );
    assert!(
        !rels.iter().any(|r| r.ends_with("Makefile")),
        "확장자 없는 파일을 모았다"
    );

    let mut dead = Vec::new();
    rs_files(&dir.join("nonexistent-zeta"), &mut dead);
    assert!(
        dead.is_empty(),
        "없는 뿌리에서 무언가를 모았다 — 순회가 다른 자리를 보고 있다"
    );
}
