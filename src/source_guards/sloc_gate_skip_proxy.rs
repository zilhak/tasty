//! 파일 SLOC 게이트의 `skip()` 이 **대리인**이라는 사실을 못박는다.
//!
//! `scripts/check-file-size.sh` 는 자기 의도를 이렇게 적는다:
//!
//! ```text
//! # skip(게이트 미적용): 테스트 모듈·생성/전사 코드는 아래 skip() 에서 제외.
//! ```
//!
//! 의도는 **"출하되지 않는 코드"** 이고 구현은 **"이름이 그렇게 생긴 파일"** 이다.
//! 도입 커밋 `8b8030c1` 은 의도만 적고 **파일명을 고른 이유를 안 적었다** — 임계 1000 에
//! 유도가 없는 것과 같은 자리다. 파일명은 그 의도의 대리인이고, 대리인은 양쪽으로 틀릴 수
//! 있다.
//!
//! ## 왜 게이트를 고치지 않고 여기서 보는가
//!
//! 실측(2026-09-05, main `980bd306`): 판별을 파일명에서 선언으로 바꾸면 분류가 바뀌는
//! 파일이 25 개인데 **그중 임계를 넘는 것이 0 개**라 게이트 판정은 한 건도 안 바뀐다.
//! 오늘 아무 판정도 안 바꾸면서 게이트에 파서를 하나 더 들이는 값이 없고, 그 파서가
//! 틀리면 **면제하는 방향으로 조용히** 틀린다.
//!
//! 진짜 결함은 개수가 아니라 **심사 없는 면제 채널**이다:
//!
//! ```text
//! allowlist 면제  →  파일에 이름이 적히고 diff 에 보이고 사유를 요구한다
//! 파일명   면제  →  아무 데도 안 적히고 아무도 안 본다
//! ```
//!
//! 그리고 **출하 파일을 `*_tests.rs` 로 개명하면 게이트를 통과한다.** "임계를 넘으면
//! 목록에 적고 심사받는다" 는 계약이 개명 한 번으로 우회된다. 아래 (가)가 그것을 막는다.
//!
//! ## 두 명제
//!
//! - **(가)** 이름으로 `skip` 되는 파일은 전부 선언상 test-only 이거나 통합 타깃/생성물이다.
//!   위반 = **출하 코드가 개명으로 면제받고 있다.**
//! - **(나)** 선언상 test-only 인데 이름 때문에 계수되는 파일이 임계에 닿지 않았다.
//!   위반 = **대리인이 실제로 손해를 내기 시작했다** — 그날이 게이트를 선언 기반으로
//!   바꿀 때다. 덮는 것이 아니라 질문이 언제 다시 열리는지를 기계가 지키게 하는 것이다.
//!
//! ## 대조군
//!
//! 선언 파서와 파일명 규칙은 **서로 다른 계측기**다 — 하나는 소스를 파싱하고 하나는 경로
//! 문자열을 본다. 둘을 대조하는 것이 곧 (가)·(나)라서 스냅샷이 필요 없다.
//!
//! `skip` 패턴은 **게이트 스크립트에서 읽는다.** 외우면 스크립트가 바뀔 때 이 가드가
//! 조용히 다른 것을 재게 된다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::{SCAN_ROOTS, repo_root, rust_sources};

const GATE: &str = "scripts/check-file-size.sh";

/// 게이트의 임계. 여기서만 쓰는 값이 아니라 게이트가 쓰는 값이므로 스크립트에서 읽는다.
fn gate_threshold() -> usize {
    let text = std::fs::read_to_string(repo_root().join(GATE))
        .unwrap_or_else(|e| panic!("{GATE} 를 읽을 수 없다 — {e}"));
    text.lines()
        .find_map(|line| line.trim().strip_prefix("THRESHOLD="))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or_else(|| panic!("{GATE} 에서 THRESHOLD 를 못 읽었다 — 형식이 바뀌었다"))
}

