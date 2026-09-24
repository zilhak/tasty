//! 최상위 CLI 명령이 한국어 사용자 가이드에 나오거나 사유를 적은 예외 목록에 있는지 확인한다.
//! core의 Commands enum과 플러그인 매니페스트의 contributes.cli를 함께 읽는다.
//! 명령의 설명 품질·영어 번역·하위 명령은 검사하지 않는다. tasty <명령> 문자열이 한 번 있으면 기재된 것으로 본다.
//! doc-guards.yml의 경로 필터 없는 main push·PR에서 실행된다.

use std::collections::BTreeSet;

use std::path::{Path, PathBuf};
use tasty_doc_guards::temp_scratch::Scratch;

/// 가이드에 싣지 않는 명령과 사유. 아직 작성하지 못한 경우는 부채:로 구별한다.
/// 기본 배포에 포함되지 않는 명령도 수집에서 제거하지 않고 여기에서 제외 이유를 기록한다.
/// 배포 방식이 바뀌면 사유를 다시 검토해야 한다.
const NOT_IN_THE_GUIDE: &[(&str, &str)] = &[
    (
        "debug",
        "debug 빌드에만 있는 명령이다. 설치해서 쓰는 사람에게는 존재하지 않는다",
    ),
    (
        "agent-stream",
        "tasty-plugin-agent-stream은 bundle=false라 기본 배포 패키지에 포함되지 않는다. 기본 사용자 가이드 대신 개발 문서에서 다룬다.",
    ),
    (
        "completion-strategy",
        "list로 내부 완료 판정 전략의 등록 목록을 조회하는 개발·디버깅 명령이다. 사용자에게 보이는 완료 알림은 에이전트 가이드에서 설명하고 내부 전략은 docs/에서 다룬다.",
    ),
];

/// 2026-09-06 실측 42(core36·plugin6)에 여유를 둔 하한 35. 감소 시 실제 명령 목록과 판독을 대조한다.
const MIN_COMMANDS: usize = 35;

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

/// 대문자 앞에 하이픈을 넣고 소문자로 바꾼다. 연속 대문자 약어도 글자마다 나뉜다.
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

/// 들여쓰기 형태로 최상위 변이를 읽고 바로 앞 줄의 debug cfg를 표시한다.
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

/// contributes.cli의 첫 비어 있지 않은 비주석 줄이 name인 경우만 읽는다.
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

/// 한국어 가이드의 최상위 분류. 순회에서 빠진 분류를 찾도록 수집 결과와 독립된 목록으로 둔다.
const GUIDE_BRANCHES: &[&str] = &[
    "agents",
    "customize",
    "getting-started",
    "help",
    "plugins",
    "remote",
    "using",
];

/// 가이드 본문을 수집한다. 최상위 분류별 수집 여부는 별도 검사에서 확인한다.
fn guide_text(root: &Path) -> String {
    guide_scan(root).0
}

/// 합성 트리에는 실제 분류가 없으므로 순회와 분류 명부 대조를 분리한다.
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
                "가이드 디렉터리를 읽지 못했다: {} — {e}. 수집 누락을 설명 부재로 오해하지 않도록 실패시킨다.",
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

/// 분류 추가와 기존 분류의 수집 누락을 양방향으로 확인한다.
#[test]
fn the_guide_walk_reaches_every_branch() {
    let (_, branches, touched) = guide_scan(&repo_root());
    let missing: Vec<&&str> = GUIDE_BRANCHES
        .iter()
        .filter(|b| !touched.contains(**b))
        .collect();
    assert!(
        missing.is_empty(),
        "다음 가이드 분류에서 Markdown을 수집하지 못했다: {missing:?}. 등록 {}개 중 {}개를 읽었다. 경로 제외와 디렉터리 상태를 확인한다.",
        GUIDE_BRANCHES.len(),
        touched.len()
    );
    let extra: Vec<&String> = branches
        .iter()
        .filter(|b| !GUIDE_BRANCHES.contains(&b.as_str()))
        .collect();
    assert!(
        extra.is_empty(),
        "site/content에 미등록 분류가 있다: {extra:?}. GUIDE_BRANCHES에 추가해 수집 누락 검사에 포함한다."
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
        "CLI 명령을 {total}개만 읽었다(하한 {MIN_COMMANDS}). 실제 명령 목록과 enum·매니페스트 판독을 대조한다."
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
        "한국어 사용자 가이드와 예외 목록에 없는 CLI 명령이다:\n{}\n사용자가 쓰는 명령은 site/content에 사용법을 설명한다. 사용자를 위한 명령이 아니라면 NOT_IN_THE_GUIDE에 구체적인 사유를 등록한다. 아직 쓰지 못한 설명은 정책 예외와 구별해 부채로 기록한다.",
        undocumented.join("\n")
    );
}

/// 가이드에 설명을 추가했다면 오래된 제외 항목도 제거한다.
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
        "가이드에 설명했지만 제외 목록에 남은 명령이다: {stale:?}. 완료한 항목을 목록에서 제거한다."
    );
}

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

    assert!(core_commands("fn main() {}").is_empty());
}

#[test]
fn the_kebab_conversion_matches_clap() {
    assert_eq!(kebab("New"), "new");
    assert_eq!(kebab("SurfaceMeta"), "surface-meta");
    assert_eq!(kebab("CompletionStrategy"), "completion-strategy");
}

/// 플러그인 명령 누락과 번역·비 Markdown 문서의 혼입은 서로 다른 오류이므로 각각 검증한다.
/// 명령 총수 하한은 core 명령만으로 채워질 수 있어 플러그인 수집 누락을 보장하지 못한다.
#[test]
fn the_plugin_and_guide_readers_answer_on_a_substituted_tree() {
    let probe = Scratch::new("cli-guide-reader");
    let dir = probe.path();

    let mk = |name: &str, body: &str| {
        let d = dir.join("crates").join(name);
        std::fs::create_dir_all(&d).expect("합성 크레이트 디렉토리를 만들지 못했다");
        std::fs::write(d.join("tasty-plugin.toml"), body).expect("합성 매니페스트 실패");
    };
    mk(
        "zeta-plugin",
        "[[contributes.cli]]\nname = \"zeta-cmd\"\ndescription = \"z\"\n",
    );
    mk(
        "omega-plugin",
        "[[contributes.cli]]\n\n# 주석 한 줄\nname = \"omega-one\"\n\n\
         [[contributes.cli]]\nname = \"omega-two\"\n",
    );
    mk("sigma-plugin", "[plugin]\nid = \"sigma\"\n");
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
        "영어 번역이 한국어 가이드 수집에 섞였다"
    );
    assert!(
        !guide.contains("sigma-decoy"),
        "`.md` 가 아닌 파일을 읽었다 — 같은 방향이다"
    );
}

/// 현재 plugin_commands는 name이 첫 필드가 아니면 명령을 읽지 못한다.
/// 판독 범위를 바꿀 때 실제 매니페스트만으로는 드러나지 않을 수 있어 합성 입력으로 이 한계를 고정한다.
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
        "name이 첫 필드가 아닌 명령도 읽도록 판독 범위가 달라졌다: {got:?}. 의도한 개선이라면 이 한계 검사와 설명을 함께 갱신한다."
    );
}
