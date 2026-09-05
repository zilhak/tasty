//! **설정 파일의 절 이름과 컨텍스트 메뉴 항목 이름이 사용자 가이드에 닿는가.**
//!
//! `CLAUDE.md` 의 "문서 갱신 (필수)" 가 묶은 다섯 축 중 **메뉴**와 **설정 키** 쪽이다.
//! CLI 명령과 설치 산출물 파일명은 각각 다른 파일이 본다.
//!
//! # 한 축이 아니라 넷이었다 — 그중 둘만 여기 있다
//!
//! "메뉴 · 설정 키" 를 한 물음으로 놓으면 답이 안 나온다. 갈라서 각각에 "두 쪽이 같은
//! 어휘를 쓰는가" 를 물으면 갈린다(실측 2026-09-06).
//!
//! | 축 | 두 쪽의 어휘 | 판정 |
//! |---|---|---|
//! | 설정 파일 **절** 이름 | 양쪽 다 `[general]` 같은 식별자 | **여기서 본다** |
//! | 컨텍스트 메뉴 **항목** | 가이드가 `lang/ko.toml` 값을 글자 그대로 인용한다 | **여기서 본다** |
//! | 설정 **필드** 이름 | 갈린다 — 아래 | 채널 **없다** |
//! | macOS **애플리케이션 메뉴** | 갈린다 — 아래 | 채널 **없다** |
//!
//! **설정 필드 축이 안 되는 이유.** 가이드는 설정을 두 어휘로 적는다 — 탭 표에서는 화면
//! 라벨로, 설정 파일 절에서는 toml 키로. 그런데 코드에는 **사용자에게 보이는 설정과 내부
//! 영속 슬롯을 가르는 표시가 없다.** `theme_base`(테마 색 전체 덤프) · `sidebar_width`
//! (드래그 결과) · `macos_fda_notice_shown`(다시 보지 않기 플래그)이 사용자가 고르는
//! 항목과 같은 구조체에 나란히 있다. 이름으로 가르려고 `lang/ko.toml` 의 `_label` 접미사를
//! 써 봤다 — 실측 119 개 중 **91** 이 가이드에 있고 나머지 **23** 이 남는데, 그 23 은
//! 누락이 아니라 대부분 어휘 차이다(가이드는 "패인 분할 (좌우 / 상하)" 로 묶어 쓰고
//! `lang` 은 "페인 수직 분할" · "페인 수평 분할" 로 나눠 쓴다). 문자열 대조를 놓으면 그
//! 23 을 고발하고, 가장 싼 초록화는 **가이드를 코드 어휘로 고쳐 쓰는 것**이 된다.
//!
//! **macOS 애플리케이션 메뉴 축이 안 되는 이유.** 실측 11 항목 중 가이드에 있는 것이 4 다.
//! 없는 7 중 셋은 제목이 **형식 문자열**(`{} 정보` 처럼 앱 이름이 들어간다)이라 문자열
//! 대조가 원리적으로 못 맞힌다. 나머지는 `make_std_item` 으로 만드는 **OS 표준 항목**
//! (가리기 · 모두 보기 등)이라 어느 Mac 앱에나 있고, 가이드가 안 적는 것이 옳다. 남는
//! 모수는 한 자리이고 그것도 **macOS 에만 있다**(Windows·Linux 는 등록 자리가 0 이라
//! 거기 초록은 "잴 것이 없다" 다).
//!
//! # 이 가드가 단정하지 않는 것
//!
//! - **가이드가 그것을 제대로 설명하는지.** 이름이 한 번 나오면 통과다.
//! - **영어 번역(`site/content/en/`).** 원본이 정본이라 여기서 안 본다.
//! - **설정 값의 의미·기본값.** 이름만 본다.
//!
//! # 채널
//!
//! `doc-guards.yml` — main push · PR 마다 경로 필터 없이 돈다. 이 축을 재는 채널은 그 하나다.

use std::path::{Path, PathBuf};

/// 가이드의 설정 파일 절 목록에 **일부러 없는** 절과 그 사유. 자리로 적는다.
///
/// `부채:` 로 시작하는 것은 "없어도 되는 것" 이 아니라 **아직 안 쓴 것**이다.
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

/// 항목 이름을 가이드와 대조할 `lang/ko.toml` 네임스페이스.
///
/// **macOS 애플리케이션 메뉴(`menu.macos`)는 여기 없다** — 모듈 doc 의 표에 적은 대로 그
/// 축은 어휘가 갈린다. 빼는 것을 조용히 하지 않으려고 여기 적는다.
const MENU_NAMESPACES: &[&str] = &[
    "context_menu",
    "tab_context_menu",
    "pane_context_menu",
    "terminal_context_menu",
    "surface_context_menu",
    "tools_menu",
];

/// 훑어야 할 최소 절 수 — **모수가 살아 있다는 증거**. 실측 12(2026-09-06).
///
/// ★ 이 수를 내려서 통과시키지 마라. 먼저 가른다 — `Settings` 의 필드가 정말 줄었나,
/// 아니면 구조체의 모양이 바뀌어 파서가 못 읽나. 뒤쪽이면 하한을 내리는 것은 고장을
/// 초록으로 만드는 것이다.
const MIN_SECTIONS: usize = 8;

