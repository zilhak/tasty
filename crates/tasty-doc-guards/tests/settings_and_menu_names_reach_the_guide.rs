//! 설정 파일의 절 이름과 컨텍스트 메뉴 라벨이 한국어 사용자 가이드에 나오는지 확인한다.
//! 이름이 한 번 나오면 통과하며 설명의 정확성·설정값의 의미·기본값은 검사하지 않는다.
//! 영어 번역과 macOS 애플리케이션 메뉴는 제외한다. macOS 메뉴에는 OS 표준 항목과
//! 앱 이름을 넣는 형식 문자열이 있어 가이드의 고정 라벨과 직접 비교하기 어렵다.
//!
//! 개별 설정 필드도 검사하지 않는다. 같은 구조체에 사용자 설정과 내부 저장값이 섞여 있고,
//! 가이드가 화면 라벨과 TOML 키를 구별해 설명하므로 단순 이름 대조로 누락을 판단할 수 없다.
//! 자동 실행 경로는 docs/dev-guide/ci-gates.md에 있다.

use std::path::{Path, PathBuf};
use tasty_doc_guards::temp_scratch::Scratch;

/// 가이드에서 빠진 설정 절과 사유. 부채:로 시작하면 정당한 제외가 아니라 아직 쓰지 않은 문서다.
const SECTION_NOT_IN_THE_GUIDE: &[(&str, &str)] = &[
    (
        "memory",
        "부채: plugin 메모리 스토어의 용량 상한 셋(entry 하나 · plugin 별 secret · regular \
         합계). 사용자가 올릴 수 있는 값인데 설정 파일 절 목록에 이 절이 없다",
    ),
    (
        "scripts",
        "부채: Lua 스크립트 등록 목록. 기능 자체는 가이드가 다루는데(스크립트 장) 그 값이 \
         `config.toml` 의 어느 절에 저장되는지는 안 적는다",
    ),
];

/// 가이드와 대조할 컨텍스트 메뉴 네임스페이스. menu.macos는 제외한다.
const MENU_NAMESPACES: &[&str] = &[
    "context_menu",
    "tab_context_menu",
    "pane_context_menu",
    "terminal_context_menu",
    "surface_context_menu",
    "tools_menu",
];

/// 설정 절 수집 하한. 2026-09-06 측정12개. 줄면 실제 필드와 파서를 먼저 대조한다.
const MIN_SECTIONS: usize = 8;

/// 메뉴 항목 수집 하한. 2026-09-06 측정23개. 줄면 네임스페이스와 추출 범위를 확인한다.
const MIN_MENU_ITEMS: usize = 15;

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

/// `pub struct Settings` 의 절 이름. `#[serde(skip)]` 필드는 디스크에 안 나가므로 뺀다.
fn config_sections(root: &Path) -> Vec<String> {
    let src = std::fs::read_to_string(root.join("crates/tasty-settings/src/lib.rs"))
        .expect("tasty-settings/src/lib.rs 를 읽지 못했다");
    let start = src
        .find("pub struct Settings {")
        .expect("`pub struct Settings` 를 못 찾았다 — 구조체 이름이 바뀌었나");
    // 구조체 선언도 pub으로 시작하므로 본문만 읽는다.
    let body = &src[start + "pub struct Settings {".len()..];
    let end = body.find("\n}").expect("구조체 끝을 못 찾았다");
    let mut out = Vec::new();
    let mut skip_next = false;
    for line in body[..end].lines() {
        let line = line.trim();
        if line.starts_with("#[serde(skip)]") {
            skip_next = true;
            continue;
        }
        let Some(rest) = line.strip_prefix("pub ") else {
            continue;
        };
        let Some(name) = rest.split(':').next() else {
            continue;
        };
        if skip_next {
            skip_next = false;
            continue;
        }
        out.push(name.trim().to_string());
    }
    out
}

/// 지정한 번역 절에서 고정 메뉴 라벨을 읽는다. 실행 시 값을 채울 {} 형식은 제외한다.
fn menu_labels(root: &Path) -> Vec<(String, String)> {
    let text =
        std::fs::read_to_string(root.join("lang/ko.toml")).expect("lang/ko.toml 을 읽지 못했다");
    let mut section = String::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(inner) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = inner.to_string();
            continue;
        }
        if !MENU_NAMESPACES.contains(&section.as_str()) {
            continue;
        }
        let Some((key, value)) = t.split_once('=') else {
            continue;
        };
        let value = value.trim();
        let Some(value) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
            continue;
        };
        if value.contains("{}") || value.is_empty() {
            continue;
        }
        out.push((format!("{section}.{}", key.trim()), value.to_string()));
    }
    out
}

