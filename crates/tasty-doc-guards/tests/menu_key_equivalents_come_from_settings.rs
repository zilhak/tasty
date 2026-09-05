//! **NSMenu 항목의 단축키는 설정에서 오거나 비어 있어야 한다** — 그 둘 밖을 잡는다.
//!
//! 프로젝트 규칙(`CLAUDE.md` "단축키")은 tasty 가 직접 등록하는 모든 메뉴 항목의 key
//! equivalent 를 `KeybindingSettings` 의 binding 에서 가져오거나 **비우라**고 요구한다.
//! selector 가 OS 표준(`performClose:` 등)이라는 사실은 단축키 하드코딩의 정당화가 되지
//! 않는다 — selector 와 단축키는 독립적으로 결정한다.
//!
//! 이 규칙은 여태 판정기가 없었다(원칙 명부에서 [구두]로 분류돼 있었다). 만들 수 있는지를
//! 먼저 쟀고, 값이 나왔으므로 만든다: 호출 지점이 **넷**이고 전부 한 파일에 있으며 셋은
//! 설정에서, 하나는 빈 문자열에서 온다.
//!
//! # 판별식 — 이름이 아니라 인자의 출처
//!
//! `setKeyEquivalent(` 의 인자를 보고 셋으로 가른다:
//!
//! - `NSString::from_str("")` — **빈 값**. 정책이 허용하는 한쪽.
//! - 같은 함수 안에서 [`FROM_SETTINGS`] 를 거쳐 묶인 이름 — **설정에서 온 값**. 다른 한쪽.
//! - 그 밖 전부 — 위반 후보. 리터럴 `"q"` 를 넣는 형태가 여기 걸린다.
//!
//! 한 홉만 따라간다(그 이름이 묶인 `let` 한 줄). 두 홉 이상 — 예컨대 하드코딩한 키를
//! 반환하는 헬퍼를 새로 만들어 그것을 부르는 형태 — 는 **통과한다.** 이 사각을 아래
//! "단정하지 않는 것" 에 적어 둔다. 지금 레포에 그런 형태가 없어서 판별식을 더 무겁게
//! 만들 근거가 없다(R501 — 판별식이 먼저다).
//!
//! # 이 가드가 단정하지 않는 것
//!
//! - **두 홉 이상의 우회.** 위 참조.
//! - **그 binding 값이 옳은가.** 설정에서 왔다는 것만 보고, 어느 필드에서 왔는지는 안 본다.
//! - **macOS 밖.** Windows `AcceleratorTable` · Linux 메뉴에는 지금 key equivalent 를
//!   등록하는 자리가 **하나도 없다**(실측). 그래서 이 축의 모수는 그 두 플랫폼에서 0 이고,
//!   거기서의 초록은 "지킨다" 가 아니라 **"잴 것이 없다"** 이다 — 그 구별이 안 보이게
//!   되지 않도록 아래 [`MIN_SITES`] 하한이 모수를 노출한다.
//! - **타입으로 이미 막힌 자리.** `make_std_item` 은 단축키 인자를 아예 안 받아 호출부가
//!   단축키를 박을 수 없다. 그건 스캐너보다 강한 강제이고, 이 가드는 그것을 대체하지
//!   않는다.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 설정에서 키를 뽑는 변환 함수. 이 이름을 거친 값만 "설정에서 왔다" 로 센다.
const FROM_SETTINGS: &str = "binding_to_nsmenu_key";

/// 훑어야 할 최소 호출 지점 수 — **모수가 살아 있다는 증거**.
///
/// ★ 이 수를 **내려서 통과시키지 마라.** 내리는 순간 이 가드는 "호출 지점을 못 찾았다" 와
/// "위반이 없다" 를 같은 초록으로 돌려주고, 그 둘은 전혀 다른 사실이다. 메뉴 항목이 실제로
/// 줄어 이 하한이 걸리면, 값을 고치기 전에 **줄어든 자리를 세서** 그 수가 맞는지부터
/// 확인해라(`rg 'setKeyEquivalent\(' src/`). 늘어나는 것은 이 하한이 안 본다 — 늘어난
/// 쪽은 아래 위반 판정이 본다.
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

