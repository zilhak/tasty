//! **사용자에게 보이는 CLI 명령이 늘면 사용자 가이드가 그것을 알아야 한다** — 분류를 강제한다.
//!
//! `CLAUDE.md` 의 "문서 갱신 (필수)" 는 **사용자에게 보이는 동작**(메뉴·단축키·설정 키·
//! **CLI 명령**·설치 절차)이 바뀌면 공개 사이트의 사용자 가이드(`site/content/`)도 같은
//! 커밋에서 갱신하라고 요구한다. 그 요구에는 집행이 없었다 — 명령을 하나 더해도 아무것도
//! 빨개지지 않는다.
//!
//! # 이 가드가 요구하는 것은 "문서화" 가 아니라 **분류**다
//!
//! 모든 명령을 가이드에 넣으라고 하지 않는다. 그건 판단이고, 판단은 사람 몫이다. 대신
//! 새 명령이 들어올 때 **둘 중 하나를 고르게** 만든다:
//!
//! - 가이드(`site/content/`, 한국어 원본)에 `tasty <명령>` 으로 등장시키거나,
//! - 아래 [`NOT_IN_THE_GUIDE`] 에 **사유와 함께** 등록하거나.
//!
//! 등록 명부는 **지금 상태와 정확히 일치해야** 한다 — 늘어도(새 미기재) 줄어도(문서화했는데
//! 명부에 남음) 빨개진다. 수 하나로 들면 "수를 올린다" 가 가장 싼 수선이 되므로, 자리로 든다.
//!
//! # 모수 — 두 곳에서 온다
//!
//! 명령 목록은 한 곳이 아니다. 실측(2026-09-06)으로 `--help` 는 **42** 개를 냈는데 core
//! enum 은 **36** 개였다. 나머지 여섯은 **plugin 이 기여한 것**이다
//! (`tasty-plugin.toml` 의 `[[contributes.cli]]`). core 만 세면 그 여섯이 조용히 빠진다.
//!
//! - core: `crates/tasty-cli/src/lib.rs` 의 `pub enum Commands` 변이(kebab-case 로 변환)
//! - plugin: `crates/tasty-plugin-*/tasty-plugin.toml` 의 `[[contributes.cli]] name`
//!
//! # 이 가드가 단정하지 않는 것
//!
//! - **가이드가 그 명령을 제대로 설명하는지.** `tasty <명령>` 이 한 번 나오면 통과다.
//!   품질은 이 축이 답할 물음이 아니다.
//! - **영어 번역(`site/content/en/`).** 원본이 정본이고 번역은 별도 절차(`--stamp`)라
//!   여기서 안 본다.
//! - **하위 명령(`tasty list tree` 의 `tree`).** 최상위만 본다. 하위까지 넓히면 모수가
//!   수백이 되고, 그 수를 채우는 일은 이 가드가 강제할 성질이 아니다.
//!
//! # 채널
//!
//! `doc-guards.yml` — main push · PR 마다 경로 필터 없이 돈다. 이 축을 재는 채널은 그 하나다.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 가이드에 **일부러 없는** 명령과 그 사유. 자리로 적는다 — 부류로 적으면 도망길이 된다.
///
/// 사유가 `부채:` 로 시작하면 "없어도 되는 것" 이 아니라 **아직 안 쓴 것**이다. 그 줄을
/// 지우는 방법은 하나뿐이다 — 가이드에 쓰는 것. 지금 이 명부에 `부채:` 는 없다: 두 줄 다
/// **가이드에 싣지 않는 것이 옳다** 는 판정이고, 판정 근거가 명령마다 다르다.
const NOT_IN_THE_GUIDE: &[(&str, &str)] = &[
    (
        "debug",
        "debug 빌드에만 있는 명령이다. 설치해서 쓰는 사람에게는 존재하지 않는다",
    ),
    (
        "completion-strategy",
        "`list` 전용이다 — host/plugin 이 등록한 내부 완료-판정 전략 레지스트리를 읽어 덤프한다. \
         사용자가 수행하는 조작이 없고, 그 전략이 만드는 사용자 결과(완료 알림)는 가이드의 \
         에이전트 장이 이미 다룬다. 기제를 개발·디버깅할 때 들여다보는 introspection 이라 \
         `docs/` 소관이다",
    ),
];

/// 훑어야 할 최소 명령 수 — **모수가 살아 있다는 증거**.
///
/// 실측 42(core 36 + plugin 6, 2026-09-06). 여유를 두고 35 로 둔다 — 래칫이 아니라
/// **생존 바닥**이다.
///
/// ★ 이 수를 **내려서 통과시키지 마라.** 내리면 "파서가 죽었다" 와 "명령이 없다" 가 같은
/// 초록이 된다. 명령이 실제로 줄어 이 하한이 걸리면 `tasty --help` 로 먼저 세라.
const MIN_COMMANDS: usize = 35;

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

