//! 워크플로의 `on:` 트리거를 **구조로** 읽는다 — 그 파일이 매 push 도는가, 아니면
//! 경로 필터 뒤에 있는가.
//!
//! 이 판독이 따로 필요한 이유는 **주석과 트리거 키가 같은 글자를 쓰기 때문**이다.
//! `text.contains("paths-ignore:")` 로 세면 다른 워크플로의 필터를 *설명하는* 주석을
//! 가진 파일이 필터를 *가진* 파일로 읽힌다. 실측(2026-09-05): 레포의 워크플로 11 개 중
//! 정확히 하나가 그 형태였고, 하필 그것이 **필터가 없다는 것 자체가 존재 이유인**
//! `doc-guards.yml` 이다(ADR-0138). 그 오탐은 "필터 없는 채널이 하나도 없다" 쪽으로
//! 기울어 안전한 방향이지만, 안전한 방향의 오탐도 판정을 못 쓰게 만드는 것은 같다.
//!
//! **이 판독이 답하지 않는 것**: 그 필터가 실제로 어떤 push 를 걸렀는가. 어느 커밋들이
//! 한 push 였는지는 git 에 없다 — 그 수는 워크플로의 run 목록으로만 재고, 재는 법은
//! `docs/dev-guide/ci-gates.md` 에 있다. 여기서 보는 것은 **필터의 유무**뿐이다.

/// 한 워크플로의 `push:` 트리거 모양.
#[derive(Debug, PartialEq, Eq)]
pub struct PushTrigger {
    /// `on:` 블록에 `push:` 가 있는가.
    pub present: bool,
    /// 그 `push:` 아래에 `paths:` 나 `paths-ignore:` 가 붙었는가.
    pub path_filtered: bool,
    /// 그 `push:` 가 `tags:` 만 걸고 `branches:` 를 안 거는가.
    ///
    /// 필터가 없다는 것과 **보통 push 마다 돈다**는 것은 다르다. 태그 전용 트리거는
    /// 경로 필터가 하나도 없어도 일상 커밋에서는 안 뜬다 — 그것을 "필터 없는 채널" 로
    /// 세면 릴리스 워크플로가 매 push 채널인 척하게 된다(실측: `release.yml`).
    pub tags_only: bool,
}

/// YAML 주석을 지운다. `#` 은 **줄 첫 토큰이거나 공백 뒤에 올 때만** 주석이다 —
/// 그것이 YAML 의 규칙이고, 안 지키면 `'**/*.md#x'` 같은 값의 일부를 주석으로 잘라
/// 뒤따르는 키를 통째로 잃는다(있는 필터를 없다고 읽는 쪽이라 더 나쁘다).
fn strip_yaml_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let bytes = line.as_bytes();
        let mut cut = line.len();
        for (i, b) in bytes.iter().enumerate() {
            if *b == b'#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
                cut = i;
                break;
            }
        }
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    out
}

/// `on:` 블록의 줄들. 최상위 `on:`(들여쓰기 0)부터 다음 최상위 키 직전까지.
///
/// `on: [push]` 같은 **인라인 흐름 표기**는 이 판독이 안 다룬다. 그 형태를 만나면
/// [`push_trigger`] 가 `None` 을 내고, 부르는 쪽은 그것을 통과가 아니라 **판정 불가**로
/// 다뤄야 한다 — 모르는 것을 초록으로 바꾸면 그 순간부터 아무것도 안 보게 된다.
fn on_block(text: &str) -> Option<Vec<&str>> {
    let mut lines = text.lines();
    // 찾은 줄 자체는 안 쓴다 — 이터레이터를 그 줄 **다음**으로 진행시키는 것이 목적이고,
    // 없으면 `?` 로 판정 불가를 낸다.
    lines.by_ref().find(|l| {
        let t = l.trim_end();
        t == "on:" || t == "\"on\":" || t == "'on':"
    })?;
    let mut block = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        // 다음 최상위 키에서 끝난다.
        if !line.starts_with(' ') && !line.starts_with('\t') {
            break;
        }
        block.push(line);
    }
    Some(block)
}