/// 수집 누락을 찾도록 순회 결과와 독립된 최상위 가이드 디렉터리 목록을 둔다. 영어 번역은 제외한다.
const GUIDE_BRANCHES: &[&str] = &[
    "agents",
    "customize",
    "getting-started",
    "help",
    "plugins",
    "remote",
    "using",
];

/// 한국어 가이드 본문을 모은다. 읽기 실패와 최상위 디렉터리 누락은 별도로 검사한다.
fn guide_text(root: &Path) -> String {
    guide_scan(root).0
}

/// 합성 트리에서도 쓸 수 있도록 순회와 실제 가이드 디렉터리 목록 대조를 분리한다.
fn guide_scan(
    root: &Path,
) -> (
    String,
    std::collections::BTreeSet<String>,
    std::collections::BTreeSet<String>,
) {
    fn walk(
        dir: &Path,
        top: &Path,
        out: &mut String,
        branches: &mut std::collections::BTreeSet<String>,
        touched: &mut std::collections::BTreeSet<String>,
        current: Option<&str>,
    ) {
        let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
            panic!(
                "가이드 디렉터리를 읽지 못했다: {} — {e}. 일부 문서를 빠뜨리지 않도록 오류를 해결한다.",
                dir.display()
            )
        });
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().map(|n| n == "en").unwrap_or(false) {
                    continue; // 번역은 별도 절차다.
                }
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let next = if dir == top {
                    branches.insert(name.clone());
                    Some(name)
                } else {
                    current.map(str::to_owned)
                };
                walk(&path, top, out, branches, touched, next.as_deref());
            } else if path.extension().and_then(|e| e.to_str()) == Some("md")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                if let Some(b) = current {
                    touched.insert(b.to_owned());
                }
                out.push_str(&text);
                out.push('\n');
            }
        }
    }
    let top = root.join("site/content");
    let mut out = String::new();
    let (mut branches, mut touched) = (
        std::collections::BTreeSet::new(),
        std::collections::BTreeSet::new(),
    );
    walk(&top, &top, &mut out, &mut branches, &mut touched, None);
    (out, branches, touched)
}

#[test]
fn the_guide_walk_reaches_every_branch() {
    let (_, branches, touched) = guide_scan(&repo_root());
    let missing: Vec<&&str> = GUIDE_BRANCHES
        .iter()
        .filter(|b| !touched.contains(**b))
        .collect();
    assert!(
        missing.is_empty(),
        "다음 가이드 디렉터리에서 Markdown을 수집하지 못했다: {missing:?}. 등록{}개 중 {}개를 읽었다. 경로와 제외 범위를 확인하고, 실제 범위 변경 근거 없이 목록에서 제거하지 않는다.",
        GUIDE_BRANCHES.len(),
        touched.len()
    );
    let extra: Vec<&String> = branches
        .iter()
        .filter(|b| !GUIDE_BRANCHES.contains(&b.as_str()))
        .collect();
    assert!(
        extra.is_empty(),
        "site/content에 등록되지 않은 디렉터리가 있다: {extra:?}. GUIDE_BRANCHES에 추가해 수집 누락을 확인할 수 있게 한다."
    );
}