/// 한 호출 지점의 판정.
#[derive(Debug, PartialEq, Eq)]
enum Source {
    /// 빈 문자열 리터럴.
    Empty,
    /// 같은 함수 안에서 [`FROM_SETTINGS`] 를 거쳐 묶인 이름.
    Settings,
    /// 그 밖 — 위반 후보.
    Unknown,
}

/// `setKeyEquivalent(` 인자를 괄호 한 겹까지 균형 맞춰 잘라 낸다.
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

/// 그 이름이 이 지점 **앞**에서 [`FROM_SETTINGS`] 를 거쳐 묶였는가.
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

/// 한 파일의 호출 지점들을 판정한다 — 파일 순회와 분리된 순수 함수.
fn sites_in(text: &str) -> Vec<(usize, Source)> {
    let needle = "setKeyEquivalent";
    let mut out = Vec::new();
    for (idx, _) in text.match_indices(needle) {
        let Some(open) = text[idx..].find('(').map(|o| idx + o) else {
            continue;
        };
        // `setKeyEquivalentModifierMask(` 는 다른 물음이다 — 이름이 이어지면 건너뛴다.
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
        "`setKeyEquivalent(` 호출 지점을 {seen} 곳만 찾았다(하한 {MIN_SITES}) — 스캔이 \
         죽었거나 그 API 를 부르는 방식이 바뀌었다. 그러면 아래 판정은 빈 집합을 훑고 \
         조용히 통과한다. ★ 수를 내려서 통과시키지 마라: 내리면 '못 찾았다' 와 '위반이 \
         없다' 가 같은 초록이 된다. 줄어든 자리를 먼저 세라."
    );

    assert!(
        violations.is_empty(),
        "NSMenu 항목의 단축키가 **설정에서도, 빈 값에서도** 오지 않는다:\n{}\n\n\
         정책은 둘만 허용한다 — `KeybindingSettings` 의 binding 에서 가져오거나(`{}` 를 \
         거쳐) 빈 문자열로 두거나. selector 가 OS 표준이라는 사실은 단축키 하드코딩의 \
         정당화가 아니다. 고쳐라: 대응 binding 필드를 읽어 변환하거나, binding 이 없으면 \
         key equivalent 를 비워 단축키 없는 항목으로 둬라.\n  \
         ★ 이 판정은 **한 홉만** 본다. 하드코딩한 키를 반환하는 헬퍼를 새로 만들어 그것을 \
         부르면 이 가드는 조용하다 — 그건 고친 것이 아니다.",
        violations.join("\n"),
        FROM_SETTINGS
    );
}

/// 판독기가 **양쪽 답을 다 낸다** — 한 방향만 재면 무정보다.
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

/// 이름이 이어지는 다른 API 를 자기 것으로 세지 않는다.
#[test]
fn the_modifier_mask_setter_is_a_different_question() {
    let text = "item.setKeyEquivalentModifierMask(NSEventModifierFlags::empty());\n";
    assert!(sites_in(text).is_empty());
}

/// 같은 이름이 **뒤에서** 묶여도 앞의 호출을 설정으로 세지 않는다.
#[test]
fn a_binding_after_the_call_does_not_count() {
    let text = "item.setKeyEquivalent(&key);\n\
                let (key, _) = binding_to_nsmenu_key(b);\n";
    assert_eq!(sites_in(text), vec![(1, Source::Unknown)]);
}

/// 이 축의 모수가 어느 플랫폼에 있는지 — 값으로 고정한다.
#[test]
fn the_registration_sites_live_in_one_place() {
    let root = repo_root();
    let mut files = Vec::new();
    rs_files(&root.join("src"), &mut files);
    let mut owners = BTreeSet::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        if !sites_in(&text).is_empty() {
            // 아래에서 `o.contains("macos")` 로 성분을 찾는다 — 구분자가 섞이면
            // 같은 트리가 플랫폼마다 다른 답을 낸다.
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
        "key equivalent 등록 자리가 macOS 한 파일 밖으로 퍼졌다: {owners:?}\n  \
         퍼진 것 자체는 결함이 아니다 — 다만 이 가드의 모수가 바뀐다. 새 자리에도 같은 \
         정책(설정에서 오거나 비어 있거나)이 적용되는지 확인하고 이 단정을 갱신해라."
    );
}
