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
use tasty_doc_guards::temp_scratch::Scratch;

/// 가이드에 **일부러 없는** 명령과 그 사유. 자리로 적는다 — 부류로 적으면 도망길이 된다.
///
/// 사유가 `부채:` 로 시작하면 "없어도 되는 것" 이 아니라 **아직 안 쓴 것**이다. 그 줄을
/// 지우는 방법은 하나뿐이다 — 가이드에 쓰는 것. 지금 이 명부에 `부채:` 는 없다: 세 줄 다
/// **가이드에 싣지 않는 것이 옳다** 는 판정이고, 판정 근거가 셋 다 서로 다르다 —
/// 빌드 조합에 없다(`debug`) · 출하되지 않는다(`agent-stream`) · 출하되지만 사용자가
/// 수행하는 조작이 아니다(`completion-strategy`).
///
/// ★ 출하 여부를 **모수에서** 빼지 않고 여기 적는 이유: 모수에서 빼면 그 명령이 존재한
/// 적도 없는 것처럼 보인다. `debug` 를 처음부터 이 명부에 둔 것과 같은 판단이다 —
/// 명령은 있고, 그것이 사용자에게 닿지 않는다는 **판정**이 여기 적힌다. `bundle` 이
/// 참으로 바뀌면 그때 이 줄이 거짓이 되고, 거짓이 된 줄은 사람이 읽어서 지운다.
const NOT_IN_THE_GUIDE: &[(&str, &str)] = &[
    (
        "debug",
        "debug 빌드에만 있는 명령이다. 설치해서 쓰는 사람에게는 존재하지 않는다",
    ),
    (
        "agent-stream",
        "출하되지 않는다 — `tasty-plugin-agent-stream` 은 매니페스트가 `bundle = false` 라 \
         배포 패키징에 안 들어간다. 설치해서 쓰는 사람에게는 그 명령이 애초에 없다",
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
/// `site/content` 의 최상위 갈래. **순회 밖에 있어야** 가지치기가 넓어져 한 갈래가
/// 통째로 빠진 것을 잡는다 — 순회가 본 것으로 이 목록을 만들면 빠진 갈래는 목록에서도
/// 빠진다. `en` 은 번역이라 순회가 일부러 건너뛰므로 여기 없다.
const GUIDE_BRANCHES: &[&str] = &[
    "agents",
    "customize",
    "getting-started",
    "help",
    "plugins",
    "remote",
    "using",
];

/// 가이드 본문 한 벌. **갈래마다 하나라도 닿았는지 확인하고 돌려준다.**
///
/// 이 함수가 돌려주는 문자열은 아래 판정들의 **우변**이다. 좌변(등록 명부·명령 목록)은
/// 코드 상수라 절대 안 비는데, 우변이 조용히 줄면 "가이드에 없다"·"가이드에 이미 있다"
/// 가 **둘 다 초록**이 된다. 그래서 두 자리를 막는다.
///
/// 1. `read_dir` 실패를 **안 삼킨다.** 예전 판은 `let Ok(..) else { return }` 이라
///    권한·경합으로 한 디렉토리를 못 읽으면 그 갈래가 통째로 빠진 채 초록이었다.
/// 2. `site/content` 의 **최상위 갈래마다** `.md` 를 하나라도 담았는지 본다.
///
/// **실측 2026-09-08(`12bc0f4b2`)**: 갈래 하나를 순회에서 빼고 세 파일을 돌리는 변이를
/// 7 갈래 × 3 파일 = 21 칸으로 재니 **15 칸이 초록**이었다. `help/` 와 `plugins/` 는
/// 세 파일 **전부**가 못 잡았다. 지금은 21 칸 전부가 이 함수에서 죽는다.
///
/// **명부를 순회 밖에 둔다 — 첫 판은 순회 안에서 갈래를 모았고 그것이 틀렸다.**
/// 순회가 본 갈래만 모으면 가지치기로 빠진 갈래는 **목록에도 안 들어가서** 확인 대상이
/// 아니게 된다. 위 21 칸 변이를 그 판에 대고 재니 15 칸이 그대로 초록이었다 — 좌변을
/// 재는 사본과 판정하는 사본이 같으면 그 둘이 함께 줄어든다(R1116 과 같은 형태다).
/// 그래서 [`GUIDE_BRANCHES`] 는 상수고, 그 명부가 낡는 것은 반대 방향 판정이 잡는다.
/// 하한(`>= N`)도 안 쓴다. 갈래 확인은 여유가 필요 없고, 순회가 통째로 죽는 것과 한
/// 갈래만 빠지는 것을 같은 판정으로 잡는다.
fn guide_text(root: &Path) -> String {
    guide_scan(root).0
}

/// 순회 본체. 본문 · **명부와 대조할 두 집합**을 함께 낸다.
///
/// 갈래 확인이 [`guide_text`] 안에 있으면 **합성 트리를 먹는 형제 시험이 깨진다** —
/// 그 트리에는 레포의 갈래 일곱이 없다. 그래서 순회와 판정을 갈랐다: 여기서는 읽기
/// 실패만 막고, 명부 대조는 레포를 상대로만 도는 별도 시험이 한다.
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
                "가이드 순회가 {} 를 못 읽었다 — {e}\n\
                 조용히 건너뛰면 그 갈래가 통째로 빠진 채 이 파일의 판정이 초록으로 \
                 나온다. 우변이 비면 \"가이드에 없다\" 도 \"가이드에 이미 있다\" 도 \
                 참이 된다.",
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

/// [`GUIDE_BRANCHES`] 의 판정 — 순회가 **갈래마다 하나라도 닿았는가**, 그리고 그 명부가
/// 낡지 않았는가. 두 방향이라 갈래가 빠져도, 늘어도 잡힌다.
#[test]
fn the_guide_walk_reaches_every_branch() {
    let (_, branches, touched) = guide_scan(&repo_root());
    let missing: Vec<&&str> = GUIDE_BRANCHES
        .iter()
        .filter(|b| !touched.contains(**b))
        .collect();
    assert!(
        missing.is_empty(),
        "가이드 순회가 이 갈래에서 `.md` 를 하나도 안 담았다: {missing:?}\n\
         명부 {} 개 중 {} 개만 닿았다. 가지치기가 넓어졌거나 그 디렉토리가 비었다 — \
         이 파일의 판정은 우변이 줄면 **더 조용히** 초록이 되므로, 갈래를 빼서 \
         통과시키지 마라.",
        GUIDE_BRANCHES.len(),
        touched.len()
    );
    let extra: Vec<&String> = branches
        .iter()
        .filter(|b| !GUIDE_BRANCHES.contains(&b.as_str()))
        .collect();
    assert!(
        extra.is_empty(),
        "`site/content` 에 명부에 없는 갈래가 있다: {extra:?}\n\
         `GUIDE_BRANCHES` 에 추가해라 — 안 하면 그 갈래는 위 확인의 대상이 아니라서 \
         통째로 빠져도 초록이다."
    );
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

/// **양성 대조 — 대조가 없던 두 판독.**
///
/// `core_commands` 는 이미 [`the_reader_answers_both_yes_and_no`] 가 합성 소스로 건다.
/// 안 걸려 있던 것은 [`plugin_commands`] 와 [`guide_text`] 다 — 둘 다 디스크를 훑고,
/// 둘 다 실패를 **조용한 빈손**으로 넘긴다(`read_dir`·`read_to_string` 의 `let Ok`).
///
/// 이 가드의 수치 레버는 `MIN_COMMANDS` 하나뿐이고 그것은 하한이라 **좁아지는 쪽만**
/// 본다. 그런데 여기서 두 판독이 틀리는 방향은 서로 반대다:
///
///   - `plugin_commands` 가 **덜** 걷으면 그 명령은 가이드 요구에서 통째로 빠진다.
///     조용한 구멍이고, `MIN_COMMANDS` 는 core 명령만으로도 채워져서 안 짖는다.
///   - `guide_text` 가 **더** 걷으면(번역·비-`.md`) 본문이 넘쳐 "가이드에 있다" 가
///     쉽게 참이 된다. 하한이 원리적으로 못 보는 방향이다.
///
/// ★ 이름은 전부 합성이다(R1078) — 진짜 plugin 이름이나 명령 이름을 안 쓴다.
#[test]
fn the_plugin_and_guide_readers_answer_on_a_substituted_tree() {
    let probe = Scratch::new("cli-guide-reader");
    let dir = probe.path();

    // ── 판독 1: plugin 이 기여한 명령 ───────────────────────────────────
    let mk = |name: &str, body: &str| {
        let d = dir.join("crates").join(name);
        std::fs::create_dir_all(&d).expect("합성 크레이트 디렉토리를 만들지 못했다");
        std::fs::write(d.join("tasty-plugin.toml"), body).expect("합성 매니페스트 실패");
    };
    mk(
        "zeta-plugin",
        "[[contributes.cli]]\nname = \"zeta-cmd\"\ndescription = \"z\"\n",
    );
    // 헤더와 `name` 사이의 빈 줄·주석은 건너뛴다. 그리고 한 매니페스트에 둘 이상.
    mk(
        "omega-plugin",
        "[[contributes.cli]]\n\n# 주석 한 줄\nname = \"omega-one\"\n\n\
         [[contributes.cli]]\nname = \"omega-two\"\n",
    );
    // `[[contributes.cli]]` 가 없는 매니페스트 — 아무것도 안 낸다.
    mk("sigma-plugin", "[plugin]\nid = \"sigma\"\n");
    // 매니페스트가 없는 디렉토리 — 건너뛴다(라이브러리 크레이트가 이 모양이다).
    std::fs::create_dir_all(dir.join("crates/no-manifest/src"))
        .expect("합성 무매니페스트 디렉토리 실패");

    let got = plugin_commands(dir);
    assert_eq!(
        got,
        vec![
            ("omega-one".to_string(), "omega-plugin".to_string()),
            ("omega-two".to_string(), "omega-plugin".to_string()),
            ("zeta-cmd".to_string(), "zeta-plugin".to_string()),
        ],
        "plugin 명령 판독이 합성 트리에서 다른 답을 냈다"
    );
    assert!(
        got.iter().all(|(_, owner)| !owner.is_empty()),
        "소유 크레이트 이름을 못 붙였다 — 실패문이 어느 plugin 인지 못 가리킨다"
    );
    assert!(
        !got.iter().any(|(c, _)| c == "sigma"),
        "`[[contributes.cli]]` 가 없는 매니페스트에서 명령을 만들어 냈다"
    );

    // ── 판독 2: 가이드 본문 ─────────────────────────────────────────────
    let content = dir.join("site/content");
    std::fs::create_dir_all(content.join("en")).expect("합성 가이드 트리를 만들지 못했다");
    std::fs::create_dir_all(content.join("sub")).expect("합성 하위 장을 만들지 못했다");
    std::fs::write(content.join("cli.md"), "zeta-cmd 를 설명한다\n").expect("합성 원본 실패");
    std::fs::write(
        content.join("sub").join("deep.md"),
        "deep-marker 가 여기 있다\n",
    )
    .expect("합성 하위 원본 실패");
    std::fs::write(content.join("en").join("cli.md"), "translated-omega\n")
        .expect("합성 번역 실패");
    std::fs::write(content.join("notes.txt"), "md 가 아니다 sigma-decoy\n").expect("잡파일 실패");

    let guide = guide_text(dir);
    assert!(guide.contains("zeta-cmd"), "원본 `.md` 를 안 읽었다");
    assert!(guide.contains("deep-marker"), "하위 디렉토리로 안 내려갔다");
    assert!(
        !guide.contains("translated-omega"),
        "번역(`en/`)이 원본에 섞였다 — 본문이 넘치면 \"가이드에 있다\" 가 쉽게 참이 되고, \
         하한은 그 방향을 원리적으로 못 본다"
    );
    assert!(
        !guide.contains("sigma-decoy"),
        "`.md` 가 아닌 파일을 읽었다 — 같은 방향이다"
    );
}

/// **경계를 값으로 적어 둔다 — `name` 이 첫 필드가 아니면 그 명령은 안 보인다.**
///
/// [`plugin_commands`] 는 `[[contributes.cli]]` 뒤의 **첫** 비어 있지 않은 비주석 줄만
/// 보고 `break` 한다. 그 줄이 `name` 이 아니면 그 명령은 모수에서 통째로 빠지고,
/// 빠진 명령은 가이드에 없어도 아무도 안 짖는다 — `MIN_COMMANDS` 는 core 명령만으로
/// 채워지므로 하한도 안 걸린다.
///
/// 지금은 **잠복**이다. 실측(2026-09-08): 번들 매니페스트 6 개가 전부 `name` 을 첫
/// 필드로 둔다. 그래서 이것은 지금 나는 고장이 아니라 **한 줄 순서만 바뀌면 조용해지는
/// 자리**이고, 그 사실을 시험으로 박아 둔다 — 고치는 날 이 시험이 함께 빨개져서
/// "의도한 변경" 이라는 것이 값으로 남는다.
///
/// 양성 대조(2026-09-08, 내 트리): [`plugin_commands`] 의 `break` 를 `name` 을 찾은 뒤로
/// 옮겨 판독을 넓히면 이 시험만 rc=101 로 죽는다(패키지 568 passed / 1 failed).
/// **실물 판정은 안 움직인다** — 번들 매니페스트 6 개가 전부 `name` 을 첫 필드로 두어
/// 넓혀도 같은 답이 나오기 때문이다. 그것이 이 경계 시험이 더하는 구간의 전부이자
/// 이유다: 실물이 조용한 회귀를 합성 매니페스트 하나가 소리나게 만든다.
#[test]
fn a_command_whose_name_is_not_the_first_field_is_currently_invisible() {
    let probe = Scratch::new("cli-latent");
    let dir = probe.path();
    let d = dir.join("crates/latent-plugin");
    std::fs::create_dir_all(&d).expect("합성 크레이트 디렉토리를 만들지 못했다");
    std::fs::write(
        d.join("tasty-plugin.toml"),
        "[[contributes.cli]]\ndescription = \"설명이 먼저 온다\"\nname = \"latent-cmd\"\n",
    )
    .expect("합성 매니페스트 실패");

    let got = plugin_commands(dir);
    assert!(
        got.is_empty(),
        "이 판독이 넓어졌다 — `name` 이 첫 필드가 아닌 자리도 이제 보인다: {got:?}\n\
         ★ 그것이 **의도한 개선이면** 이 시험을 지우고 위 doc 주석의 '잠복' 서술도 함께 \
         지워라. 의도하지 않았으면 판독이 두 곳을 다르게 세고 있다는 뜻이다."
    );
}
