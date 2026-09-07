//! 문서가 **명령 자리에서** 부르는 `just <recipe>` 가 `Justfile` 에 실재하는가.
//!
//! ## 왜 이 축인가
//!
//! `CLAUDE.md` 의 "`cargo build` 는 plugin 바이너리를 다시 만들지 않는다" 경고가 처방으로
//! 내놓는 것이 `PROFILE=debug just build-plugins` 다. 그 경고 자체("낡은 바이너리로 쟀는가")는
//! 판정할 수 없다 — 입력이 `target/` 이라 CI 에서는 모수가 빈다. 그런데 **경고가 시키는
//! 명령이 실재하는가**는 판정된다. recipe 이름이 바뀌면 그 처방은 조용히 아무 데도 안
//! 가리키고, 경고를 읽은 사람은 존재하지 않는 명령을 친다.
//!
//! 두 쪽이 **같은 어휘**를 쓴다 — 문서가 `just build-plugins` 라 적고 `Justfile` 이
//! `build-plugins:` 라 선언한다. 변환이 없다.
//!
//! ## 술어가 못 보는 것 — 그리고 그것을 어떻게 갈랐는가
//!
//! `just` 는 영어 부사이기도 하다. 순진한 술어(`just <낱말>`)는 산문을 센다 — 실측
//! 2026-09-07 로 `just the` · `just before` · `just after` · `just as` · `just like` 등
//! **낱말 10 종**이 잡혔다. 그래서 **명령 자리**만 센다: 코드 펜스 안의 줄이거나 인라인
//! 코드 스팬이고, 그 안에서 (선택적 `$ ` 프롬프트와 `ENV=VAL` 접두 뒤) `just` 로 **시작**하는 것.
//!
//! ★ 펜스 안이라고 다 명령은 아니다. `site/content/en/agents/claude-codex.md` 의
//! `--prompt "Review the diff that was just committed"` 는 펜스 안이지만 **문자열 리터럴
//! 속 영어**다. 줄머리 규칙이 그것을 자동으로 뺀다 — 인용부호를 따로 추적할 필요가 없다.
//! (그 한 건은 순진한 술어가 유일하게 남긴 오탐이었고, 이 규칙으로 0 이 됐다.)
//!
//! ## 안 덮는 것
//!
//! - recipe **인자**는 안 본다. `just build-plugin claude` 에서 `claude` 가 실재하는
//!   plugin 인지는 다른 물음이고, 그 답은 `Justfile` 에 없다.
//! - `just` 없이 부르는 스크립트(`scripts/*.sh`)는 이 축이 아니다.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};

const JUSTFILE: &str = "Justfile";

/// 추적 문서 순회 하한.
const DOC_FLOOR: Floor = Floor {
    min: 40,
    measured: 60,
    measured_on: "2026-09-07",
    why_this_gap: "문서는 회차마다 늘고 줄어서 실측에 딱 붙이면 무관한 추가·삭제가 가드를 \
                   깨운다. 하한이 막는 것은 순회가 죽어 모수가 비는 것 하나다",
};

/// recipe 수 하한. 2026-09-07 실측 16.
const MIN_RECIPES: usize = 10;
/// 명령 자리 인용 하한. 2026-09-07 실측 33.
const MIN_CITATIONS: usize = 15;

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

/// `Justfile` 이 선언한 recipe 이름.
///
/// `PROFILE := env_var_or_default(...)` 같은 **변수 선언**은 뺀다 — `:=` 가 그 표지다.
/// 안 빼면 변수 이름이 recipe 로 세어져, 문서가 변수 이름을 명령처럼 적어도 통과한다.
fn recipes(root: &Path) -> BTreeSet<String> {
    let text = std::fs::read_to_string(root.join(JUSTFILE))
        .unwrap_or_else(|e| panic!("{JUSTFILE} 을 읽지 못했다 — {e}"));
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let head = line.split('#').next().unwrap_or("");
        if head.contains(":=") {
            continue;
        }
        let Some(colon) = head.find(':') else {
            continue;
        };
        let name: String = head[..colon]
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            && name.starts_with(|c: char| c.is_ascii_alphabetic())
        {
            out.insert(name);
        }
    }
    out
}