/// 훑어야 할 최소 메뉴 항목 수 — 같은 이유. 실측 23(2026-09-06).
///
/// ★ 같은 금지가 여기에도 걸린다. 네임스페이스 하나가 통째로 사라지면 이 수가 먼저
/// 떨어진다 — 그때 하한이 아니라 [`MENU_NAMESPACES`] 를 봐라.
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
    // 선언 줄 자신을 모수에 넣지 않으려고 `{` 뒤부터 읽는다 — `pub struct Settings {` 도
    // `pub ` 로 시작해서, 그냥 읽으면 절 이름이 하나 늘어난다(첫 실행에서 실제로 났다).
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

/// `lang/ko.toml` 의 지정 네임스페이스에 있는 항목 이름.
///
/// 값에 `{}` 가 있으면 뺀다 — 실행할 때 채워지는 자리라 **문자열 대조가 원리적으로 못
/// 맞힌다.** 이건 면제가 아니라 모수의 성질이고, 그래서 이름이 아니라 성질로 가른다.
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

/// 한국어 가이드 원본 전체를 한 덩어리로.
fn guide_text(root: &Path) -> String {
    fn walk(dir: &Path, out: &mut String) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().map(|n| n == "en").unwrap_or(false) {
                    continue; // 번역은 별도 절차다.
                }
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                out.push_str(&text);
                out.push('\n');
            }
        }
    }
    let mut out = String::new();
    walk(&root.join("site/content"), &mut out);
    out
}

#[test]
fn every_config_section_is_named_in_the_guide_or_registered_with_a_reason() {
    let root = repo_root();
    let sections = config_sections(&root);
    assert!(
        sections.len() >= MIN_SECTIONS,
        "설정 절을 {}개밖에 못 찾았다(하한 {MIN_SECTIONS}) — 추출이 깨졌다.\n\
         ★ 이 수를 내려서 통과시키지 마라. `pub struct Settings` 의 필드가 정말 줄었는지 \
         먼저 세라.",
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
        "`config.toml` 에 저장되는 절인데 사용자 가이드가 그 이름을 한 번도 안 적는다:\n  \
         {}\n\n고치는 길 둘:\n\
           (가) 설정 가이드의 설정 파일 절 목록에 그 절을 적는다 — 손으로 파일을 고치는 \
         독자가 그 목록을 스키마로 읽는다.\n\
           (나) 사용자가 손댈 값이 **아니면** 이 파일의 `SECTION_NOT_IN_THE_GUIDE` 에 \
         사유와 함께 등록해라. ★ 사유가 '아직 안 썼다' 면 그것은 예외가 아니라 부채다.",
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
        "가이드가 이 절을 이미 적는데 명부에 남아 있다: {stale:?} — 부채를 갚았으면 그 줄을 \
         지워라. 안 지우면 다음 사람이 아직 빚이 있다고 읽는다."
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
        "명부가 이제 없는 절을 붙들고 있다: {dead:?} — 사라진 절의 부채는 부채가 아니다"
    );
}

#[test]
fn every_registration_carries_its_own_reason() {
    let mut seen: Vec<&str> = Vec::new();
    for (name, reason) in SECTION_NOT_IN_THE_GUIDE {
        assert!(
            reason.len() > 20,
            "{name} 의 사유가 너무 짧다 — 사유는 다음 사람이 판단을 다시 할 수 있을 만큼 적는다"
        );
        assert!(
            !seen.contains(reason),
            "{name} 의 사유가 다른 줄과 글자 그대로 같다. 사유가 같으면 그건 자리가 아니라 \
             **부류**이고, 부류 예외는 도망길이 된다 — 이 절만의 근거를 적어라."
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
        "컨텍스트 메뉴 항목을 {}개밖에 못 찾았다(하한 {MIN_MENU_ITEMS}) — 추출이 깨졌거나 \
         네임스페이스가 사라졌다.\n\
         ★ 이 수를 내려서 통과시키지 마라. `MENU_NAMESPACES` 의 절이 `lang/ko.toml` 에 \
         아직 있는지 먼저 봐라.",
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
        "사용자가 오른쪽 클릭으로 보는 항목인데 가이드가 그 이름을 한 번도 안 적는다:\n  \
         {}\n\n가이드는 이 이름들을 `lang/ko.toml` 의 값 **그대로** 인용한다 — 그래서 \
         이 대조가 성립한다. 항목을 더했으면 그 항목을 다루는 장에 이름을 적어라.\n\
         ★ 표기를 바꿔서 맞추지 마라. 가이드가 화면과 다른 낱말을 쓰면 독자가 화면에서 \
         그것을 못 찾는다.",
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
        "macOS 애플리케이션 메뉴가 모수에 새어 들어왔다 — 그 축은 어휘가 갈려 여기서 안 본다"
    );
    assert!(
        labels.iter().all(|(_, v)| !v.contains("{}")),
        "형식 문자열이 모수에 남았다 — 문자열 대조가 원리적으로 못 맞히는 값이다"
    );

    let guide = guide_text(&root);
    assert!(guide.contains("[general]"), "예: 있음");
    assert!(
        !guide.contains("[nonexistent_section]"),
        "예: 없음 — 없는 것을 있다고 읽으면 이 가드는 아무것도 안 본다"
    );
}