/// 게이트의 `skip()` 이 쓰는 glob 패턴들. **외우지 않고 스크립트에서 읽는다.**
fn gate_skip_patterns() -> Vec<String> {
    let text = std::fs::read_to_string(repo_root().join(GATE))
        .unwrap_or_else(|e| panic!("{GATE} 를 읽을 수 없다 — {e}"));
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("skip()") {
            inside = true;
            continue;
        }
        if inside {
            if t == "}" {
                break;
            }
            if let Some(head) = t.split(')').next() {
                for pat in head.split('|') {
                    let pat = pat.trim();
                    // `*)` 는 case 의 **기본 분기**다. 패턴으로 읽으면 모든 경로에 매칭돼
                    // (가)가 레포 전체를 "이름으로 면제됨" 으로 보고 (나)는 모수가 빈다.
                    if pat == "*" {
                        continue;
                    }
                    if pat.starts_with('*') || pat.contains('/') {
                        out.push(pat.to_string());
                    }
                }
            }
        }
    }
    assert!(
        !out.iter().any(|p| p == "*"),
        "skip() 의 기본 분기 `*)` 를 패턴으로 주웠다 — 그러면 모든 경로가 면제로 읽혀 \
         (가)가 레포 전체를 위반으로 보고한다. 파싱을 고쳐라: {out:?}"
    );
    assert!(
        out.len() >= 4,
        "{GATE} 의 skip() 에서 패턴을 {} 개만 읽었다 — 0 개나 소수는 파싱 실패이고, \
         패턴이 비면 (가)는 판정 대상이 없어 언제나 통과한다",
        out.len()
    );
    out
}

/// `*` 만 지원하는 최소 glob. 게이트가 쓰는 `case` 패턴이 그것뿐이다.
fn glob_matches(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut rest = text;
    if !parts[0].is_empty() {
        if !rest.starts_with(parts[0]) {
            return false;
        }
        rest = &rest[parts[0].len()..];
    }
    let last = parts.len() - 1;
    for (i, part) in parts.iter().enumerate().skip(1) {
        if part.is_empty() {
            continue;
        }
        if i == last && !pattern.ends_with('*') {
            return rest.ends_with(part) && rest.len() >= part.len();
        }
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    true
}

fn name_skipped(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| glob_matches(p, path))
}

/// 선언상 **출하되지 않는** 파일 집합. 부모가 test 게이트면 자식도 안 나간다(전이 폐쇄).
/// 반환 경로는 **레포 상대**다(`rust_sources` 의 형태).
///
/// **판정기는 하나다.** 같은 물음("이 파일은 출하되는가")을 갖는 자리가 이 모듈 말고도
/// 형제 가드(`plugin_locale_specific_literals`)와 루트 통합 타깃
/// (`tests/cli_method_table_parity.rs`)에 있는데, 뒤쪽은 이 모듈의 비공개 항목을 못 본다.
/// 그래서 판정을 `tasty_doc_guards::shipping_scope` 로 올리고 **모수만** 각자 넘긴다.
/// 사본을 두면 답이 갈리고, 갈린 쪽은 면제하는 방향으로 조용히 틀린다.
pub(super) fn test_only_files() -> BTreeSet<PathBuf> {
    tasty_doc_guards::shipping_scope::test_only_files(&repo_root(), &rust_sources())
}

fn implies_test(pred: &str) -> bool {
    tasty_doc_guards::cfg_predicate::implies(pred, "test")
}

fn as_slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// 면제를 떠받치는 **강제 가능한 성질**. `skip()` 의 정당화는 하나가 아니라 셋이고,
/// 셋의 근거가 서로 다르다 — 앞의 둘은 "출하되지 않는다", 셋째는 "출하되지만 사람이
/// 쓰지 않는다" 다. 셋 다 파일명과 독립으로 확인할 수 있다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backing {
    /// `#[cfg(test)]` 선언 아래에 있다 — 출하 빌드에 안 들어간다.
    Declaration,
    /// 크레이트 루트 바로 아래 `tests/` 다 — cargo 가 별도 타깃으로 빌드하므로 lib/bin 에
    /// 안 들어간다. 이건 레이아웃이 강제하는 것이라 선언이 없어도 성립한다.
    CargoTestTarget,
    /// 생성기가 만든 파일이다 — 출하되지만 사람이 유지하지 않아 복잡도 예산 밖이다.
    /// 근거는 파일 자신이 선언한다.
    Generated,
    /// **이름 말고는 아무 근거가 없다.** 면제가 개명 한 번으로 얻어진 상태다.
    NameOnly,
}

/// 생성물이 자기를 밝히는 표식. 실측(2026-09-05, main `e5128a8c`): `*generated*` 에
/// 걸리는 6 파일 전부가 **첫 줄에** 이 문구를 갖는다.
///
/// 표식은 파일이 스스로 다는 것이라 "정말 생성기가 만들었는가" 까지는 답하지 못한다.
/// 답하는 것은 그보다 약한 명제다 — **이름만으로 면제되지는 않는다.** 이름은 옮기면
/// 따라오지만 이 문구는 파일 안에 있어 diff 에 남는다.
const GENERATED_MARKER: &str = "DO NOT EDIT";