/// 한 줄의 들여쓰기 폭(공백 수).
fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// **답하는 물음**: 이 워크플로의 `push:` 가 매 push 도는가 — 경로 필터가 붙었는가,
/// 태그 전용인가.
///
/// 워크플로 본문에서 `push:` 트리거의 모양을 읽는다.
///
/// `None` 은 "필터가 없다" 가 아니라 **"이 판독기가 못 읽는 모양이다"** 다.
/// 최상위 `on:` 이 블록 표기가 아닐 때 그렇게 된다.
pub fn push_trigger(text: &str) -> Option<PushTrigger> {
    let stripped = strip_yaml_comments(text);
    let block = on_block(&stripped)?;
    let Some(at) = block
        .iter()
        .position(|l| indent(l) == 2 && l.trim_end().trim_start() == "push:")
    else {
        return Some(PushTrigger {
            present: false,
            path_filtered: false,
            tags_only: false,
        });
    };
    let mut path_filtered = false;
    let mut has_tags = false;
    let mut has_branches = false;
    for line in &block[at + 1..] {
        // 다음 트리거(같은 깊이)에서 끝난다.
        if indent(line) <= 2 {
            break;
        }
        let key = line.trim_start();
        if key.starts_with("paths:") || key.starts_with("paths-ignore:") {
            path_filtered = true;
        }
        if key.starts_with("tags:") || key.starts_with("tags-ignore:") {
            has_tags = true;
        }
        if key.starts_with("branches:") || key.starts_with("branches-ignore:") {
            has_branches = true;
        }
    }
    Some(PushTrigger {
        present: true,
        path_filtered,
        tags_only: has_tags && !has_branches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 이 판독기가 존재하는 이유 그 자체. 주석이 다른 워크플로의 필터를 설명해도
    /// 이 파일에는 필터가 없다.
    #[test]
    fn a_comment_mentioning_a_filter_is_not_a_filter() {
        let yaml = "\
# crossplatform-check.yml 은 paths-ignore: docs/** 뒤에 있다.
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  x:
";
        assert_eq!(
            push_trigger(yaml),
            Some(PushTrigger {
                present: true,
                path_filtered: false,
                tags_only: false
            })
        );
    }

    #[test]
    fn a_real_filter_under_push_is_seen() {
        let yaml = "\
on:
  push:
    branches: [main]
    paths-ignore:
      - 'docs/**'
  pull_request:
    branches: [main]
";
        assert!(push_trigger(yaml).unwrap().path_filtered);
    }

    /// 다른 트리거의 필터는 `push:` 의 필터가 아니다. 깊이로 끊지 않으면 여기서 샌다.
    #[test]
    fn a_filter_on_another_trigger_does_not_leak_into_push() {
        let yaml = "\
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
    paths:
      - 'src/**'
";
        assert_eq!(
            push_trigger(yaml),
            Some(PushTrigger {
                present: true,
                path_filtered: false,
                tags_only: false
            })
        );
    }

    #[test]
    fn a_workflow_without_push_says_so() {
        let yaml = "on:\n  workflow_dispatch:\n";
        assert_eq!(
            push_trigger(yaml),
            Some(PushTrigger {
                present: false,
                path_filtered: false,
                tags_only: false
            })
        );
    }

    /// 태그 전용 트리거는 필터가 없어도 매 push 채널이 아니다.
    #[test]
    fn a_tags_only_push_is_not_an_every_push_channel() {
        let yaml = "on:\n  push:\n    tags: ['v*']\n  workflow_dispatch:\n";
        let t = push_trigger(yaml).unwrap();
        assert!(t.present && !t.path_filtered && t.tags_only);
    }

    /// 브랜치를 함께 걸면 태그 전용이 아니다.
    #[test]
    fn a_push_naming_branches_is_not_tags_only() {
        let yaml = "on:\n  push:\n    branches: [main]\n    tags: ['v*']\n";
        assert!(!push_trigger(yaml).unwrap().tags_only);
    }

    /// 못 읽는 모양은 통과가 아니라 판정 불가다.
    #[test]
    fn an_inline_trigger_is_unreadable_rather_than_unfiltered() {
        assert_eq!(push_trigger("on: [push]\njobs:\n"), None);
    }

    /// 값 안의 `#` 은 주석이 아니다. 잘라 내면 뒤따르는 키를 잃는다.
    #[test]
    fn a_hash_inside_a_value_is_not_a_comment() {
        let yaml = "\
on:
  push:
    branches: [main]
    paths:
      - 'a#b/**'
";
        assert!(push_trigger(yaml).unwrap().path_filtered);
    }
}

/// 경로 필터 없는 채널이 덮는 테스트 타깃의 집합.
///
/// **왜 한 벌인가**: 이 물음("필터 없는 채널이 이 타깃을 덮는가")에 답하는 것이 한때
/// 둘이었고, 둘이 **같은 항목에 다른 답**을 냈다. 실측(2026-09-05) — `doc-guards.yml` 의
/// 호출을 `--test` 하나로 좁히는 변이에서 `filter_free_channel_still_exists` 는 발화했고
/// `filtered_guards_are_not_totally_blind` 는 침묵했다. 뒤쪽이 `--test <이름>` 만 읽고
/// `-p <패키지>` 를 안 읽었기 때문이고, 그 구멍을 **디렉토리 이름 상수로 건너뛰기**로
/// 메우고 있었다. 이름으로 면제하면 성질이 같고 이름이 다른 것이 통째로 빠진다(ADR-0175).
///
/// 반대로 **합치지 않은 것도 있다.** `ci_channel_claims_match_workflows` 는 "자동 채널이
/// 있는가(언젠가라도)" 를 묻고 여기서는 "매 push 도는가" 를 묻는다. `test.yml` 에 필터를
/// 다는 변이에서 앞은 침묵하고 뒤는 발화하는데 **둘 다 자기 물음에는 맞다** — 그러면
/// 물음이 둘이라 사본도 둘이다.
#[derive(Debug, Default)]
pub struct FilterFreeCoverage {
    /// `--test <이름>` 으로 지목된 타깃 이름들.
    pub named: std::collections::BTreeSet<String>,
    /// `-p <패키지>` 로 **좁혀지지 않고** 불린 패키지들.
    pub packages: std::collections::BTreeSet<String>,
    /// `--workspace` 를 좁힘 없이 부르는 잡이 있는가 — 있으면 전부 덮인다.
    pub whole_workspace: bool,
}

/// 호출을 특정 타깃/종류로 좁히는 플래그.
const NARROWING: &[&str] = &["--test", "--lib", "--bins", "--bin", "--doc", "--example"];

/// 주석·스텝 이름을 지우고 한 줄로 편다.
///
/// 스텝 이름을 지우는 이유는 이 레포의 스텝 이름이 명령을 그대로 쓰기 때문이다
/// (`- name: cargo test -p tasty-doc-guards`). 안 지우면 **이름이 채널로 읽혀**, 실행
/// 스텝이 좁혀졌는데도 안 좁혀진 옛 이름을 들고 있으면 통과한다.
///
/// 한 줄로 펴는 이유는 `run: |` 블록과 `run: >` 접힌 스칼라, 줄 끝 `\` 이음이 전부 한
/// 명령을 여러 줄에 나누기 때문이다. 줄 단위로 보면 `cargo test ... \` 에서 끊겨 뒤에
/// 오는 플래그를 놓친다 — **있는 좁힘을 없다고 판정하는 쪽이라 더 나쁘다.** 실측으로
/// 이 함정에 걸릴 뻔했다: `test.yml` 의 semver-guards 는 접힌 스칼라로 `--test` 셋을
/// 거는데, 줄 단위로 보면 좁혀지지 않은 전체 호출로 보인다.
fn flatten(yaml: &str) -> String {
    yaml.replace("\r\n", "\n")
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with('#') && !t.starts_with("- name:") && !t.starts_with("name:")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `jobs:` 아래의 잡 본문 중 **자동 회차에 도는 것**.
///
/// 잡 단위로 갈라야 하는 이유: 워크플로에 필터가 없어도 그 안의 잡이 이벤트 조건으로
/// 수동 전용일 수 있다. 파일 단위로 보면 그 잡의 명령이 자동 채널로 읽힌다 — 실측으로
/// 이 함정에 걸렸다(2026-09-05): `test.yml` 은 필터가 없고 `cargo test --workspace` 를
/// 들고 있지만 그 잡은 수동 전용이라, 잡을 안 가른 첫 판에서 판정이 **잘못된 이유로**
/// 초록이었다.
/// **답하는 물음**: 이 워크플로의 잡 중 **자동 회차에 도는 것**은 어느 것인가.
/// (수동 전용 조건이 붙은 잡을 뺀 나머지. 트리거·경로필터는 [`push_trigger`] 가 본다.)
///
/// **밖에서 부를 수 있게 `pub` 이다.** 이 판정을 셸이나 일회용 스크립트로 흉내 내면
/// 갈린다 — 실측(2026-09-05): 이 레포에서 하루에 세 레인이 각자 미러를 만들었고 셋 다
/// 원본과 다른 답을 냈다. 갈리는 방향은 대체로 **덜 잡는 쪽**이라 조용하다.
/// 부르는 길은 `workflow-channels` 판정기 바이너리다.
///
/// **잡 헤더 판정은 2 칸 들여쓰기 관례에 매달려 있다.** 관례를 깨는 워크플로가 오면
/// 잡 헤더가 하나도 안 잡혀 파일 전체가 한 덩어리가 되고, 그러면 그 안의 수동 전용
/// 조건 하나가 **파일 전체의 호출을 통째로** 지운다 — 또 줄이는 방향이다.
/// 실측(2026-09-05): 레포의 워크플로 11 개가 전부 2 칸이라 지금은 안 걸린다.
pub fn automatic_job_bodies(yaml: &str) -> Vec<String> {
    let mut bodies = Vec::new();
    let mut current = String::new();
    let mut in_jobs = false;
    for line in yaml.replace("\r\n", "\n").lines() {
        if line.starts_with("jobs:") {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            continue;
        }
        let is_job_head =
            line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':');
        if is_job_head && !current.is_empty() {
            bodies.push(std::mem::take(&mut current));
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.is_empty() {
        bodies.push(current);
    }
    // ★ **이 술어는 줄이는 방향으로 틀린다** — 그 방향의 오차는 언제나 더 초록이라
    // 안 보인다. 문자열이 들어 있기만 하면 버리므로, `if:` 가 **분리(∨)** 인 잡도
    // 수동 전용으로 센다. 실측(2026-09-05): `release.yml` 의 빌드 잡 4 개가 그 형태다
    // (`always() && (needs... == 'success' || event_name == 'workflow_dispatch')`) —
    // 태그 push 에서도 도는데 여기서 버려진다.
    //
    // **오늘 그 누락의 효과는 0 이고, 그 0 을 쟀다**: `release.yml` 에는 `cargo test`
    // 호출이 하나도 없어서 커버리지에 기여할 것이 애초에 없고, 그 파일은 태그 전용이라
    // [`push_trigger`] 단계에서 이미 빠진다. 그래서 지금 고치지 않는다 — 다만 그 파일이
    // 언젠가 `cargo test` 를 들이면 **조용히** 안 보이게 된다. 그때는 술어를 "순수 조건
    // (`if: github.event_name == 'workflow_dispatch'`)일 때만 버린다" 로 좁혀라.
    //
    // 같은 술어를 `ci_channel_claims_match_workflows` 도 쓴다. 답을 둘로 만들지 않으려고
    // 형태를 맞춰 둔 것이고, 위 한계도 그대로 공유한다.
    bodies
        .into_iter()
        .filter(|b| !b.contains("github.event_name == 'workflow_dispatch'"))
        .collect()
}

/// 평탄화된 본문에서 `cargo test` 호출을 하나씩 잘라낸다. 각 조각은 다음 `cargo ` 직전
/// 까지라, 한 스텝에 명령이 여럿이어도 플래그가 섞이지 않는다.
fn cargo_test_invocations(flat: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = flat[from..].find("cargo test") {
        let start = from + rel;
        let rest = &flat[start + "cargo test".len()..];
        let end = rest
            .find("cargo ")
            .map_or(flat.len(), |n| start + "cargo test".len() + n);
        out.push(&flat[start..end]);
        from = start + "cargo test".len();
    }
    out
}

/// **답하는 물음**: 경로 필터 없이 매 push 도는 잡이 어느 테스트 타깃을 덮는가.
///
/// 워크플로 디렉토리 전체에서 [`FilterFreeCoverage`] 를 읽는다.
///
/// `on:` 을 못 읽는 워크플로가 있으면 `Err` 로 그 이름들을 낸다 — **판정 불가는 통과가
/// 아니다.** 모르는 것을 빈 집합으로 바꾸면 덮인 타깃을 안 덮인 것으로 세고, 그 방향의
/// 오답은 "이미 덮인 가드를 옮겨라" 라는 거짓 요구가 된다.
pub fn filter_free_coverage(
    workflows: &std::path::Path,
) -> Result<FilterFreeCoverage, Vec<String>> {
    let mut out = FilterFreeCoverage::default();
    let mut unreadable = Vec::new();
    let entries = match std::fs::read_dir(workflows) {
        Ok(e) => e,
        Err(e) => return Err(vec![format!("{}: {e}", workflows.display())]),
    };
    let mut paths: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "yml" || x == "yaml"))
        .collect();
    paths.sort();
    for path in paths {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let Ok(text) = std::fs::read_to_string(&path) else {
            unreadable.push(name);
            continue;
        };
        let Some(trigger) = push_trigger(&text) else {
            unreadable.push(name);
            continue;
        };
        if !trigger.present || trigger.path_filtered || trigger.tags_only {
            continue;
        }
        for body in automatic_job_bodies(&text) {
            let flat = flatten(&body);
            for inv in cargo_test_invocations(&flat) {
                let words: Vec<&str> = inv.split_whitespace().collect();
                for w in words.windows(2) {
                    if w[0] == "--test" {
                        out.named.insert(w[1].to_string());
                    }
                }
                if words.iter().any(|w| NARROWING.contains(w)) {
                    continue;
                }
                for w in words.windows(2) {
                    if w[0] == "-p" || w[0] == "--package" {
                        out.packages.insert(w[1].to_string());
                    }
                }
                if words.contains(&"--workspace") {
                    out.whole_workspace = true;
                }
            }
        }
    }
    if unreadable.is_empty() {
        Ok(out)
    } else {
        Err(unreadable)
    }
}

impl FilterFreeCoverage {
    /// 이 타깃이 필터 없는 채널에 덮이는가. `stem` 은 `--test` 에 쓰는 타깃 이름,
    /// `package` 는 그 타깃이 속한 패키지 이름이다.
    pub fn covers(&self, stem: &str, package: &str) -> bool {
        self.whole_workspace || self.named.contains(stem) || self.packages.contains(package)
    }
}

#[cfg(test)]
mod coverage_tests {
    use super::*;

    fn coverage() -> FilterFreeCoverage {
        filter_free_coverage(&crate::repo_root().join(".github/workflows"))
            .unwrap_or_else(|bad| panic!("`on:` 을 못 읽은 워크플로: {bad:?}"))
    }

    /// 침묵 탐지. 판독이 통째로 비면 모든 타깃이 "안 덮임" 이 되고, 그 방향의 오답은
    /// 이미 덮인 가드에 "옮겨라" 를 요구한다.
    ///
    /// **한쪽만 비는 것은 고장이 아니다** — 워크플로가 `--test` 를 안 쓰거나 `-p` 를 안
    /// 쓰는 것은 정상 설정이다. 그 자리를 "수집이 깨졌다" 로 말하면 설정 변화에 틀린
    /// 진단이 붙는다(실측으로 그 오진을 냈다: `test.yml` 에 필터를 다는 변이에서
    /// `named` 가 비었고, 그건 판독이 아니라 채널이 사라진 것이었다). 내용은 아래 두
    /// 테스트가 각각 짚는다.
    #[test]
    fn the_reader_is_not_silently_empty() {
        let c = coverage();
        assert!(
            !c.named.is_empty() || !c.packages.is_empty() || c.whole_workspace,
            "필터 없는 채널을 하나도 못 읽었다 — 판독이 깨졌거나 채널이 전부 사라졌다"
        );
    }

    /// 접힌 스칼라(`>`)로 여러 줄에 걸친 `--test` 를 읽는가. 줄 단위로 보면 이 호출은
    /// 좁혀지지 않은 것으로 보이고, 그러면 워크스페이스 전체가 덮인 것으로 센다.
    #[test]
    fn a_folded_scalar_narrowing_is_seen() {
        let c = coverage();
        assert!(
            c.named.contains("changelog_unreleased"),
            "접힌 스칼라 안의 `--test changelog_unreleased` 를 못 읽었다: {:?}",
            c.named
        );
        assert!(
            !c.packages.contains("tasty"),
            "좁혀진 호출을 패키지 전체 채널로 셌다: {:?}",
            c.packages
        );
    }

    /// ★ **분할 규칙 자체를 시험한다 — 잡이 둘일 때만 시험된다.**
    ///
    /// 이 단정이 없던 동안 잡 헤더 규칙(2 칸 들여쓰기)을 3 칸으로 바꾸는 변이가
    /// **아무 테스트도 못 죽였다**(실측 2026-09-05). 이유는 단순하다 — 그때 있던
    /// 픽스처가 전부 **잡 하나짜리**였다. 잡이 하나면 헤더를 못 찾아 파일 전체가 한
    /// 덩어리가 돼도 개수가 1 로 같다. 규칙이 시험되려면 잡이 둘이어야 한다.
    ///
    /// 그리고 둘일 때 진짜 위험이 드러난다: 헤더를 못 찾으면 **수동 전용 잡 하나가
    /// 같은 파일의 자동 잡을 함께 지운다.** 아래가 그 형태다.
    #[test]
    fn two_jobs_split_and_a_manual_one_does_not_silence_the_automatic_one() {
        let yaml = "on:\n  push:\n    branches: [main]\njobs:\n  auto:\n    steps:\n      \
                    - run: cargo test -p alpha\n  manual:\n    if: github.event_name == \
                    'workflow_dispatch'\n    steps:\n      - run: cargo test -p beta\n";
        let bodies = automatic_job_bodies(yaml);
        assert_eq!(
            bodies.len(),
            1,
            "잡 둘 중 자동인 하나만 남아야 한다 — 0 이면 헤더를 못 찾아 파일이 한 덩어리가 \
             되고 수동 전용 조건이 자동 잡까지 지운 것이다: {bodies:?}"
        );
        assert!(
            bodies[0].contains("cargo test -p alpha"),
            "자동 잡의 명령이 사라졌다: {bodies:?}"
        );
        assert!(
            !bodies.iter().any(|b| b.contains("cargo test -p beta")),
            "수동 전용 잡의 명령이 자동 채널로 새어 들어왔다: {bodies:?}"
        );
    }

    /// 레포에서도 같은 것을 묻는다 — 픽스처만으로는 관례가 실제로 그러한지 모른다.
    ///
    /// 잡이 여럿인 워크플로가 실재하므로 **잡 본문 총수 > 워크플로 파일 수** 여야 한다.
    /// 헤더 규칙이 깨지면 파일마다 최대 하나가 되어 이 부등식이 무너진다.
    #[test]
    fn the_repo_has_more_automatic_job_bodies_than_workflow_files() {
        let dir = crate::repo_root().join(".github/workflows");
        let mut files = 0usize;
        let mut bodies = 0usize;
        for e in std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
            .flatten()
        {
            let p = e.path();
            if !p.extension().is_some_and(|x| x == "yml" || x == "yaml") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            files += 1;
            bodies += automatic_job_bodies(&text).len();
        }
        assert!(
            files >= 8,
            "워크플로를 {files} 개밖에 못 읽었다 — 모수가 깨졌다"
        );
        assert!(
            bodies > files,
            "잡 본문 {bodies} 개 ≤ 워크플로 {files} 개 — 잡이 여럿인 워크플로가 실재하는데 \
             파일마다 하나 이하로 나왔다. 헤더 규칙이 깨져 파일이 한 덩어리가 된 것이다"
        );
    }

    /// 좁힘 없이 패키지를 부르는 잡이 있다 — 그것이 문서 가드들의 채널이다.
    ///
    /// ★ **양쪽을 함께 묻는다.** "덮인다" 만 재면 [`FilterFreeCoverage::covers`] 가 항상
    /// 참을 내도 통과한다 — 그리고 그 고장은 이 판독을 쓰는 모든 가드를 한꺼번에
    /// 무력화한다(덮였다고 답하면 사각이 0 이 되어 전부 초록이다). 한 방향만 재면
    /// 무정보다.
    #[test]
    fn the_coverage_answers_both_yes_and_no() {
        let c = coverage();
        assert!(
            c.covers("no_checkbox_in_docs", "tasty-doc-guards"),
            "`tasty-doc-guards` 가 필터 없는 채널에 안 덮인다: {c:?}"
        );
        assert!(
            !c.covers("a_target_no_workflow_names", "tasty-plugin-markdown"),
            "아무 워크플로도 이름으로 부르지 않고 그 패키지를 좁힘 없이 돌리지도 않는데 \
             덮였다고 답했다. 판독이 한쪽으로만 답하는 상태이거나, 필터 없는 잡이 새로 \
             `--workspace` 를 돌기 시작한 것이다(그러면 이 판독을 쓰는 사각 탐지가 전부 \
             공허해지므로 그 자리에서 다시 판단해야 한다): {c:?}"
        );
    }
}