/// CamelCase 변이 이름을 clap 이 쓰는 kebab-case 로.
fn kebab(name: &str) -> String {
    let mut out = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            out.push('-');
        }
        out.extend(ch.to_lowercase());
    }
    out
}

/// core 명령 — `pub enum Commands` 의 최상위 변이. debug 게이트된 것은 따로 표시한다.
fn core_commands(src: &str) -> Vec<(String, bool)> {
    let Some(start) = src.find("pub enum Commands {") else {
        return Vec::new();
    };
    let body = &src[start..];
    let end = body.find("\n}\n").unwrap_or(body.len());
    let body = &body[..end];
    let mut out = Vec::new();
    let lines: Vec<&str> = body.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        // 최상위 변이는 정확히 4 칸 들여쓰기 + 대문자로 시작한다.
        if !line.starts_with("    ") || line.starts_with("     ") {
            continue;
        }
        let t = line.trim();
        let Some(first) = t.chars().next() else {
            continue;
        };
        if !first.is_ascii_uppercase() {
            continue;
        }
        let name: String = t
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if name.is_empty() {
            continue;
        }
        // 변이 이름 뒤에는 `{`, `(`, `,` 중 하나가 온다 — 타입 이름 등을 배제한다.
        let rest = t[name.len()..].trim_start();
        if !(rest.starts_with('{') || rest.starts_with('(') || rest.starts_with(',')) {
            continue;
        }
        let debug_only =
            i > 0 && lines[i - 1].contains("cfg(") && lines[i - 1].contains("debug_assertions");
        out.push((kebab(&name), debug_only));
    }
    out
}