/// 크레이트 루트 바로 아래 `tests/` 인가. `src/` 안의 `tests/` 디렉토리는 **여기 해당하지
/// 않는다** — 그건 그냥 모듈이고, 선언이 없으면 출하된다. 경로에 `/tests/` 가 들어 있는지
/// 보는 것으로는 둘이 안 갈린다.
fn is_cargo_test_target(rel: &str) -> bool {
    let full = repo_root().join(rel);
    let mut dir = full.parent();
    while let Some(d) = dir {
        if d.join("Cargo.toml").is_file() {
            return full
                .strip_prefix(d)
                .is_ok_and(|rest| rest.starts_with("tests"));
        }
        dir = d.parent();
    }
    false
}

fn backing(c: &Candidate) -> Backing {
    if c.test_only {
        Backing::Declaration
    } else if is_cargo_test_target(&c.rel) {
        Backing::CargoTestTarget
    } else if c.generated {
        Backing::Generated
    } else {
        Backing::NameOnly
    }
}

/// 판정 한 줄. 트리에서 뽑거나(`population`) 뮤테이션이 합성한다.
///
/// **모수 구성과 판정을 일부러 갈라 놓았다.** 트리를 읽는 쪽(`rust_sources` ·
/// `test_only_files` · `gate_skip_patterns`)은 아래 두 실측 테스트가 그대로 돌려서
/// 덮고, 판정하는 쪽은 합성 모수로 변별력을 잰다. 그래서 뮤테이션이 소스 파일을
/// 건드리지 않는다 — 복원할 것이 없다.
#[derive(Clone, Debug)]
struct Candidate {
    rel: String,
    lines: usize,
    test_only: bool,
    generated: bool,
}

/// 트리 실측 모수.
fn population() -> Vec<Candidate> {
    let test_only = test_only_files();
    rust_sources()
        .into_iter()
        .map(|(path, text)| Candidate {
            test_only: test_only.contains(&path),
            generated: text
                .lines()
                .take(20)
                .any(|line| line.contains(GENERATED_MARKER)),
            rel: as_slash(&path),
            lines: text.lines().count(),
        })
        .collect()
}

/// **(가)의 판정.** 이름으로 면제되는데 실제로는 출하되는 파일들 + 면제 대상 총수.
fn bypassing(pop: &[Candidate], patterns: &[String]) -> (Vec<String>, usize) {
    let skipped: Vec<&Candidate> = pop
        .iter()
        .filter(|c| name_skipped(&c.rel, patterns))
        .collect();
    let bad = skipped
        .iter()
        .filter(|c| backing(c) == Backing::NameOnly)
        .map(|c| c.rel.clone())
        .collect();
    (bad, skipped.len())
}

/// **(나)의 판정.** 선언상 출하 안 되는데 이름이 관례에 안 맞아 게이트가 재는 파일들을
/// 줄 수 내림차순으로.
fn measured_though_not_shipped(pop: &[Candidate], patterns: &[String]) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = pop
        .iter()
        .filter(|c| c.test_only && !name_skipped(&c.rel, patterns))
        .map(|c| (c.lines, c.rel.clone()))
        .collect();
    out.sort_by_key(|(lines, _)| std::cmp::Reverse(*lines));
    out
}

/// (나)를 **원시 줄 수로 재지 않는 이유**의 실측 근거. 좌표: main `e5128a8c`, 2026-09-05.
///
/// 게이트는 tokei 의 code SLOC 을 재고 이 가드는 tokei 를 부를 수 없다. 두 대체 척도를
/// 재 봤고 둘 다 발화 임계로는 못 쓴다 — 추적 `.rs` 1131 파일 전수:
///
/// - **원시 줄 수**: `code <= 줄 수` 가 반례 0 으로 성립해 방향은 안전하지만 너무 헐겁다
///   (여유 중앙 46 · 95% 291 · 최대 1269). 실제로 `src/design_token_guard.rs` 가 원시
///   1052 줄인데 code 는 727 이라, 게이트 임계를 원시 줄 수에 그대로 대면 **위반이 없는데
///   빨개진다.** 거짓 경보가 진짜 경보를 죽인다.
/// - **주석·공백을 마스킹한 줄 수**: 훨씬 조이지만 **상한이 아니다** — 434 파일에서 tokei
///   code 보다 작다(`mask_non_code` 가 문자열 리터럴도 지운다: `src/gfx/renderer/shaders.rs`
///   masked 4 vs code 95). 이걸 쓰면 가드가 **늦게** 울고, 늦는 것은 조용하다.
///
/// 그래서 (나)는 SLOC 을 재지 않고 **게이트가 이미 남긴 흔적**을 본다. 임계를 넘은 파일이
/// 레포를 초록으로 유지하는 길은 allowlist 등재뿐이므로, 재어지는 test-only 파일이 거기
/// 올라온 순간이 곧 비용이 발생한 순간이다. 척도가 필요 없으니 대리인도 없다.
const MEASURED_NOTE: (&str, usize, usize) = ("src/design_token_guard.rs", 727, 1052);