#[test]
fn every_config_section_is_named_in_the_guide_or_registered_with_a_reason() {
    let root = repo_root();
    let sections = config_sections(&root);
    assert!(
        sections.len() >= MIN_SECTIONS,
        "설정 절을 {}개만 찾았다(하한 {MIN_SECTIONS}). Settings 필드의 실제 감소와 파싱 실패를 구별한다.",
        sections.len()
    );

    let guide = guide_text(&root);
    let missing: Vec<&String> = sections
        .iter()
        .filter(|s| !guide.contains(&format!("[{s}]")) && !guide.contains(&format!("[{s}.")))
        .filter(|s| {
            !SECTION_NOT_IN_THE_GUIDE
                .iter()
                .any(|(n, _)| *n == s.as_str())
        })
        .collect();

    assert!(
        missing.is_empty(),
        "config.toml에 저장되지만 가이드에서 찾지 못한 절이다:\n  {}\n설정 가이드에 절 이름을 추가한다. 사용자 설정이 아니라면 SECTION_NOT_IN_THE_GUIDE에 이유를 기록한다. 아직 문서를 쓰지 않은 경우는 부채로 구분한다.",
        missing
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn no_registered_section_is_already_in_the_guide() {
    let root = repo_root();
    let guide = guide_text(&root);
    let stale: Vec<&str> = SECTION_NOT_IN_THE_GUIDE
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| guide.contains(&format!("[{n}]")) || guide.contains(&format!("[{n}.")))
        .collect();
    assert!(
        stale.is_empty(),
        "가이드에 이미 있는 절이 누락 목록에 남았다. 항목을 제거한다: {stale:?}"
    );
}

#[test]
fn every_registered_section_still_exists() {
    let root = repo_root();
    let sections = config_sections(&root);
    let dead: Vec<&str> = SECTION_NOT_IN_THE_GUIDE
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| !sections.iter().any(|s| s == n))
        .collect();
    assert!(
        dead.is_empty(),
        "누락 목록에 실제 설정에 없는 절이 남았다: {dead:?}"
    );
}

#[test]
fn every_registration_carries_its_own_reason() {
    let mut seen: Vec<&str> = Vec::new();
    for (name, reason) in SECTION_NOT_IN_THE_GUIDE {
        assert!(
            reason.len() > 20,
            "{name}: 문서에서 제외한 근거를 판단할 수 있도록 사유를 적는다."
        );
        assert!(
            !seen.contains(reason),
            "{name}: 다른 항목과 같은 사유다. 이 절을 제외한 구체적인 근거를 적는다."
        );
        seen.push(reason);
    }
}

#[test]
fn every_context_menu_item_is_named_in_the_guide() {
    let root = repo_root();
    let labels = menu_labels(&root);
    assert!(
        labels.len() >= MIN_MENU_ITEMS,
        "컨텍스트 메뉴 항목을 {}개만 찾았다(하한 {MIN_MENU_ITEMS}). MENU_NAMESPACES의 절과 lang/ko.toml의 실제 항목을 확인한다.",
        labels.len()
    );

    let guide = guide_text(&root);
    let missing: Vec<String> = labels
        .iter()
        .filter(|(_, v)| !guide.contains(v.trim_end_matches(':')))
        .map(|(k, v)| format!("{k} = {v}"))
        .collect();

    assert!(
        missing.is_empty(),
        "가이드에서 찾지 못한 컨텍스트 메뉴 항목이다:\n  {}\n해당 기능을 설명하는 절에 lang/ko.toml의 화면 라벨 그대로 적는다. 화면과 다른 이름으로 바꾸지 않는다.",
        missing.join("\n  ")
    );
}

#[test]
fn the_reader_answers_both_yes_and_no() {
    let root = repo_root();
    let sections = config_sections(&root);
    assert!(sections.iter().any(|s| s == "general"), "예: 있음");
    assert!(sections.iter().any(|s| s == "appearance"), "예: 있음");
    assert!(
        !sections.iter().any(|s| s == "origin"),
        "`#[serde(skip)]` 필드가 절로 새어 들어왔다 — 디스크에 안 나가는 값을 가이드에 \
         적으라고 요구하게 된다"
    );

    let labels = menu_labels(&root);
    assert!(
        labels.iter().all(|(k, _)| !k.starts_with("menu.macos")),
        "제외 대상인 macOS 애플리케이션 메뉴가 수집됐다"
    );
    assert!(
        labels.iter().all(|(_, v)| !v.contains("{}")),
        "실행 시 채우는 형식 문자열이 고정 라벨 목록에 포함됐다"
    );

    let guide = guide_text(&root);
    assert!(guide.contains("[general]"), "예: 있음");
    assert!(
        !guide.contains("[nonexistent_section]"),
        "없는 설정 절을 가이드에서 찾았다고 판단했다"
    );
}

