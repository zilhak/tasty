//! 소스 텍스트를 읽어 **한 플랫폼·한 빌드 조합에서만 드러나는 함정**을 전 플랫폼에서
//! 막는 드리프트 가드.
//!
//! ## 왜 `tests/` 가 아니라 여기인가
//!
//! `tests/` 의 통합 테스트는 **헤드리스 조합에서만** 자동으로 실행된다
//! (`check-headless` 가 전체 스위트를 돌린다). 기본 조합에는 실행 채널이 없고, 그
//! 헤드리스 잡조차 `paths-ignore` 때문에 문서·site 만 담은 push 에서는 발사되지 않는다.
//! 소스를 런타임에 읽는 스캔 가드는 컴파일만 되어서는 아무것도 보장하지 못하므로 —
//! 가드의 본체가 곧 스캔이다 — 두 조합 모두에서 도는 곳에 둔다. 이 모듈은 `tasty` bin 의
//! 유닛 테스트라 **두 조합의 자동 잡이 모두 실행한다** — Windows 잡은
//! `cargo test --workspace --lib --bins` 로, 헤드리스 잡은 그 상위집합인 전체 스위트로.
//! (한때 두 잡이 같은 `--lib --bins` 명령을 썼으나 헤드리스가 전체 스위트로 넓어졌다.
//! 바뀐 것은 명령이지 이 모듈의 채널이 아니다 — `tests/` 로 옮기면 Windows 잡을 잃는다.)
//!
//! 채널의 **존재**는 채널의 **건강**이 아니다. 어떤 검증을 "이 가드가 CI 에서 잡아준다"
//! 를 근거로 면제하려면, 그 잡이 최근 실행에서 실제로 통과했는지를 따로 확인해야 한다.
//! 트리거·러너를 포함한 전체 매트릭스는 [`docs/dev-guide/ci-gates.md`] 가 정본이다.
//!
//! ## 스캔 방식
//!
//! 파일을 읽어 **주석·문자열·문자 리터럴을 공백으로 덮은 사본**(`mask_non_code`)을
//! 만든 뒤 그 위에서만 판정한다. 줄 구조는 그대로 두므로 줄 번호가 보존된다.
//! 들여쓰기나 rustfmt 스타일에 의존하지 않는다. CRLF 는 읽는 즉시 LF 로 정규화하고,
//! 경로는 `Path::join` 으로만 만들어 Windows 에서도 같은 결과를 낸다.
//!
//! **이 모듈의 파일들도 스캔 대상에 포함된다.** 금지 형태를 상수·합성 스니펫으로 들고
//! 있지만 전부 문자열 리터럴이라 마스킹으로 지워지므로 자기 자신을 잡지 않는다. 예외를
//! 두어 스스로를 빼면 그 예외만큼 이 모듈이 사각이 되므로 그렇게 하지 않았다 —
//! `the_guard_file_scans_itself` 가 조각 하나하나의 포함 여부를 못박는다.
//!
//! ## 판정기는 스캔 루프에서 분리한다
//!
//! 각 가드의 판정은 순수 함수(`scan`)로 뽑아 두고, 레포 전수 테스트와 합성 입력
//! 테스트가 **같은 함수**를 부른다. 루프 안에 인라인이면 면제를 찌르는 변이가
//! "레포에 진짜 위반을 심었다 되돌리기" 로만 가능해지는데, 그건 느리고 트리를
//! 더럽히며 되돌리다 사고가 난다.
//!
//! ## 면제에는 그것을 겨냥한 변이를 붙인다
//!
//! 면제(allowlist · 창 · skip 조건)를 하나 넣을 때마다 **그 면제 창 안쪽에 진짜
//! 위반을 심었을 때 잡히는가**를 묻는 테스트를 함께 넣는다. 각 가드의
//! `exemption_mutations` 모듈이 그것이다. 검증되지 않은 면제는 그 면제만큼 구멍이다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// 스캔 하한 — 워커가 망가져 파일을 거의 못 읽으면 모든 가드가 조용히 통과한다.
/// 현재 실측은 1100 개 남짓이라 여유를 두고 잡는다.
const MIN_SCANNED_FILES: usize = 900;

/// 통합 테스트 하한 — 실측 58 개.
const MIN_INTEGRATION_TEST_FILES: usize = 40;

/// 스캔 루트 — **출하되는 코드**. 본체 + 모든 크레이트.
///
/// "Rust 소스 전부" 가 아니다. 실측(2026-09-05) git 이 아는 `.rs` 1265 개 중 **63 개가
/// 이 루트 밖**이다 — `tests/` 58 · `site/src/` 4 · `build.rs` 1. 대부분의 가드는
/// "출하되는 코드가 규칙을 지키나" 를 물으므로 그 63 이 대상이 아닌 것이 맞다.
///
/// **다만 테스트 자신을 대상으로 삼는 가드는 물음이 다르다** — 아래
/// [`SCAN_ROOTS_WITH_INTEGRATION_TESTS`] 를 쓴다. 물음이 둘이면 모수도 둘이다.
const SCAN_ROOTS: &[&str] = &["src", "crates"];