/// 게이트의 심사 목록. 비면 `Err` 로 본다 — 0 건을 "위반 없음" 으로 세지 않는다.
fn allowlist_entries() -> BTreeSet<String> {
    let text = std::fs::read_to_string(repo_root().join(".complexity-file-allowlist"))
        .expect("`.complexity-file-allowlist` 를 못 읽었다 — 경로가 바뀌었는지 확인해라");
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// **(나)의 판정.** 재어지는 test-only 파일 중 게이트 심사 목록에 올라온 것.
fn costing(measured: &[(usize, String)], allow: &BTreeSet<String>) -> Vec<String> {
    measured
        .iter()
        .map(|(_, rel)| rel)
        .filter(|rel| allow.contains(*rel))
        .cloned()
        .collect()
}

/// **(가)** 이름으로 면제되는 파일은 전부 진짜로 출하되지 않는 파일인가.
///
/// 위반은 **출하 코드가 개명으로 게이트를 우회하고 있다**는 뜻이다. allowlist 에 적히면
/// 사람이 보지만 개명은 아무도 안 본다.
#[test]
fn every_name_skipped_file_is_really_not_shipped() {
    let patterns = gate_skip_patterns();
    let (bad, skipped) = bypassing(&population(), &patterns);

    assert!(
        skipped > 0,
        "이름으로 면제되는 파일이 0 개다 — 패턴 파싱이 깨졌으면 이 판정은 대상이 없어 \
         언제나 통과한다(패턴 {} 개)",
        patterns.len()
    );
    assert!(
        bad.is_empty(),
        "출하되는 파일이 **이름 때문에** SLOC 게이트에서 빠지고 있다 — allowlist 에 적혀 \
         심사받는 대신 개명으로 숨은 것이다. 파일을 되돌려 이름 짓거나, 정말 커야 한다면 \
         `.complexity-file-allowlist` 에 사유와 함께 등록해라.\n  {}",
        bad.join("\n  ")
    );
}

/// **(나)** 대리인이 아직 손해를 내지 않았는가.
///
/// 선언상 출하되지 않는데 이름이 관례에 안 맞아 게이트가 **재는** 파일들이 모수다. 그중
/// 하나가 임계를 넘으면 레포를 초록으로 유지하는 길은 `.complexity-file-allowlist` 등재뿐이다.
/// 그러니 **모수와 심사 목록이 겹치는 순간**이 "출하되지 않는 코드가 출하 코드와 같은 잣대로
/// 재어진다" 가 추상이 아니라 비용이 된 순간이고, **그날이 게이트를 선언 기반으로 바꿀 때다.**
///
/// SLOC 을 다시 재지 않는다 — 왜 못 재는지는 `MEASURED_NOTE` 에 실측으로 적었다. 게이트가
/// 이미 남긴 흔적을 보는 쪽이 척도를 하나 더 들이는 것보다 정확하다.
#[test]
fn the_filename_proxy_has_not_started_costing_anything() {
    let patterns = gate_skip_patterns();
    let threshold = gate_threshold();
    let measured = measured_though_not_shipped(&population(), &patterns);
    let allow = allowlist_entries();

    assert!(
        !measured.is_empty(),
        "선언상 test-only 인데 이름으로 면제되지 않는 파일이 0 개다 — 선언 파서가 깨졌으면 \
         이 판정은 대상이 없어 언제나 통과한다"
    );
    assert!(
        !allow.is_empty(),
        "심사 목록을 한 줄도 못 읽었다 — 빈 목록과 겹치면 이 판정은 언제나 통과한다"
    );

    let bad = costing(&measured, &allow);
    let (note, note_code, note_raw) = MEASURED_NOTE;
    assert!(
        bad.is_empty(),
        "대리인이 손해를 내기 시작했다 — 출하되지 않는 파일이 게이트 임계 {threshold} 를 넘어 \
         심사 목록에 올랐다:\n  {}\n\
         두 갈래다 — (1) 이름을 `*_tests.rs` 관례에 맞추면 게이트가 뺀다(대리인을 그대로 두는 \
         쪽), (2) 게이트가 `#[cfg(test)]` 선언을 보게 고친다(대리인을 없애는 쪽). 이 단정이 \
         발화한 날이 (2)를 검토할 때다.\n\
         모수 {} 개 · 심사 목록 {} 개. 참고로 이 모수의 최대치는 원시 줄 수로는 이미 임계에 \
         가깝다({note} 원시 {note_raw} / code {note_code}) — 원시 줄 수를 발화 임계로 쓰지 \
         않는 이유는 MEASURED_NOTE 에 있다.",
        bad.join("\n  "),
        measured.len(),
        allow.len()
    );
}

/// 이 가드 자신이 (나)의 모수 안에 있는가 — 이 파일도 `#[cfg(test)] mod source_guards;`
/// 아래이고 이름이 `*_tests.rs` 가 아니다. 자기가 모수 안에 있어야 판정이 자기에게도
/// 적용된다. 비면 위 두 단정이 무엇을 재는지 알 수 없다.
#[test]
fn this_guard_is_inside_the_population_it_judges() {
    let me: PathBuf = ["src", "source_guards", "sloc_gate_skip_proxy.rs"]
        .iter()
        .collect();
    let test_only = test_only_files();
    assert!(
        test_only.contains(&me),
        "이 파일이 선언상 test-only 로 안 잡힌다 — 선언 파서가 `#[cfg(test)] mod \
         source_guards;` 를 못 따라온 것이다. 스캔 루트: {SCAN_ROOTS:?}"
    );
    assert!(
        !name_skipped(&as_slash(&me), &gate_skip_patterns()),
        "이 파일이 이름으로 면제된다 — 그러면 (나)의 모수 밖이라 자기 판정을 안 받는다"
    );
}

/// 선언 파서가 **한 번 나를 속인 두 형태**를 지금도 보는가.
///
/// 이 가드를 만들며 쓴 첫 판(파이썬)이 두 번 틀렸고, 둘 다 **면제하지 않는 방향**이라
/// 조용했다. 초록이 파서가 옳다는 증거가 아니므로 형태를 이름으로 못박는다.
///
/// - `#[cfg(all(test, feature = "gui"))]` — `cfg(test)` 리터럴만 찾으면 놓친다.
/// - `#[cfg(test)] #[path = "registry_tests.rs"] mod tests;` — `#[path]` 를 안 보면
///   선언처를 아예 못 찾아 파일이 고아가 된다. 값이 문자열이라 마스킹에 지워지므로
///   **같은 줄 번호의 원문**에서 꺼내야 한다.
///
/// 음성 대조를 같이 둔다 — 출하 파일 하나가 test-only 로 잡히면 (가)가 침묵한다.
#[test]
fn the_declaration_parser_still_sees_the_two_shapes_that_once_fooled_it() {
    let test_only = test_only_files();
    let has = |parts: &[&str]| {
        let p: PathBuf = parts.iter().collect();
        assert!(
            repo_root().join(&p).is_file(),
            "표본이 사라졌다: {} — 옮겨졌으면 이 테스트의 좌표를 고쳐라",
            p.display()
        );
        test_only.contains(&p)
    };

    assert!(
        has(&["src", "state", "popup_close_tests.rs"]),
        "`#[cfg(all(test, feature = \"gui\"))]` 형태를 못 읽는다 — 복합 cfg 안의 test 를 \
         함의로 판정해야 한다"
    );
    assert!(
        has(&["src", "completion_strategy", "registry_tests.rs"]),
        "`#[path = \"...\"]` 로 선언된 모듈을 못 따라간다 — 마스킹된 줄에는 경로가 없으니 \
         같은 줄 번호의 원문에서 꺼내야 한다"
    );
    assert!(
        !has(&["src", "main.rs"]),
        "출하 진입점이 test-only 로 잡혔다 — 이 방향의 오류는 (가)를 통째로 침묵시킨다"
    );

    // 0 을 보고하는 자리라 같은 산출물의 비영 대조를 같은 자리에 둔다.
    assert!(
        test_only.len() > 10,
        "선언상 test-only 가 {} 개뿐이다 — 파서가 거의 아무것도 못 따라오고 있다",
        test_only.len()
    );
}

/// `implies_test` 가 **함의**를 판정하는가. `not(test)` 와 `any(..., test)` 는 test 를
/// 함의하지 않는다 — 전자는 반대이고 후자는 다른 조건으로도 컴파일된다. 레포에 둘 다 있다.
#[test]
fn cfg_predicates_that_do_not_imply_test_are_not_treated_as_test_only() {
    assert!(implies_test("test"));
    assert!(implies_test("all(test, feature = \"gui\")"));
    assert!(implies_test("all(unix, test)"));
    assert!(implies_test("all(test, all(unix, debug_assertions))"));

    assert!(!implies_test("not(test)"), "부정을 함의로 읽었다");
    assert!(!implies_test("any(unix, test)"), "선언을 함의로 읽었다");
    assert!(
        !implies_test("all(unix, windows)"),
        "test 가 없는데 참이라 한다"
    );
    assert!(!implies_test("feature = \"gui\""));
}

/// 뮤테이션 대상을 **실측으로** 고른다 — 이름으로 박으면 변이가 약해도 초록이고, 그
/// 초록이 서술을 증명한 것처럼 보인다. 가장 큰 것을 고르는 이유는 "이게 숨으면 가장
/// 크게 손해" 라서다.
fn largest(pop: &[Candidate], pick: impl Fn(&Candidate) -> bool) -> Candidate {
    let mut hit: Vec<&Candidate> = pop.iter().filter(|c| pick(c)).collect();
    hit.sort_by(|a, b| b.lines.cmp(&a.lines).then(a.rel.cmp(&b.rel)));
    hit.first()
        .map(|c| (*c).clone())
        .expect("변이 대상이 모수에 없다 — 대상 없이 통과한 것을 판정력으로 세지 않는다")
}

/// **(가)의 변별력** — 출하 파일을 `*_tests.rs` 로 개명하면 잡히는가. 그리고 그때
/// **게이트 자신은 조용한가**(그게 이 가드가 존재하는 이유다).
#[test]
fn a_shipping_file_renamed_to_a_test_name_is_caught() {
    let patterns = gate_skip_patterns();
    let pop = population();

    let (before, skipped_before) = bypassing(&pop, &patterns);
    assert!(
        before.is_empty(),
        "변이 전에 이미 위반이 있다 — 아래 판정이 변이 때문인지 알 수 없다: {before:?}"
    );

    let victim = largest(&pop, |c| {
        !c.test_only && !name_skipped(&c.rel, &patterns) && !c.generated
    });
    // 변이 강도를 단정한다 — 20 줄짜리를 숨기는 것과 수백 줄을 숨기는 것은 다른 사건이고,
    // 약한 대상으로 얻은 초록은 "개명 우회를 본다" 를 증명하지 않는다.
    assert!(
        victim.lines >= 200,
        "가장 큰 출하 파일이 {} 줄({})뿐이다 — 이 변이로는 판정력을 주장할 수 없다",
        victim.lines,
        victim.rel
    );
    let renamed = victim.rel.replace(".rs", "_tests.rs");

    // R79 ② — 변이 후에도 **약한 쪽은 통과한다**. 게이트는 이 개명을 면제로 읽는다.
    assert!(
        name_skipped(&renamed, &patterns),
        "개명이 게이트의 면제 패턴에 걸리지 않는다 — 이 변이는 게이트를 우회하지 못하므로 \
         아래 판정이 무엇을 증명하는지 알 수 없다: {renamed}"
    );
    assert!(
        !is_cargo_test_target(&renamed) && !victim.generated,
        "다른 근거로도 면제되는 파일을 골랐다 — 개명이 원인이라고 말할 수 없다: {renamed}"
    );

    let mut mutated = pop.clone();
    mutated
        .iter_mut()
        .find(|c| c.rel == victim.rel)
        .expect("고른 대상이 모수에 없다")
        .rel = renamed.clone();

    let (after, skipped_after) = bypassing(&mutated, &patterns);
    assert!(
        after.contains(&renamed),
        "출하 파일 `{}` ({} 줄) 을 `{renamed}` 로 개명했는데 (가)가 조용하다 — 이 가드는 \
         개명 우회를 못 본다. 위반 목록: {after:?}",
        victim.rel,
        victim.lines
    );
    assert_eq!(
        skipped_after,
        skipped_before + 1,
        "면제 대상 수가 1 만큼 늘어야 한다 — 그 외의 변화가 있으면 변이가 모수를 흔든 것이다"
    );
}

/// **(가)의 변별력, 개수를 그대로 두는 변이.** 파일 수도, 면제 대상 수도, 이름도 그대로
/// 두고 **출하 여부만** 뒤집는다 — `*_tests.rs` 인 채로 출하 코드에서 참조되기 시작하는
/// 실제 형태다. 개수만 세는 판정은 여기서 조용하다.
#[test]
fn a_name_skipped_file_that_starts_shipping_is_caught_without_changing_any_count() {
    let patterns = gate_skip_patterns();
    let pop = population();
    let (before, skipped_before) = bypassing(&pop, &patterns);
    assert!(before.is_empty(), "변이 전에 이미 위반이 있다: {before:?}");

    let victim = largest(&pop, |c| {
        c.test_only && name_skipped(&c.rel, &patterns) && !is_cargo_test_target(&c.rel)
    });

    assert!(
        victim.lines >= 100,
        "이름으로 면제되는 파일 중 가장 큰 것이 {} 줄({})뿐이다 — 변이가 약하다",
        victim.lines,
        victim.rel
    );

    let mut mutated = pop.clone();
    mutated
        .iter_mut()
        .find(|c| c.rel == victim.rel)
        .expect("고른 대상이 모수에 없다")
        .test_only = false;

    let (after, skipped_after) = bypassing(&mutated, &patterns);
    assert_eq!(
        (mutated.len(), skipped_after),
        (pop.len(), skipped_before),
        "이 변이는 어떤 개수도 바꾸지 않아야 한다 — 바뀌었으면 변별력을 개수가 대신 낸 것이다"
    );
    assert_eq!(
        after,
        vec![victim.rel.clone()],
        "`{}` 이 이름은 그대로 둔 채 출하되기 시작했는데 (가)가 조용하다 — 개수가 안 변하는 \
         우회를 못 본다",
        victim.rel
    );
}

/// **(나)의 변별력** — 재어지는 test-only 파일이 심사 목록에 오르면 우는가. 그리고 목록의
/// **크기를 그대로 둔 채** 한 줄만 바꿔도 우는가(개수를 세는 판정은 여기서 조용하다).
#[test]
fn a_measured_file_entering_the_review_list_is_caught_even_when_the_list_size_is_unchanged() {
    let patterns = gate_skip_patterns();
    let measured = measured_though_not_shipped(&population(), &patterns);
    let allow = allowlist_entries();

    assert!(
        costing(&measured, &allow).is_empty(),
        "변이 전에 이미 위반이 있다 — 아래 판정이 변이 때문인지 알 수 없다"
    );

    // 대상을 이름으로 박지 않고 실측으로 고른다 — 가장 큰 것이 목록에 오를 가능성이 가장 크다.
    let (victim_lines, victim) = measured
        .first()
        .cloned()
        .expect("모수가 비었다 — 대상 없이 통과한 것을 판정력으로 세지 않는다");
    assert!(
        victim_lines >= 200,
        "모수의 최대치가 {victim_lines} 줄({victim})뿐이다 — 변이가 약하다"
    );

    // ① 목록에 더한다. 크기가 1 늘어난다.
    let mut grown = allow.clone();
    grown.insert(victim.clone());
    assert_eq!(
        costing(&measured, &grown),
        vec![victim.clone()],
        "재어지는 test-only 파일 `{victim}` 이 심사 목록에 올랐는데 (나)가 조용하다"
    );

    // ② 크기를 그대로 두고 한 줄만 바꾼다 — 개수만 보는 판정은 이걸 못 본다.
    let evicted = allow
        .iter()
        .next()
        .cloned()
        .expect("심사 목록이 비었다 — 크기 보존 변이를 만들 수 없다");
    let mut swapped = allow.clone();
    swapped.remove(&evicted);
    swapped.insert(victim.clone());
    assert_eq!(
        swapped.len(),
        allow.len(),
        "크기 보존 변이가 크기를 바꿨다 — 아래 판정이 크기 변화에 반응한 것일 수 있다"
    );
    assert_eq!(
        costing(&measured, &swapped),
        vec![victim.clone()],
        "심사 목록의 크기를 그대로 둔 채 `{evicted}` 를 `{victim}` 으로 바꿨는데 (나)가 \
         조용하다 — 개수가 안 변하는 형태를 못 본다"
    );

    // ③ 모수 밖 파일이 목록에 오르는 것은 위반이 아니다 — 과민하지 않은가.
    let outsider = allow
        .iter()
        .find(|a| !measured.iter().any(|(_, rel)| rel == *a))
        .cloned()
        .expect("심사 목록 전부가 모수 안이다 — 음성 대조를 만들 수 없다");
    let mut only_outsider = BTreeSet::new();
    only_outsider.insert(outsider.clone());
    assert!(
        costing(&measured, &only_outsider).is_empty(),
        "모수 밖 파일 `{outsider}` 이 목록에 있는 것을 위반으로 읽었다 — 출하 코드가 임계를 \
         넘어 심사받는 것은 게이트의 정상 동작이다"
    );
}

/// **면제 채널을 종류별로 연다.** 이름으로 게이트에서 빠지는 파일이 각각 **무엇에** 기대어
/// 빠지는지 세고, 근거 없이 이름만으로 빠지는 것이 없음을 단정한다.
///
/// 개수를 문턱으로 박지 않는다 — 테스트 파일 하나가 커지면 그 수가 움직인다. 움직이는 것이
/// 결함이 아닌 수를 문턱으로 쓰면 정상 성장에 빨개지고, **거짓 경보가 진짜 경보를 죽인다.**
/// 잡아야 하는 것은 **크기가 아니라 종류의 변화**이므로 집합으로 판정한다.
#[test]
fn every_exemption_rests_on_something_other_than_the_name() {
    let patterns = gate_skip_patterns();
    let pop = population();

    let mut by_kind: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in pop.iter().filter(|c| name_skipped(&c.rel, &patterns)) {
        by_kind
            .entry(format!("{:?}", backing(c)))
            .or_default()
            .push(c.rel.clone());
    }

    // 비영 대조 — 세 근거가 전부 실재해야 이 판정이 무엇을 재는지 알 수 있다.
    for kind in ["Declaration", "CargoTestTarget", "Generated"] {
        assert!(
            by_kind.get(kind).is_some_and(|v| !v.is_empty()),
            "면제 근거 `{kind}` 에 해당하는 파일이 0 개다 — 그 가지를 판정하는 코드가 아무것도 \
             안 보고 있다. 관측된 종류: {:?}",
            by_kind.keys().collect::<Vec<_>>()
        );
    }

    let nameless = by_kind.get("NameOnly").cloned().unwrap_or_default();
    assert!(
        nameless.is_empty(),
        "이름 말고는 면제 근거가 없는 파일이 있다 — 게이트에서 빠지는 데 필요한 것이 개명 \
         하나뿐인 상태다. `#[cfg(test)]` 아래로 넣거나, 크레이트의 `tests/` 로 옮기거나, \
         생성물이면 `{GENERATED_MARKER}` 표식을 달아라. 셋 다 아니면 심사 대상이다:\n  {}",
        nameless.join("\n  ")
    );
}

/// **닫은 구멍 둘** — 이름의 두 가지가 근거 없이도 면제를 주던 자리다. 둘 다 변이가 파일
/// **개수를 바꾸지 않는다**(경로만 갈아끼운다).
#[test]
fn a_name_that_merely_looks_exempt_no_longer_buys_an_exemption() {
    let patterns = gate_skip_patterns();
    let pop = population();
    let (before, _) = bypassing(&pop, &patterns);
    assert!(before.is_empty(), "변이 전에 이미 위반이 있다: {before:?}");

    let victim = largest(&pop, |c| {
        !c.test_only && !name_skipped(&c.rel, &patterns) && !c.generated
    });
    assert!(
        victim.lines >= 200,
        "고른 출하 파일이 {} 줄({})뿐이다 — 변이가 약하다",
        victim.lines,
        victim.rel
    );

    // ① `src/` 안에 `tests/` 디렉토리를 만들어 넣는다. cargo 타깃이 아니라 **그냥 모듈**이라
    //    선언이 없으면 출하되는데, 경로에 `/tests/` 가 들어갔다는 이유로 게이트는 뺀다.
    let in_tests_dir = "src/tests/smuggled.rs".to_string();
    assert!(
        name_skipped(&in_tests_dir, &patterns),
        "게이트가 이 경로를 면제하지 않는다 — 변이가 우회를 만들지 못했다: {in_tests_dir}"
    );
    assert!(
        !is_cargo_test_target(&in_tests_dir),
        "`{in_tests_dir}` 를 cargo 통합 타깃으로 읽었다 — 크레이트 루트 바로 아래 `tests/` 만 \
         타깃이고 `src/` 안의 같은 이름 디렉토리는 모듈이다"
    );

    // ② 이름에 generated 를 넣는다. 표식이 없으면 생성물이라는 근거가 없다.
    let named_generated = "src/looks_generated.rs".to_string();
    assert!(
        name_skipped(&named_generated, &patterns),
        "게이트가 이 경로를 면제하지 않는다: {named_generated}"
    );

    for smuggled in [in_tests_dir, named_generated] {
        let mut mutated = pop.clone();
        let slot = mutated
            .iter_mut()
            .find(|c| c.rel == victim.rel)
            .expect("고른 대상이 모수에 없다");
        slot.rel = smuggled.clone();
        assert_eq!(
            mutated.len(),
            pop.len(),
            "경로만 바꾸는 변이가 모수 크기를 바꿨다"
        );

        let (after, _) = bypassing(&mutated, &patterns);
        assert!(
            after.contains(&smuggled),
            "출하 파일을 `{smuggled}` 로 옮겼는데 (가)가 조용하다 — 이름의 이 가지가 아직 \
             근거 없이 면제를 준다. 위반 목록: {after:?}"
        );
    }
}