/// 수집이 과도하게 넓어지는 오류는 하한으로 찾을 수 없어 합성 입력의 정확한 결과도 비교한다.
/// 메뉴 네임스페이스의 값은 목록에서 가져와 내용 변경과 무관하게 필터 동작을 확인한다.
#[test]
fn the_three_readers_answer_on_a_substituted_tree() {
    let probe = Scratch::new("settings-guide-reader");
    let dir = probe.path();

    let settings_src = dir.join("crates/tasty-settings/src");
    std::fs::create_dir_all(&settings_src).expect("합성 설정 트리를 만들지 못했다");
    std::fs::write(
        settings_src.join("lib.rs"),
        "pub struct Settings {\n    \
             pub alpha_zone: AlphaCfg,\n    \
             #[serde(skip)]\n    \
             pub hidden_zone: HiddenCfg,\n    \
             pub omega_zone: OmegaCfg,\n    \
             not_pub_at_all: u8,\n\
         }\n",
    )
    .expect("합성 lib.rs 를 쓰지 못했다");

    let sections = config_sections(dir);
    assert_eq!(
        sections,
        vec!["alpha_zone".to_string(), "omega_zone".to_string()],
        "설정 절 판독이 합성 트리에서 다른 답을 냈다"
    );
    assert!(
        !sections.iter().any(|s| s.contains("struct")),
        "선언 줄 자신을 절로 셌다 — `{{` 뒤부터 읽는 오프셋이 사라졌다"
    );
    assert!(
        !sections.iter().any(|s| s == "hidden_zone"),
        "`#[serde(skip)]` 필드를 셌다 — 디스크에 안 나가는 것을 가이드에 요구하게 된다"
    );
    assert!(
        !sections.iter().any(|s| s == "not_pub_at_all"),
        "`pub` 이 아닌 필드를 셌다"
    );

    let inside = MENU_NAMESPACES
        .first()
        .expect("메뉴 네임스페이스 목록이 비어 합성 입력을 만들 수 없다");
    std::fs::create_dir_all(dir.join("lang")).expect("합성 lang 트리를 만들지 못했다");
    std::fs::write(
        dir.join("lang/ko.toml"),
        format!(
            "[{inside}]\n\
             alpha = \"제타 라벨\"\n\
             with_slot = \"값 {{}} 이 들어간다\"\n\
             empty_one = \"\"\n\
             not_quoted = bare\n\
             \n\
             [zeta_not_a_namespace]\n\
             beta = \"명부 밖 라벨\"\n"
        ),
    )
    .expect("합성 ko.toml 을 쓰지 못했다");

    let labels = menu_labels(dir);
    assert_eq!(
        labels,
        vec![(format!("{inside}.alpha"), "제타 라벨".to_string())],
        "메뉴 라벨 판독이 합성 트리에서 다른 답을 냈다"
    );
    assert!(
        !labels
            .iter()
            .any(|(k, _)| k.starts_with("zeta_not_a_namespace")),
        "명부 밖 네임스페이스를 셌다 — 명부로 거르는 일이 사라졌다"
    );
    assert!(
        !labels.iter().any(|(_, v)| v.contains("{}")),
        "실행할 때 채워지는 자리를 낀 라벨을 셌다 — 문자열 대조가 원리적으로 못 맞힌다"
    );
    assert!(!labels.iter().any(|(_, v)| v.is_empty()), "빈 라벨을 셌다");
    assert!(
        !labels.iter().any(|(k, _)| k.ends_with("not_quoted")),
        "따옴표가 없는 값을 라벨로 셌다"
    );

    let content = dir.join("site/content");
    std::fs::create_dir_all(content.join("en")).expect("합성 가이드 트리를 만들지 못했다");
    std::fs::create_dir_all(content.join("sub")).expect("합성 하위 장을 만들지 못했다");
    std::fs::write(content.join("settings.md"), "alpha_zone 을 설명한다\n")
        .expect("합성 원본 실패");
    std::fs::write(
        content.join("sub").join("deep.md"),
        "deep-marker 가 여기 있다\n",
    )
    .expect("합성 하위 원본 실패");
    std::fs::write(content.join("en").join("settings.md"), "translated-omega\n")
        .expect("합성 번역 실패");
    std::fs::write(content.join("notes.txt"), "md 가 아니다 sigma-decoy\n").expect("잡파일 실패");

    let guide = guide_text(dir);
    assert!(guide.contains("alpha_zone"), "원본 `.md` 를 안 읽었다");
    assert!(guide.contains("deep-marker"), "하위 디렉토리로 안 내려갔다");
    assert!(
        !guide.contains("translated-omega"),
        "영어 번역이 한국어 가이드 본문에 섞였다"
    );
    assert!(
        !guide.contains("sigma-decoy"),
        "Markdown이 아닌 파일을 읽었다"
    );
}
