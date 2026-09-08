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
use tasty_doc_guards::temp_scratch::Scratch;

const JUSTFILE: &str = "Justfile";

/// 추적 문서 순회 하한.
///
/// **`measured` 가 세는 것은 `keep` 이 남긴 파일이다** — [`walk_with_floor`] 의 하한은
/// 방문 수가 아니라 반환 벡터의 길이를 본다. 같은 모듈의 `walk_dirs_with_floor` 는 반대로
/// **방문한 것**을 세므로(그 파일의 `the_directory_walk_floors_what_it_visited_not_what_it_kept`
/// 가 그 차이를 못박는다) 두 값을 같은 저울에 올리면 안 된다. 여기서 남는 것은 레포 전체의
/// `.md`(`target/` 제외, 점 디렉토리 안 내려감)이고, 아래 `citations` 의 doc 주석이 적은
/// 440 과 **같은 모수**다.
///
/// ★ 첫 판의 `measured: 60` 은 낡은 값이 아니라 **안 잰 값**이었다. 이 파일이 태어난 날
/// (2026-09-07) 이 트리의 추적 `.md` 는 이미 **428** 이었고, 그 커밋이 값으로 남긴 것은
/// recipe 16 · 명령 자리 인용 33 뿐이다 — 세 하한 중 이 하나만 실측 없이 들어갔다.
/// 하루 만에 60 → 440 으로 벌어진 것이 아니라, 60 이 이 순회의 수였던 적이 없다.
const DOC_FLOOR: Floor = Floor {
    min: 300,
    measured: 440,
    measured_on: "2026-09-08",
    counted_on: tasty_doc_guards::floored_walk::CountedOn::LaneTip("671aa69b2"),
    why_this_gap: "여유 140 은 증감 폭이 아니라 **못 잡는 것의 크기**로 정했다. 실측 \
                   2026-08-25~09-08 에 이 모수는 271 → 440 으로 늘기만 했고 하락은 0 건이라, \
                   증감으로 여유를 잡으면 근거가 없는 수가 된다. 300 이 잡는 것은 순회가 \
                   죽는 것과 `docs/`(394) 또는 `docs/adr/`(203) 가 통째로 빠지는 것이고, \
                   `docs/features/`(64) · `site/`(36) 규모의 누락은 여전히 못 본다",
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
///
/// **점 디렉토리는 안 내려간다.** 이 순회는 레포 루트에서 시작해 작업 트리에 있는 `.md`
/// 를 전부 보는데, 커밋되지 않는 로컬 작업 폴더는 clone·CI 에 없고 **개발자의 작업
/// 트리에만** 있다. 그러면 같은 커밋이 기계마다 다른 좌변을 낸다 — worktree 에서는 그
/// 폴더가 레포 밖을 가리키는 링크라 안 세어지고 원본 저장소에서는 실물이라 세어진다.
/// 실측 2026-09-08: 레포 루트에 점 디렉토리를 만들고 그 안 `.md` 에 없는 recipe 를 적으니
/// 이 가드가 rc=101 로 그 좌표를 찍었다. 형제 가드(`cited_anchors_resolve`)가 통합
/// 자리에서 같은 형태로 빨개진 뒤 같은 처방을 받았다.
///
/// **판정 능력은 안 준다** — 추적되는 `.md` 중 점 디렉토리 아래 있는 것이 0 개다
/// (`git ls-files '*.md' | grep -c '^\.\|/\.'`). 이 트리에서 잰 좌변은 점 배제 전후로
/// 440 · 440 이다(worktree 라 그 폴더가 링크여서 원래도 안 세어졌다 — 값이 갈리는 것은
/// 원본 저장소 쪽이고 그것은 여기서 못 잰다).
fn citations(root: &Path) -> Vec<(String, usize, String)> {
    citations_under(root, &DOC_FLOOR)
}

/// 위 판독의 알맹이 — **순회 뿌리와 하한을 인자로 받는다.**
///
/// 하한을 함수 안에 박아 두면 이 판독을 합성 트리로 잴 길이 없다. 레포의 `.md` 수와
/// 합성 트리의 `.md` 수는 애초에 다른 모수인데, 상수 하나가 둘을 다 판정하려 들면
/// 둘 중 하나는 반드시 틀린다(R1072).
fn citations_under(root: &Path, floor: &Floor) -> Vec<(String, usize, String)> {
    let docs = walk_with_floor(
        root,
        root,
        floor,
        Descend::SkipBuildCachesAndDotDirs,
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
    // 점 배제로 로컬 작업 폴더는 닫혔지만 **점 없는 이름**의 미추적 폴더는 그대로 들어온다
    // (실측 2026-09-08: 그 형태로 이 가드가 그 좌표를 찍는다). 실패할 때만 좌표의 출신을
    // 물어 처방이 레포 밖 문서에 붙는 것을 막는다 — 근거는 [`tasty_doc_guards::tracked_scope`].
    let outside = if missing.is_empty() {
        String::new()
    } else {
        let rels: Vec<String> = cited
            .iter()
            .filter(|(_, _, n)| !known.contains(n))
            .map(|(f, _, _)| f.clone())
            .collect();
        tasty_doc_guards::tracked_scope::outside_repo_note(&root, &rels)
    };
    assert!(
        missing.is_empty(),
        "문서가 `Justfile` 에 없는 recipe 를 명령으로 적고 있다:\n{}\n\
         ★ 문서를 고치든 recipe 를 되살리든 하나는 해야 한다. 지금 그 줄을 읽은 사람은 \
         존재하지 않는 명령을 친다 — 특히 `CLAUDE.md` 의 plugin 재빌드 경고가 처방으로 \
         내놓는 것이 이 형태다.{}",
        missing.join("\n"),
        outside
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

/// **양성 대조 — 대조가 없던 두 판독.**
///
/// `just_target`(한 조각의 술어)은 이미 [`the_extractor_reads_command_position_only`]
/// 가 건다. 안 걸려 있던 것은 디스크를 읽는 둘, [`recipes`] 와 [`citations_under`] 다.
///
/// 이 가드는 좌변과 우변이 **둘 다 판독**이라 방향이 둘이다:
///   - `recipes` 가 **더** 걷으면(변수 줄까지 recipe 로) 없는 recipe 인용이 통과한다.
///   - `recipes` 가 **덜** 걷으면 멀쩡한 인용이 거짓 위반이 되고, 그 처방
///     ("Justfile 에 recipe 를 추가하거나 인용을 고쳐라")은 실재하지 않는 위반에 대한
///     것이다.
///   - `citations` 가 **덜** 걷으면 깨진 인용이 조용히 통과한다.
/// 하한 셋(`DOC_FLOOR`·`MIN_RECIPES`·`MIN_CITATIONS`)은 이 중 뒤쪽 하나만 본다.
///
/// ★ recipe 이름·문서 내용은 전부 합성이다(R1078) — 진짜 `Justfile` 의 값을 안 쓴다.
#[test]
fn the_recipe_and_citation_readers_answer_on_a_substituted_tree() {
    let probe = Scratch::new("just-citation");
    let dir = probe.path();
    std::fs::create_dir_all(dir.join("sub")).expect("합성 트리를 만들지 못했다");

    // ── 판독 1: Justfile 의 recipe 이름 ─────────────────────────────────
    std::fs::write(
        dir.join(JUSTFILE),
        "zeta-build:\n    cargo build\n\
         omega_probe: zeta-build\n    echo hi\n\
         SIGMA := \"변수라 recipe 가 아니다\"\n\
         tau-run: # 꼬리 주석이 붙은 recipe\n    echo run\n\
         # kappa-commented: 주석 줄 전체\n\
         phi-decoy #주석 안의 콜론: 이 줄은 recipe 선언이 아니다\n\
         4nope:\n    echo 숫자로 시작하면 이름이 아니다\n",
    )
    .expect("합성 Justfile 실패");

    let got = recipes(dir);
    let mut names: Vec<&str> = got.iter().map(|s| s.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["omega_probe", "tau-run", "zeta-build"],
        "recipe 판독이 합성 Justfile 에서 다른 답을 냈다"
    );
    assert!(
        !got.contains("SIGMA"),
        "`:=` 변수 줄을 recipe 로 셌다 — 없는 recipe 인용이 통과하게 된다"
    );
    assert!(
        !got.iter().any(|n| n.starts_with("kappa")),
        "주석 줄에서 recipe 를 만들어 냈다"
    );
    // `#` 를 안 벗기면 **주석 안의 콜론**이 recipe 콜론으로 읽혀 `phi-decoy` 가
    // recipe 로 들어온다 — 없는 recipe 를 인용해도 통과하게 되는 방향이다.
    assert!(
        !got.contains("phi-decoy"),
        "주석 안의 콜론을 recipe 콜론으로 읽었다 — `#` 를 안 벗긴 것이다"
    );
    assert!(
        !got.contains("4nope"),
        "숫자로 시작하는 이름을 recipe 로 셌다"
    );

    // ── 판독 2: 문서의 인용 ─────────────────────────────────────────────
    std::fs::write(
        dir.join("top.md"),
        "산문에서 just 는 부사다.\n\
         인라인은 `just zeta-build` 로 적는다.\n\
         ```sh\n\
         just omega_probe\n\
         ```\n",
    )
    .expect("합성 문서 실패");
    std::fs::write(dir.join("sub/deep.md"), "```sh\n$ just tau-run\n```\n")
        .expect("합성 하위 문서 실패");
    // `.md` 가 아닌 것 — 모수 밖이다.
    std::fs::write(dir.join("notes.txt"), "```sh\njust sigma-decoy\n```\n")
        .expect("합성 잡파일 실패");

    let fixture_floor = Floor {
        min: 2,
        measured: 2,
        measured_on: "2026-09-08",
        counted_on: tasty_doc_guards::floored_walk::CountedOn::SyntheticTree,
        why_this_gap: "합성 트리라 문서 수를 이 시험이 직접 정한다 — 간격이 0 인 것이 맞다.",
    };
    let cites = citations_under(dir, &fixture_floor);
    let mut seen: Vec<(&str, usize, &str)> = cites
        .iter()
        .map(|(f, l, n)| (f.as_str(), *l, n.as_str()))
        .collect();
    seen.sort_unstable();

    assert!(
        seen.iter()
            .any(|(f, l, n)| *f == "top.md" && *l == 2 && *n == "zeta-build"),
        "펜스 밖 인라인 코드 스팬의 인용을 못 읽었다(또는 줄 번호가 1 기반이 아니다): {seen:?}"
    );
    assert!(
        seen.iter()
            .any(|(f, l, n)| *f == "top.md" && *l == 4 && *n == "omega_probe"),
        "펜스 안의 인용을 못 읽었다: {seen:?}"
    );
    assert!(
        seen.iter()
            .any(|(f, _, n)| *f == "sub/deep.md" && *n == "tau-run"),
        "하위 디렉토리 문서로 안 내려갔다: {seen:?}"
    );
    assert!(
        !seen.iter().any(|(_, _, n)| *n == "sigma-decoy"),
        "`.md` 가 아닌 파일을 읽었다 — 그 인용은 이 가드의 모수가 아니다: {seen:?}"
    );
    assert!(
        !seen.iter().any(|(_, l, _)| *l == 1),
        "산문 줄(`just` 가 부사)에서 인용을 만들어 냈다 — 거짓 위반의 원천이다: {seen:?}"
    );

    // 좌변과 우변이 맞물리는지 — 이 합성 트리에서는 깨진 인용이 0 이어야 한다.
    let missing: Vec<&(&str, usize, &str)> =
        seen.iter().filter(|(_, _, n)| !got.contains(*n)).collect();
    assert!(
        missing.is_empty(),
        "합성 트리의 인용이 합성 Justfile 과 안 맞물린다 — 두 판독 중 하나가 틀렸다: \
         {missing:?}"
    );
}