/// 통합 테스트까지 포함한 스캔 루트 — **테스트를 대상으로 삼는 가드 전용**.
///
/// `cargo test` 가 병렬로 돌리는 것은 한 바이너리 안의 테스트이고, `tests/` 의 통합
/// 테스트도 그 대상이다. 그래서 "이 전역을 만지는 테스트가 전부 락을 잡는다" 같은
/// **전수 명제**를 세우는 가드가 `SCAN_ROOTS` 만 보면 전수가 아니다 — 이 레포에서
/// 가장 큰 테스트 뭉치가 통째로 안 보인다.
///
/// `SCAN_ROOTS` 와 합치지 않는다. 합치면 출하 코드를 묻는 가드들이 테스트 코드까지
/// 대상으로 삼아, 대부분의 가드에서 대량 오탐이 된다.
const SCAN_ROOTS_WITH_INTEGRATION_TESTS: &[&str] = &["src", "crates", "tests"];

/// 주석을 걷어낸다.
///
/// 소스에서 문자열 리터럴을 뽑는 가드가 공통으로 필요로 한다. 판정이 "리터럴이 있는가"
/// 인데 주석이 그 이름을 **설명하려고** 인용하는 일이 잦고, 그러면 설명이 대상으로
/// 오인된다 — 문서를 잘 쓸수록 가드가 나빠진다.
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        // 줄 끝 주석: 앞의 따옴표 수가 짝수여야 문자열 밖의 `//` 다.
        let cut = line
            .match_indices("//")
            .find(|(at, _)| line[..*at].matches('"').count() % 2 == 0);
        out.push_str(cut.map_or(line, |(at, _)| &line[..at]));
        out.push('\n');
    }
    out
}

/// IPC dispatch 가 메서드 이름을 읽는 표현식. 이 뒤에 오는 모양으로 **판정 자리**를 찾는다.
///
/// 두 라우터(`handler.rs` 의 두 `match` · gui/헤드리스 step 셋)가 전부 이 표현식으로
/// 갈래를 친다. 표현식이 바뀌면 이 상수도 같이 고쳐야 한다 — 안 고치면 아래 판정이
/// 아무 자리도 안 보면서 초록이 된다(부르는 쪽이 그것을 단정한다).
const METHOD_EXPR: &str = "request.method";

/// 스캐너가 **볼 수 없는 이름**으로 판정하는 자리.
///
/// 위 판정은 dispatch 본문을 텍스트로 읽어 `"a.b"` 꼴 리터럴을 뽑는 것이다. 이름이
/// 리터럴이 아니면 — 매크로가 만들거나 상수·변수와 맞대면 — 그 이름은 목록에 안 들어온다.
/// 그러면 답하지도 사유가 적혀 있지도 않은 메서드가 **조용히** 생긴다.
///
/// 실측이 있다(2026-09-05, debug step 기준). 리터럴 하나를 `macro_rules!` 뒤로 숨기면
/// 뽑히는 항목이 12 → 11 로 줄고, 매크로가 만든 이름으로 갈래를 하나 새로 더하면 항목
/// 수가 **아예 안 변한다.** 두 경우 다 아래 하한(5)에 안 걸려 초록으로 통과했다. 즉 이
/// 부류는 크기가 0 이었던 것이 아니라 하한이 못 보던 것이다 — 수를 세는 검사로는 이
/// 사각을 못 좁힌다.
///
/// 그래서 이름의 **수** 대신 이름을 읽는 **자리**를 본다. 셋만 판정 자리로 친다:
/// `== <x>` · `.starts_with(<x>)` · `match ….as_str() { <팔> => }`. 그 자리의 값은
/// 문자열 리터럴이어야 한다. 그 밖의 모양(값을 위임 함수 인자로 **넘기는** 것)은 여기서
/// 이름을 가르지 않으므로 대상이 아니다.
fn opaque_method_sites(body: &str) -> Vec<String> {
    // 주석을 먼저 걷어낸다. 안 걷으면 주석 안의 괄호가 깊이를 흔들어 팔의 시작 자리를
    // 어긋나게 하고(실측), 주석에 적힌 메서드 이름이 판정 자리로 잡힌다.
    let body = strip_comments(body);
    let body = body.as_str();
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(i) = body[at..].find(METHOD_EXPR) {
        at += i + METHOD_EXPR.len();
        let rest = body[at..].trim_start();
        if let Some(r) = rest.strip_prefix("==") {
            if !r.trim_start().starts_with('"') {
                out.push(format!("`== {}`", head(r)));
            }
        } else if let Some(r) = rest.strip_prefix(".starts_with(") {
            if !r.trim_start().starts_with('"') {
                out.push(format!("`.starts_with({}`", head(r)));
            }
        } else if let Some(r) = rest.strip_prefix(".as_str()") {
            let r = r.trim_start();
            if r.starts_with('{') {
                out.extend(non_literal_arms(r));
            }
        }
    }
    out
}

/// 오류 메시지에 붙일 짧은 발췌 — 자리를 사람이 찾을 수 있을 만큼만.
fn head(s: &str) -> String {
    s.trim_start()
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(40)
        .collect()
}