/// 한 조각이 **명령 자리에서** `just` 를 부르면 그 recipe 이름.
///
/// 앞에 붙어도 되는 것은 셸 프롬프트 `$ ` 와 `ENV=VAL` 들뿐이다. 그 밖의 무엇이 앞에
/// 오면 명령이 아니다 — 그것이 산문과 문자열 리터럴을 한꺼번에 걸러 낸다.
fn just_target(fragment: &str) -> Option<String> {
    let mut rest = fragment.trim_start();
    if let Some(r) = rest.strip_prefix("$ ") {
        rest = r.trim_start();
    }
    loop {
        let Some(word) = rest.split_whitespace().next() else {
            return None;
        };
        if word == "just" {
            let after = rest[word.len()..].trim_start();
            let name: String = after
                .chars()
                .take_while(|c| {
                    c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-' || *c == '_'
                })
                .collect();
            return (!name.is_empty()).then_some(name);
        }
        // `ENV=VAL` 접두만 건너뛴다.
        let is_env = word.contains('=')
            && word.split('=').next().is_some_and(|k| {
                !k.is_empty()
                    && k.chars()
                        .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
            });
        if !is_env {
            return None;
        }
        rest = rest[word.len()..].trim_start();
    }
}

/// (파일, 줄, recipe 이름).
fn citations(root: &Path) -> Vec<(String, usize, String)> {
    let docs = walk_with_floor(
        root,
        root,
        &DOC_FLOOR,
        Descend::SkipBuildCaches,
        &|w: &Walked| w.rel.ends_with(".md") && !w.rel.starts_with("target/"),
    )
    .unwrap_or_else(|e| panic!("문서 순회가 실패했다 — {e}"));

    let mut out = Vec::new();
    for w in docs {
        let Ok(text) = std::fs::read_to_string(&w.path) else {
            continue;
        };
        let mut in_fence = false;
        for (i, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                continue;
            }
            if in_fence && let Some(name) = just_target(line) {
                out.push((w.rel.clone(), i + 1, name));
            }
            // 인라인 코드 스팬 — 펜스 밖의 `just build` 형태.
            let mut rest = line;
            while let Some(open) = rest.find('`') {
                let after = &rest[open + 1..];
                let Some(close) = after.find('`') else { break };
                if let Some(name) = just_target(&after[..close]) {
                    out.push((w.rel.clone(), i + 1, name));
                }
                rest = &after[close + 1..];
            }
        }
    }
    out
}

#[test]
fn every_cited_just_recipe_exists() {
    let root = repo_root();
    let known = recipes(&root);
    let cited = citations(&root);
    assert!(
        known.len() >= MIN_RECIPES,
        "`Justfile` 에서 recipe 를 {} 개만 뽑았다 (2026-09-07 실측 16) — 추출이 깨졌다. \
         모수가 비면 아래 대조는 '전부 실재한다' 로 공짜 통과한다",
        known.len()
    );
    assert!(
        cited.len() >= MIN_CITATIONS,
        "명령 자리의 `just` 인용을 {} 개만 찾았다 (2026-09-07 실측 33) — 추출이 깨졌다",
        cited.len()
    );
    let missing: Vec<String> = cited
        .iter()
        .filter(|(_, _, n)| !known.contains(n))
        .map(|(f, l, n)| format!("  {f}:{l}  just {n}"))
        .collect();
    assert!(
        missing.is_empty(),
        "문서가 `Justfile` 에 없는 recipe 를 명령으로 적고 있다:\n{}\n\
         ★ 문서를 고치든 recipe 를 되살리든 하나는 해야 한다. 지금 그 줄을 읽은 사람은 \
         존재하지 않는 명령을 친다 — 특히 `CLAUDE.md` 의 plugin 재빌드 경고가 처방으로 \
         내놓는 것이 이 형태다.",
        missing.join("\n")
    );
}

/// 술어의 극성 — 무엇이 명령이고 무엇이 아닌가.
///
/// 이 픽스처가 없으면 위 대조는 "추출기가 아무것도 안 잡는다" 여도 통과한다(하한이
/// 그 하나를 막지만, 하한은 수만 보고 **무엇을** 잡는지는 안 본다).
#[test]
fn the_extractor_reads_command_position_only() {
    assert_eq!(
        just_target("just build-plugins").as_deref(),
        Some("build-plugins")
    );
    assert_eq!(just_target("$ just run").as_deref(), Some("run"));
    assert_eq!(
        just_target("PROFILE=debug just build-plugins").as_deref(),
        Some("build-plugins")
    );
    // 산문 — `just` 가 부사다.
    assert_eq!(just_target("이것은 just the 예시다"), None);
    // 펜스 안이지만 문자열 리터럴 속 영어 — 줄머리 규칙이 뺀다.
    assert_eq!(
        just_target(r#"  --prompt "Review the diff that was just committed""#),
        None
    );
    // 인자는 이름에 안 섞인다.
    assert_eq!(
        just_target("just build-plugin claude").as_deref(),
        Some("build-plugin")
    );
}