/// plugin 이 기여한 명령 — 매니페스트의 `[[contributes.cli]] name`.
fn plugin_commands(root: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root.join("crates")) else {
        return out;
    };
    let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    dirs.sort();
    for dir in dirs {
        let manifest = dir.join("tasty-plugin.toml");
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        let owner = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut lines = text.lines();
        while let Some(line) = lines.next() {
            if line.trim() != "[[contributes.cli]]" {
                continue;
            }
            for next in lines.by_ref() {
                let t = next.trim();
                if t.is_empty() || t.starts_with('#') {
                    continue;
                }
                if let Some(rest) = t.strip_prefix("name") {
                    if let Some(v) = rest.split('"').nth(1) {
                        out.push((v.to_string(), owner.clone()));
                    }
                }
                break;
            }
        }
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
fn every_cli_command_is_either_in_the_guide_or_registered_with_a_reason() {
    let root = repo_root();
    let src = std::fs::read_to_string(root.join("crates/tasty-cli/src/lib.rs"))
        .expect("tasty-cli/src/lib.rs 를 읽지 못했다");
    let core = core_commands(&src);
    let plugins = plugin_commands(&root);
    let guide = guide_text(&root);

    let total = core.len() + plugins.len();
    assert!(
        total >= MIN_COMMANDS,
        "CLI 명령을 {total} 개만 찾았다(하한 {MIN_COMMANDS}) — 파서가 죽었거나 선언 형태가 \
         바뀌었다. 그러면 아래 판정은 빈 집합을 훑고 조용히 통과한다. ★ 수를 내려서 \
         통과시키지 마라: `tasty --help` 로 먼저 세라."
    );

    let registered: BTreeSet<&str> = NOT_IN_THE_GUIDE.iter().map(|(c, _)| *c).collect();
    let mut undocumented = Vec::new();
    let mut all: Vec<String> = core.iter().map(|(c, _)| c.clone()).collect();
    all.extend(plugins.iter().map(|(c, _)| c.clone()));
    for (cmd, debug_only) in &core {
        if *debug_only && registered.contains(cmd.as_str()) {
            continue; // debug 전용은 등록돼 있으면 그것으로 끝난다.
        }
        if !guide.contains(&format!("tasty {cmd}")) && !registered.contains(cmd.as_str()) {
            undocumented.push(format!("  {cmd}  (core)"));
        }
    }
    for (cmd, owner) in &plugins {
        if !guide.contains(&format!("tasty {cmd}")) && !registered.contains(cmd.as_str()) {
            undocumented.push(format!("  {cmd}  ({owner} 가 기여)"));
        }
    }

    assert!(
        undocumented.is_empty(),
        "사용자에게 보이는 CLI 명령인데 가이드(`site/content/`)에 `tasty <명령>` 으로 한 번도 \
         안 나오고, 예외 명부에도 없다:\n{}\n\n\
         `CLAUDE.md` 의 \"문서 갱신 (필수)\" 는 CLI 명령이 바뀌면 사용자 가이드도 **같은 \
         커밋에서** 갱신하라고 요구한다. 고치는 길 둘:\n  \
         (가) 가이드에 그 명령을 쓴다 — 독자가 다르다(설치해서 쓰는 사람). 소스 경로·ADR·IPC \
         메서드명을 넣지 마라.\n  \
         (나) 사용자에게 보이는 명령이 **아니면** 이 파일의 `NOT_IN_THE_GUIDE` 에 **사유와 \
         함께** 등록해라. ★ 사유가 '아직 안 썼다' 면 그것은 예외가 아니라 부채다 — 그렇게 \
         적어라. 부류로 넓히지 마라(예외가 부류가 되면 다시 도망길이다).",
        undocumented.join("\n")
    );
}

/// 등록 명부가 **살아 있는가** — 문서화했는데 명부에 남은 줄을 잡는다.
///
/// 죽은 등록은 다음 사람에게 "이 명령은 안 써도 된다" 로 읽힌다. 부채를 갚았으면 그 줄도
/// 함께 지워야 갚은 것이 보인다.
#[test]
fn no_registered_command_is_already_in_the_guide() {
    let root = repo_root();
    let guide = guide_text(&root);
    let stale: Vec<&str> = NOT_IN_THE_GUIDE
        .iter()
        .filter(|(c, _)| *c != "debug") // debug 는 빌드 축이라 가이드 등장과 무관하다.
        .filter(|(c, _)| guide.contains(&format!("tasty {c}")))
        .map(|(c, _)| *c)
        .collect();
    assert!(
        stale.is_empty(),
        "가이드에 이미 있는데 예외 명부에 남아 있다: {stale:?}\n  \
         부채를 갚았으면 그 줄을 지워라 — 남겨 두면 다음 사람이 '이 명령은 안 써도 된다' 로 \
         읽는다."
    );
}

/// 등록된 이름이 **실재하는 명령인가** — 사라진 명령의 등록이 남는 것을 잡는다.
#[test]
fn every_registered_command_still_exists() {
    let root = repo_root();
    let src = std::fs::read_to_string(root.join("crates/tasty-cli/src/lib.rs"))
        .expect("tasty-cli/src/lib.rs 를 읽지 못했다");
    let mut known: BTreeSet<String> = core_commands(&src).into_iter().map(|(c, _)| c).collect();
    known.extend(plugin_commands(&root).into_iter().map(|(c, _)| c));
    let gone: Vec<&str> = NOT_IN_THE_GUIDE
        .iter()
        .map(|(c, _)| *c)
        .filter(|c| !known.contains(*c))
        .collect();
    assert!(
        gone.is_empty(),
        "예외 명부에 있는데 그런 명령이 없다: {gone:?}\n  \
         명령이 사라졌으면 등록도 지워라."
    );
}

/// 각 등록에 **사유가 붙어 있는가** — 빈 사유는 등록이 아니다.
#[test]
fn every_registration_carries_a_reason() {
    let empty: Vec<&str> = NOT_IN_THE_GUIDE
        .iter()
        .filter(|(_, why)| why.trim().len() < 10)
        .map(|(c, _)| *c)
        .collect();
    assert!(
        empty.is_empty(),
        "사유 없이 등록된 명령: {empty:?}\n  사유가 없으면 다음 사람이 그 줄을 지울지 \
         남길지 판단할 수 없다."
    );
}

/// 판독기가 **양쪽 답을 다 낸다**.
#[test]
fn the_reader_answers_both_yes_and_no() {
    let src = "pub enum Commands {\n    /// doc\n    New {\n        x: u8,\n    },\n    \
               #[cfg(debug_assertions)]\n    Debug {\n        y: u8,\n    },\n    \
               IsTyping {\n        z: u8,\n    },\n    Port,\n}\n";
    let got = core_commands(src);
    assert_eq!(
        got,
        vec![
            ("new".to_string(), false),
            ("debug".to_string(), true),
            ("is-typing".to_string(), false),
            ("port".to_string(), false),
        ]
    );

    // enum 이 없으면 빈 목록 — 하한이 그것을 잡는다.
    assert!(core_commands("fn main() {}").is_empty());
}

/// kebab 변환은 **연속 대문자가 아니라 낱말 경계**를 본다.
#[test]
fn the_kebab_conversion_matches_clap() {
    assert_eq!(kebab("New"), "new");
    assert_eq!(kebab("SurfaceMeta"), "surface-meta");
    assert_eq!(kebab("CompletionStrategy"), "completion-strategy");
}