/// 팔 패턴에서 주석과 속성을 걷어낸다 — `#[cfg(debug_assertions)]` 이 붙은 팔과
/// 팔 앞의 설명 주석이 실제 코드에 있다(실측).
fn pattern_text(raw: &str) -> String {
    raw.lines()
        .map(|l| l.split("//").next().unwrap_or("").trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `match ….as_str() { … }` 의 팔 중 **패턴이 리터럴이 아닌** 것.
///
/// 팔 패턴은 문자열 리터럴이거나, `_`/소문자 식별자(전부받기 바인딩)여야 한다.
/// 대문자 상수나 매크로 호출이 패턴 자리에 오면 그 이름은 텍스트로 안 보인다.
fn non_literal_arms(block: &str) -> Vec<String> {
    let b = block.as_bytes();
    let mut out = Vec::new();
    let (mut i, mut depth, mut pat_start, mut in_str) = (0usize, 0usize, 0usize, false);
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'{' | b'(' | b'[' => {
                depth += 1;
                if depth == 1 {
                    pat_start = i + 1;
                }
            }
            b'}' | b')' | b']' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                // 블록 팔(`"x" => { … }`) 뒤의 쉼표는 생략할 수 있다. 그 닫는 중괄호도
                // 팔의 끝으로 쳐야 다음 팔의 패턴이 앞 팔 전체를 끌고 온다.
                //
                // **중괄호만이다.** 괄호까지 팔의 끝으로 치면 패턴 자신이 괄호를 가진
                // 경우(`mac!() => …`)에 시작 자리가 패턴 **뒤로** 밀려 패턴이 빈 문자열이
                // 되고, 빈 것은 검사가 건너뛴다 — 정확히 잡아야 할 모양이 통과했다(실측).
                if depth == 1 && c == b'}' {
                    pat_start = i + 1;
                }
            }
            b',' if depth == 1 => pat_start = i + 1,
            b'=' if depth == 1 && b.get(i + 1) == Some(&b'>') => {
                for alt in pattern_text(&block[pat_start..i]).split('|') {
                    let alt = alt.trim();
                    if alt.is_empty() {
                        continue;
                    }
                    let binding = alt == "_"
                        || (!alt.is_empty()
                            && alt
                                .chars()
                                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
                    if !alt.starts_with('"') && !binding {
                        out.push(format!("`match` 팔 `{alt}`"));
                    }
                }
                i += 2;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// 시그니처로 함수를 찾아 그 **본문**을 중괄호 균형으로 잘라낸다.
///
/// 들여쓰기에 의존하지 않는다 — rustfmt 스타일이 바뀌어도 같은 것을 자른다.
/// 문자열 안의 중괄호는 세지 않는다(`"{}"` 포맷 리터럴이 흔하다).
fn fn_body(src: &str, signature: &str) -> Option<String> {
    let at = src.find(signature)?;
    let open = src[at..].find('{')? + at;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in src[open..].char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(src[open..open + i + 1].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 스캔 루트 아래의 모든 `.rs` 를 (레포 상대 경로, LF 정규화된 내용)으로 모은다.
/// 빌드 산출물(`target/`)은 루트 밑에 없지만, 크레이트별 `target/` 이 생길 수 있어
/// 이름으로 한 번 더 뺀다.
fn rust_sources() -> Vec<(PathBuf, String)> {
    let out = tasty_doc_guards::source_text::rust_sources(&repo_root(), SCAN_ROOTS);
    assert!(
        out.len() >= MIN_SCANNED_FILES,
        "스캔 하한 미달: {} 개만 읽었다(하한 {MIN_SCANNED_FILES}). 워커나 스캔 루트가 깨졌다",
        out.len()
    );
    out
}

/// [`SCAN_ROOTS_WITH_INTEGRATION_TESTS`] 판. 통합 테스트가 실제로 읽혔는지를 따로
/// 못박는다 — 루트 이름이 틀리면 예외가 아니라 조용한 0 이 되고, 0 만큼 늘어난
/// 모수는 늘지 않은 것과 구별되지 않는다.
fn rust_sources_with_integration_tests() -> Vec<(PathBuf, String)> {
    let out = tasty_doc_guards::source_text::rust_sources(
        &repo_root(),
        SCAN_ROOTS_WITH_INTEGRATION_TESTS,
    );
    let integration = out
        .iter()
        .filter(|(p, _)| p.to_string_lossy().replace('\\', "/").starts_with("tests/"))
        .count();
    assert!(
        integration >= MIN_INTEGRATION_TEST_FILES,
        "통합 테스트를 {integration} 개밖에 못 읽었다(하한 {MIN_INTEGRATION_TEST_FILES}). \
         넓힌 모수가 안 넓어졌으면 이 가드는 넓히기 전과 똑같이 통과한다"
    );
    out
}

/// 스캔 **단위** — `src` 하나와 `crates/<이름>` 각각. 개수 하한(`MIN_SCANNED_FILES`)은
/// 단위 하나가 통째로 빠져도 통과한다: 실측하면 가장 큰 크레이트가 108 개라
/// 1114 − 108 = 1006 으로 하한 900 을 안 건드린다. 그래서 개수와 **별개로** 집합을 못박는다.
/// 강도는 하한 < 개수 고정 < 집합 동등 순이고, 세지는 이유는 정밀도가 아니라 재는 대상이
/// "몇 개 봤나" 에서 "무엇을 봤나" 로 바뀌기 때문이다.
///
/// 이 함수는 순수하다 — 변이 테스트가 조작한 목록을 그대로 먹일 수 있다.
fn scanned_units(files: &[(PathBuf, String)]) -> BTreeSet<String> {
    files.iter().filter_map(|(rel, _)| unit_of(rel)).collect()
}

/// 레포 상대 경로가 속한 스캔 단위. 스캔 루트 밖이면 `None`.
fn unit_of(rel: &Path) -> Option<String> {
    let mut parts = rel.components();
    let first = parts.next()?.as_os_str().to_string_lossy().into_owned();
    match first.as_str() {
        "src" => Some("src".to_owned()),
        "crates" => {
            let name = parts.next()?.as_os_str().to_string_lossy().into_owned();
            Some(format!("crates/{name}"))
        }
        _ => None,
    }
}

/// 스캔과 **독립적인 경로로** 단위 집합을 만든다 — 파일을 훑지 않고 매니페스트의 존재로
/// 센다. 같은 워커로 두 번 세면 워커의 결함이 양쪽에 똑같이 들어가 대조가 무의미해진다.
fn expected_units() -> BTreeSet<String> {
    let root = repo_root();
    assert!(root.join("src").is_dir(), "`src` 스캔 루트가 없다");
    let mut out = BTreeSet::from(["src".to_owned()]);
    let entries = std::fs::read_dir(root.join("crates")).expect("`crates` 를 읽을 수 없다");
    for entry in entries {
        let entry = entry.expect("디렉터리 항목을 읽을 수 없다");
        let is_dir = entry.file_type().expect("파일 종류를 알 수 없다").is_dir();
        if is_dir && entry.path().join("Cargo.toml").is_file() {
            out.insert(format!("crates/{}", entry.file_name().to_string_lossy()));
        }
    }
    out
}

/// `(스캔에서 빠진 단위, 스캔에만 있는 단위)`. 양방향을 다 내는 이유는 개수가 같은 수로
/// 상쇄될 수 있기 때문이다 — 하나가 빠지고 하나가 들어오면 총수는 안 변한다.
fn unit_diff(
    scanned: &BTreeSet<String>,
    expected: &BTreeSet<String>,
) -> (Vec<String>, Vec<String>) {
    (
        expected.difference(scanned).cloned().collect(),
        scanned.difference(expected).cloned().collect(),
    )
}

/// 단위별 파일 수. 변이가 겨냥할 최대 크레이트를 고르는 데 쓴다.
fn unit_counts(files: &[(PathBuf, String)]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for (rel, _) in files {
        if let Some(unit) = unit_of(rel) {
            *out.entry(unit).or_insert(0usize) += 1;
        }
    }
    out
}

/// `cfg` 로 배제되는 두 축을 각각 대표하는 파일. 스캔은 `cfg` 를 해석하지 않고 디스크의
/// 텍스트를 읽으므로 **어느 조합에서 돌든 둘 다 읽어야 한다.**
///
/// - `src/gfx/gpu.rs` — **feature 축**(`#[cfg(feature = "gui")] mod gfx;`). 헤드리스
///   조합에서 `gfx::` 유닛 테스트가 31 개에서 0 개로 사라지는 것이 그 게이트의 실행 축
///   증거다. 그 디렉터리의 `.rs` 중 `target_os` 를 쓰는 것은 0 이라 이 파일은 feature
///   축만 단독으로 대표한다.
/// - `src/host_api/webview/macos.rs` — **target_os 축**. Linux 에서는 두 조합 모두
///   컴파일되지 않으므로 이 파일 하나로는 feature 축을 가를 수 없다. 두 파일이 필요한
///   이유가 그것이다.
///
/// 텍스트로 `cfg` 를 찾아 판정하지 않는다 — 두 파일 다 게이트가 **자기 파일 밖**에 있어
/// 파일만 열어서는 "게이트 없음" 으로 읽힌다.
///
/// ## 자동 채널 없음
///
/// 위 두 문장 중 **게이트가 어디에 있는지**와 **헤드리스에서 `gfx::` 가 31→0 이라는
/// 실행 축 증거**는 이 크레이트 안에서 판정할 수 없다. 유닛 테스트는 **자기 조합
/// 하나만** 보므로 다른 조합의 목록을 볼 수단이 없고, 게이트가 다른 파일로 옮겨가도
/// 아래 단정들은 그대로 통과한다. 즉 이 두 문장은 **사람이 읽어야 잡히는 부류**이고
/// 겨냥할 변이가 없다. 지어내지 않고 부재를 여기 적어둔다.
///
/// **이 선언의 좌표**: 모수 = 이 크레이트의 유닛 테스트, base = 이 커밋의 부모.
/// 부재는 base 에 따라 만료된다 — 조합 간 목록을 대조하는 가드가 워크스페이스에
/// 들어오면 이 두 문장은 그때부터 **가드 대상**이 되고 이 선언은 죽는다.
/// 선언을 옮겨 적을 때 이 좌표를 함께 옮겨라.
///
/// 반면 "`src/gfx/` 에 target_os 가 없다" 는 전제는 실행으로 판정 가능해서
/// `the_feature_axis_sample_is_not_also_target_gated` 가 재고 있다.
/// feature 축 표본이 사는 디렉터리. 위 표본 경로와 아래 순수성 판정이 같은 값을 보게
/// 한 곳에 둔다.
const FEATURE_AXIS_DIR: &str = "src/gfx/";

const CFG_EXCLUDED_SAMPLES: &[&str] = &["src/gfx/gpu.rs", "src/host_api/webview/macos.rs"];

/// `src/gfx/gpu.rs` 가 **feature 축만** 대표한다는 전제를 못박는다. 그 디렉터리에
/// `target_os` 분기가 하나라도 들어오면 이 표본은 두 축이 섞여 위 관측의 판별력을 잃는다.
/// 전제가 실행으로 판정 가능하므로 산문으로 두지 않고 여기서 재는 쪽을 택했다.
#[test]
fn the_feature_axis_sample_is_not_also_target_gated() {
    let sources = rust_sources();
    let considered: Vec<&(PathBuf, String)> = sources
        .iter()
        .filter(|(path, _)| {
            path.to_string_lossy()
                .replace('\\', "/")
                .starts_with(FEATURE_AXIS_DIR)
        })
        .collect();
    // 판정기가 자기 모수를 함께 낸다. 접두가 어긋나 0 개를 훑으면 아래 단정은 아무것도
    // 안 보면서 통과한다 — "0 건 발견" 과 "0 회 실행" 을 여기서 가른다. 접두를 없는
    // 디렉터리로 바꾸는 변이가 이 줄이 없을 때 실제로 살아남는 것을 확인했다.
    assert!(
        !considered.is_empty(),
        "{FEATURE_AXIS_DIR} 아래에서 훑은 파일이 0 개다 — 디렉터리가 옮겨졌거나 접두가 \
         어긋났다. 이 상태로는 아래 판정이 아무것도 보지 않는다"
    );
    let dirty: Vec<String> = considered
        .iter()
        .filter(|(_, text)| mask_non_code(text).contains("target_os"))
        .map(|(path, _)| path.display().to_string())
        .collect();
    assert!(
        dirty.is_empty(),
        "`src/gfx/` 에 target_os 분기가 생겼다 — feature 축 표본이 두 축을 섞게 된다. \
         `CFG_EXCLUDED_SAMPLES` 의 feature 축 표본을 순수한 파일로 옮겨라: {dirty:?}"
    );
}

/// 스캔이 **지금 이 빌드가 컴파일하지 않는 파일까지** 읽는다는 것을 못박는다. 헤드리스
/// 조합에서 이 테스트가 통과하는 것이 feature 게이트 축의 직접 관측이고, 어느 조합에서든
/// `macos.rs` 쪽이 target_os 축의 관측이다. 하한(`MIN_SCANNED_FILES`)은 총량만 보므로
/// 특정 파일이 빠져도 통과한다 — 그래서 이름으로 따로 단정한다.
#[test]
fn the_scan_reads_files_this_build_never_compiles() {
    let scanned: BTreeSet<String> = rust_sources()
        .iter()
        .map(|(path, _)| path.to_string_lossy().replace('\\', "/"))
        .collect();
    let missing: Vec<&str> = CFG_EXCLUDED_SAMPLES
        .iter()
        .copied()
        .filter(|path| !scanned.contains(*path))
        .collect();
    assert!(
        missing.is_empty(),
        "cfg 로 배제된 대표 파일이 스캔에 없다: {missing:?}. 파일이 옮겨졌으면 이 목록을 \
         함께 고쳐라 — 목록이 낡으면 이 단정은 아무것도 안 보면서 통과한다"
    );
}

#[test]
fn every_scan_unit_contributes_at_least_one_file() {
    let files = rust_sources();
    let (missing, extra) = unit_diff(&scanned_units(&files), &expected_units());
    assert!(
        missing.is_empty() && extra.is_empty(),
        "스캔 단위 집합이 어긋난다 — 빠진 단위 {missing:?} / 여분 {extra:?}. \
         개수 하한은 단위 하나가 통째로 빠져도 통과하므로 이 대조가 따로 필요하다"
    );
}

/// 집합 동등이라는 **면제 없는 판정**도 자기를 겨냥한 변이로 못박는다 — 판정기가 돌기만
/// 하고 아무것도 못 보는 상태를 배제한다.
mod scan_unit_mutations;

/// 스캔 **단위**가 아니라 **파일** 집합을 고정한다. 위 단위 동등은 크레이트가 통째로
/// 빠지는 것을 잡지만, 한 단위 안에서 파일이 사라지는 것은 못 잡는다.
mod scan_population;

/// 파일 SLOC 게이트의 `skip()` 이 **대리인**이라는 것을 못박는다 — 게이트의 의도는
/// "출하되지 않는 코드" 이고 구현은 파일명이다. 둘이 갈리는 자리를 기계가 본다.
mod sloc_gate_skip_proxy;

/// 주석·문자열·문자 리터럴을 덮은 사본. 구현은 `tasty-doc-guards` 에 있다 —
/// 같은 마스킹이 루트 `tests/` 의 통합 타깃에도 필요한데, 그쪽은 이 모듈의 비공개
/// 항목을 못 본다. 사본을 두면 같은 물음에 판정기가 둘이 된다.
pub(crate) use tasty_doc_guards::source_text::mask_non_code;

/// 마스킹된 소스에서 `pos`(char 인덱스 아님, 바이트 인덱스) 가 몇 번째 줄인지.
fn line_of(masked: &str, pos: usize) -> usize {
    masked[..pos].bytes().filter(|b| *b == b'\n').count() + 1
}

/// 식별자 경계에서 시작하는 단어인지.
fn is_word_boundary(masked: &str, pos: usize, word: &str) -> bool {
    let before = masked[..pos].chars().next_back();
    let after = masked[pos + word.len()..].chars().next();
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    !before.is_some_and(ident) && !after.is_some_and(ident)
}

/// `masked` 안에서 `word` 가 단어로 나타나는 모든 바이트 위치.
fn word_positions(masked: &str, word: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = masked[from..].find(word) {
        let pos = from + rel;
        if is_word_boundary(masked, pos, word) {
            out.push(pos);
        }
        from = pos + word.len();
    }
    out
}

/// `open` 위치의 여는 구분자에 대응하는 닫는 구분자의 바이트 위치. 매크로 호출은
/// `(`·`{`·`[` 중 아무것이나 쓸 수 있으므로 구분자를 고정하지 않는다.
fn matching_delim(masked: &str, open: usize) -> Option<usize> {
    let opener = masked[open..].chars().next()?;
    let closer = match opener {
        '(' => ')',
        '{' => '}',
        '[' => ']',
        _ => return None,
    };
    let mut depth = 0usize;
    for (offset, c) in masked[open..].char_indices() {
        if c == opener {
            depth += 1;
        } else if c == closer {
            depth -= 1;
            if depth == 0 {
                return Some(open + offset);
            }
        }
    }
    None
}

/// `from` 이후 첫 여는 구분자(`(`·`{`·`[`)의 바이트 위치.
fn next_opening_delim(masked: &str, from: usize) -> Option<usize> {
    masked[from..]
        .char_indices()
        .find(|(_, c)| matches!(c, '(' | '{' | '['))
        .map(|(offset, _)| from + offset)
}

/// 이 모듈 자신이 스캔 모수에 들어 있는지 못박는다 — 자기 제외 면제를 두지 않았다는
/// 근거다. 빠지면 이 모듈 안의 진짜 위반을 어떤 가드도 못 잡게 된다.
///
/// 모듈이 여러 파일로 나뉘어 있으므로 **이름을 외우지 않고 디렉토리를 읽는다.** 한 파일만
/// 확인하면 나머지가 조용히 모수 밖으로 나가도 통과한다 — 파일이 하나였을 때는 그 구분이
/// 없었지만 지금은 있다. 새 조각을 더해도 이 테스트가 따라온다.
#[test]
fn the_guard_file_scans_itself() {
    let dir: PathBuf = ["src", "source_guards"].iter().collect();
    let mut own: Vec<PathBuf> = std::fs::read_dir(repo_root().join(&dir))
        .expect("가드 모듈 디렉토리를 읽지 못했다 — 경로가 바뀌었는지 확인해라")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name())
        .filter(|name| Path::new(name).extension().is_some_and(|ext| ext == "rs"))
        .map(|name| dir.join(name))
        .collect();
    own.sort();
    assert!(
        !own.is_empty(),
        "가드 모듈에서 .rs 를 하나도 찾지 못했다 — 0 개를 통과로 세지 않는다"
    );

    let scanned: BTreeSet<PathBuf> = rust_sources().into_iter().map(|(path, _)| path).collect();
    let missing: Vec<&PathBuf> = own.iter().filter(|path| !scanned.contains(*path)).collect();
    assert!(
        missing.is_empty(),
        "가드 자신의 파일이 스캔 모수에서 빠졌다 — 자기 제외 면제가 다시 생겼는지 확인해라: {missing:?}"
    );
}

mod define_class_return;

mod read_only_handle_mtime;

// ── 워크플로: 테스트를 **실행하는** 스텝은 `--no-fail-fast` 를 갖는다 ────────
//
// `cargo test` 는 기본적으로 **처음 실패한 테스트 바이너리에서 멈춘다.** 그러면 그 뒤에
// 오는 타깃이 한 번도 실행되지 않는데, 로그는 "N failed" 라고만 말한다. 실측으로 기본
// 조합 `--lib --bins` 가 이 플래그 없이는 바이너리 1 개(2017 passed)에서 멈췄고, 붙이면
// 52 개(4551 passed)가 돌았다 — 51 개 크레이트가 조용히 가려져 있었다.
//
// 이 결함은 **문서 주장이 아니라 워크플로 내부의 비대칭**이라, 문서와 워크플로를 대조하는
// `crates/tasty-doc-guards/tests/ci_channel_claims_match_workflows.rs` 가 보지 못한다. 한 잡에 플래그를 넣으면서
// 같은 파일의 다른 잡을 놓치는 형태가 실제로 있었고, 그것을 막는 것이 이 가드다.

/// 워크플로 디렉토리(레포 루트 기준).
const WORKFLOW_DIR: &str = ".github/workflows";

/// 대조군(`git ls-files`)이 낸 워크플로 파일 수의 하한 — **연기 검사**다.
///
/// 스캔 모수 자체는 아래 [`workflow_fail_fast_tests`] 가 git 목록과 **집합 동등**으로
/// 고정한다(ADR-0133 ③). 하한은 그 대조군이 비는 경우에만 남겨 둔다 — 대조군이 비면
/// 집합 동등은 양쪽이 빈 집합이라 언제나 초록이기 때문이다.
/// 값의 근거: 2026-09-05 실측 `.github/workflows` 의 `.yml` **9 개**.
const MIN_GIT_LISTED_WORKFLOWS: usize = 5;

/// 워크플로 파일별 `cargo test` 호출 수. **총계 하한이 아니라 파일별 고정값이다.**
///
/// 총계 하한이었을 때 값이 4 였고 실측이 5 였다 — 호출 하나가 사라져도 초록인 폭이다.
/// 그보다 나쁜 것은 총계가 **이동을 못 본다**는 것이다: 한 워크플로에서 호출이 빠지고
/// 다른 워크플로에 생기면 총계는 그대로다. 이 가드가 막으려는 사고가 바로 그 형태
/// (한 잡에 플래그를 넣으면서 같은 파일의 다른 잡을 놓친 것)라, 자리를 구별하지 못하는
/// 판정은 이 축에서 특히 약하다.
///
/// 호출 자체에는 식별자가 없으므로 여기서 멈춘다 — 파일 이름까지가 이 축이 가진
/// 식별자이고, 한 파일 안의 호출들은 서로 구별되지 않는다. 그래서 파일 단위 건수 고정이다.
///
/// 목록에 없는 워크플로는 **0 이 기대값**이다. 새 워크플로가 테스트를 돌리기 시작하면
/// 그 자리에서 빨개진다.
///
/// 값의 근거: 2026-09-05 실측. 이 수는 가드 자신의 계측기
/// (`cargo_test_invocations(flatten_workflow(..))`)로 센 것이지 `grep 'cargo test'` 가
/// 아니다 — grep 은 같은 파일들에서 9 줄을 내는데, 그중 넷은 `name: cargo test (unit)`
/// 같은 스텝 이름과 주석이다. 둘을 섞으면 다음 사람이 이 상수를 grep 으로 검산하고
/// 어긋난다고 판단한다.
///
/// `doc-guards.yml` 은 문서 가드를 전용 잡으로 뺀 워크플로다(ADR-0138). 그 잡이 생기면서
/// 호출이 하나 늘었고, 이 표가 **파일별**이라 그 사실이 "어느 파일에 생겼는가" 로 드러났다
/// — 총계였으면 다른 파일에서 하나 줄어든 것과 구별되지 않았다.
const EXPECTED_TEST_INVOCATIONS: &[(&str, usize)] = &[
    // 5 = macOS 유닛 · Windows 유닛 · headless 전체 스위트 · Linux gui 유닛 · Linux gui
    // e2e 1 건. 가운데 둘이 조합 격자의 Linux×gui×debug 칸을 덮는다. 마지막 것은 아직
    // 게이트가 아니라 관측용(`continue-on-error`)이고, 승격 조건은 그 스텝 주석에 있다
    // (docs/dev-guide/ci-gates.md).
    //
    // macOS 것이 다섯 번째다. 그 전까지 그 잡은 `cargo check` 하나뿐이라 macOS 로 게이트된
    // 유닛 테스트는 **컴파일만 되고 아무도 안 돌렸다**. 비용이 이 잡의 시간이 아니라
    // 워크플로 벽시계(= 잡 최댓값)라는 판단과 그것이 뒤집히는 조건은 ci-gates.md 에 있다.
    ("crossplatform-check.yml", 5),
    ("doc-guards.yml", 1),
    ("test.yml", 3),
];

/// YAML 주석 줄을 지우고 전체를 한 줄로 평탄화한다.
///
/// 평탄화하는 이유: `run: |` 블록과 `run: >` 접힌 스칼라, 그리고 줄 끝 `\` 이음이 전부
/// 한 명령을 여러 줄에 나눈다. 줄 단위로 보면 `cargo test --workspace \` 에서 끊겨
/// 뒤에 오는 플래그를 놓친다 — 있는 플래그를 없다고 판정하는 쪽이라 더 나쁘다.
///
/// 주석을 먼저 지우는 이유: 이 파일의 주석에도 `--no-fail-fast` 라는 글자가 나온다.
/// 안 지우면 **주석이 스텝을 면제해 준다.**
///
/// `name:` 줄도 지운다. 이 레포의 스텝 이름이 `cargo test (unit)` 처럼 명령을 그대로
/// 쓰기 때문이다 — 안 지우면 이름이 호출로 잡혀 오탐이 되고, 더 나쁘게는 이름의 조각이
/// 뒤따르는 진짜 명령까지 삼켜 그 명령을 **검사 대상에서 빼 버린다**(이름 슬라이스가
/// 다음 `cargo ` 앞까지라 플래그를 대신 물어 준다).
fn flatten_workflow(yaml: &str) -> String {
    yaml.replace("\r\n", "\n")
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with('#') && !t.starts_with("- name:") && !t.starts_with("name:")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// 평탄화된 워크플로에서 `cargo test` 호출을 하나씩 잘라낸다. 각 조각은 그 호출부터
/// 다음 `cargo ` 직전까지라, 한 스텝에 명령이 여럿이어도 플래그가 섞이지 않는다.
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

/// `--no-fail-fast` 가 없는 **실행** 호출들. `--no-run` 은 컴파일만 하므로 면제다 —
/// 실행하지 않는 호출에는 fail-fast 라는 개념이 없다.
fn test_invocations_missing_no_fail_fast(yaml: &str) -> Vec<String> {
    let flat = flatten_workflow(yaml);
    cargo_test_invocations(&flat)
        .into_iter()
        .filter(|inv| !inv.contains("--no-run") && !inv.contains("--no-fail-fast"))
        .map(|inv| inv.split_whitespace().take(8).collect::<Vec<_>>().join(" "))
        .collect()
}

mod debug_gate_dagger;
mod debug_handler_isolation;
/// `debug-ipc.md` 의 † 표시가 런타임 게이트라는 **코드의 성질**과 어긋나지 않는지 본다.
/// 표식은 사람이 손으로 붙이고, 성질은 소스에 있다 — 갈라져도 아무 데서도 안 터진다.
/// IPC 라우터가 스캔이 볼 수 있는 이름(문자열 리터럴)으로 갈리는지 본다. 라우터를
/// 텍스트로 읽는 가드들이 전부 이 전제 위에 서 있고, 수를 세는 하한은 이 부류를 못 본다.
mod dispatch_name_literals;
/// 프로세스 전역을 만지는 테스트가 그 전역의 직렬화 락을 잡는지 본다. 락은 잡는 쪽끼리만
/// 막으므로 하나만 밖에 있어도 직렬화가 통째로 무효가 된다.
mod test_serialization_locks;

/// 트리거와 무관하게 필요한 일(설치·부팅 정화)이 조합마다 부팅 경로에 걸려 있는지
/// 본다. 요청 경로가 유일한 채널이면 그 요청 형태가 좁아지는 순간 그 일이 조용히
/// 사라진다 — 실제로 났고, 컴파일도 두 조합의 유닛 스위트도 못 잡았다.
mod jobs_anchored_at_boot;

/// 유도표(`ipc_namespaces`)의 원본(`PluginManager.packages`)을 유도가 사는 크레이트 밖에서
/// 바꾸는 자리가 있는지 본다. 있으면 표가 낡아, 지운 plugin 의 prefix 가 남고 호스트가 같은
/// 이름에 가진 구현이 가려진다.
mod derived_plugin_tables_are_not_bypassed;

/// 번들 plugin 명부가 적힌 다섯 자리(cfg 두 갈래 · 매니페스트 실물 · 문서 두 곳)가
/// 같은 집합인지 본다. 자리가 여럿인 것이 아니라 잇는 것이 없는 것이 결함이다.
mod builtin_plugin_roster;
mod bundled_plugin_namespace_coverage;

/// 갤러리 specimen 이 **되풀이한 본체 치수**가 아직 같은지 본다. 위 가드가 "specimen 이
/// 있는가" 라면 이쪽은 "그 specimen 이 적어 놓은 수가 본체 값과 같은가" 다.
#[cfg(test)]
mod gallery_copied_dimensions;

/// 본체 등록처(popup `all_defs` · 무대 `all_metas`)에 있는 것이 갤러리 카탈로그에도
/// 있는지 본다. gallery-first 는 불가침 원칙인데 그것을 어겼을 때 빨개지는 것이 0 이었다.
#[cfg(test)]
mod gallery_specimen_parity;

mod headless_app_layer_coverage;

#[cfg(test)]
mod length_constant_frontier;

/// "이 코드가 출하물인가" 를 묻는 소스 스캔 가드들의 공용 술어. 사본을 두면 갈린 쪽이
/// 조용히 낡는다.
#[cfg(test)]
mod test_gate;

/// 값이 `size-*` 스케일 안인데 숫자로 쓴 길이 자리를 센다. `length_constant_frontier` 가
/// 선언의 **타입**을 묻는다면 이쪽은 값의 **출처**를 묻는다 — 토큰이 움직여도 안 따라가는
/// 자리다. (자리로 가리키면 사이에 모듈이 하나 끼는 순간 다른 것을 가리킨다 — 실제로 그랬다.)
#[cfg(test)]
mod on_scale_length_literal;

mod platform_gated_dispatch_complement;

/// `plugin_only` 표식과 plugin host-call 진입부의 인터셉트가 같은 집합인지 본다.
/// 갈라지면 외부 호출자가 "있는 메서드" 에 "없다" 를 받는다.
mod plugin_only_dispatch_parity;

/// 번들 plugin 프로덕션 코드에 로케일 고정(CJK) 문구가 박혀 있는지 본다. 박힌 문구는
/// 어떤 로케일 설정에서도 그 언어로만 나간다.
mod plugin_locale_specific_literals;
/// 포트 발견 모드 명부가 적힌 세 자리(코드 상수 · ko/en 가이드)가 같은 값을
/// 열거하는지 본다. ko/en 쌍이지만 첫 열이 균질해 집합 동등이 정의되는 자리다.
mod port_mode_roster;

/// 기하를 내주는 debug 관측면 둘(popup · banner)이 같은 키 모양으로 내는지, 그리고
/// 배너 쪽이 좌표계를 응답에 명시하는지 본다. 갈리면 두 표면을 재는 검증 스크립트가
/// 두 벌이 되고 그 둘은 따로 늙는다.
mod geometry_surface_shape;
/// 핸들러가 IPC params 를 숫자로 읽는 자리가 관문(`handler/params.rs`) 하나를
/// 지나는지 본다. 흩어져 있으면 자르기(`as u32`)가 한 자리에서만 고쳐진다.
mod params_chokepoint;

mod reserved_ipc_prefixes;
mod routing_key_coverage;

/// 위 가드의 명제를 **(메서드, 키) 쌍**으로 올린다. 키 단위 명제는 `"id"` 처럼 한
/// 메서드에서만 인식되는 키를 모든 메서드에서 인식된 것으로 세어 거짓 초록이 된다.
mod routing_key_method_scope;

#[cfg(test)]
mod workflow_fail_fast_tests;
