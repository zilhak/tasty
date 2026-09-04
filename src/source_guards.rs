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
//! **이 파일도 스캔 대상에 포함된다.** 금지 형태를 상수·합성 스니펫으로 들고 있지만
//! 전부 문자열 리터럴이라 마스킹으로 지워지므로 자기 자신을 잡지 않는다. 예외를 두어
//! 스스로를 빼면 그 예외만큼 이 파일이 사각이 되므로 그렇게 하지 않았다 —
//! `the_guard_file_scans_itself` 가 포함 여부를 못박는다.
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

/// 스캔 루트. 워크스페이스의 Rust 소스 전부(본체 + 모든 크레이트).
const SCAN_ROOTS: &[&str] = &["src", "crates"];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 스캔 루트 아래의 모든 `.rs` 를 (레포 상대 경로, LF 정규화된 내용)으로 모은다.
/// 빌드 산출물(`target/`)은 루트 밑에 없지만, 크레이트별 `target/` 이 생길 수 있어
/// 이름으로 한 번 더 뺀다.
fn rust_sources() -> Vec<(PathBuf, String)> {
    let root = repo_root();
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = SCAN_ROOTS.iter().map(|r| root.join(r)).collect();
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("스캔 루트를 읽을 수 없다: {} — {e}", dir.display()));
        for entry in entries {
            let entry = entry.expect("디렉터리 항목을 읽을 수 없다");
            let path = entry.path();
            let file_type = entry.file_type().expect("파일 종류를 알 수 없다");
            if file_type.is_dir() {
                if entry.file_name() == "target" {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let rel = path
                    .strip_prefix(&root)
                    .expect("스캔 경로는 레포 안이어야 한다")
                    .to_path_buf();
                let text = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("소스를 읽을 수 없다: {} — {e}", path.display()));
                out.push((rel, text.replace("\r\n", "\n")));
            }
        }
    }
    assert!(
        out.len() >= MIN_SCANNED_FILES,
        "스캔 하한 미달: {} 개만 읽었다(하한 {MIN_SCANNED_FILES}). 워커나 스캔 루트가 깨졌다",
        out.len()
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
mod scan_unit_mutations {
    use super::*;

    /// 크레이트 하나를 스캔 결과에서 통째로 지운다. **개수 하한은 여전히 통과**하는
    /// 크기라, 이 변이가 죽는 것은 오직 집합 대조 때문이다.
    #[test]
    fn a_crate_dropped_from_the_scan_is_reported_missing() {
        let files = rust_sources();
        let counts = unit_counts(&files);
        let victim = counts
            .iter()
            .filter(|(unit, _)| unit.starts_with("crates/"))
            .max_by_key(|(_, n)| **n)
            .map(|(unit, _)| unit.clone())
            .expect("크레이트 단위가 하나도 없다");
        let mutated: Vec<(PathBuf, String)> = files
            .into_iter()
            .filter(|(rel, _)| unit_of(rel).as_deref() != Some(victim.as_str()))
            .collect();
        assert!(
            mutated.len() >= MIN_SCANNED_FILES,
            "변이가 개수 하한까지 건드리면 무엇이 이 변이를 죽였는지 갈리지 않는다 — 남은 {}",
            mutated.len()
        );
        let (missing, extra) = unit_diff(&scanned_units(&mutated), &expected_units());
        assert_eq!(missing, vec![victim], "빠진 단위를 지목하지 못했다");
        assert!(extra.is_empty(), "여분이 생기면 안 된다: {extra:?}");
    }

    /// 반대 방향 — 스캔에만 있고 매니페스트 쪽에 없는 단위도 잡아야 한다.
    #[test]
    fn a_unit_absent_from_the_manifest_side_is_reported_extra() {
        let ghost = "crates/definitely-not-a-crate".to_owned();
        let mut scanned = scanned_units(&rust_sources());
        scanned.insert(ghost.clone());
        let (missing, extra) = unit_diff(&scanned, &expected_units());
        assert!(missing.is_empty(), "빠진 단위가 없어야 한다: {missing:?}");
        assert_eq!(extra, vec![ghost], "여분 단위를 지목하지 못했다");
    }

    /// 정당한 형태는 그대로 통과해야 한다 — 판정기가 무조건 빨간 것이 아님을 못박는다.
    #[test]
    fn the_unmutated_scan_passes() {
        let (missing, extra) = unit_diff(&scanned_units(&rust_sources()), &expected_units());
        assert!(
            missing.is_empty() && extra.is_empty(),
            "{missing:?} / {extra:?}"
        );
    }
}

/// 주석·문자열·문자 리터럴을 공백으로 덮은 사본을 만든다. 줄바꿈은 그대로 두므로
/// 결과 문자열의 줄 번호는 원본과 같다. 라이프타임 틱(`'a`)은 문자 리터럴과 구분한다.
fn mask_non_code(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    while i < chars.len() {
        i = match chars[i] {
            '/' if chars.get(i + 1) == Some(&'/') => mask_line_comment(&chars, i, &mut out),
            '/' if chars.get(i + 1) == Some(&'*') => mask_block_comment(&chars, i, &mut out),
            'r' | 'b' if raw_string_hashes(&chars, i).is_some() => {
                mask_raw_string(&chars, i, &mut out)
            }
            '"' => mask_quoted(&chars, i, '"', &mut out),
            '\'' if is_char_literal(&chars, i) => mask_quoted(&chars, i, '\'', &mut out),
            c => {
                out.push(c);
                i + 1
            }
        };
    }
    out
}

/// 코드가 아닌 한 글자를 공백으로 덮는다 — 줄바꿈만 그대로 둬서 줄 번호를 지킨다.
fn blank(out: &mut String, c: char) {
    out.push(if c == '\n' { '\n' } else { ' ' });
}

fn mask_line_comment(chars: &[char], mut i: usize, out: &mut String) -> usize {
    while i < chars.len() && chars[i] != '\n' {
        blank(out, chars[i]);
        i += 1;
    }
    i
}

fn mask_block_comment(chars: &[char], mut i: usize, out: &mut String) -> usize {
    let mut depth = 0usize;
    while i < chars.len() {
        let opening = chars[i] == '/' && chars.get(i + 1) == Some(&'*');
        let closing = chars[i] == '*' && chars.get(i + 1) == Some(&'/');
        if opening || closing {
            depth = if opening { depth + 1 } else { depth - 1 };
            blank(out, chars[i]);
            blank(out, chars[i + 1]);
            i += 2;
            if closing && depth == 0 {
                break;
            }
        } else {
            blank(out, chars[i]);
            i += 1;
        }
    }
    i
}

fn mask_raw_string(chars: &[char], i: usize, out: &mut String) -> usize {
    let (quote, hashes) = raw_string_hashes(chars, i).expect("호출 전에 확인했다");
    // 접두사(`r` / `br` / `#`)는 코드다 — 여는 따옴표부터 덮는다.
    for c in &chars[i..quote] {
        out.push(*c);
    }
    let mut i = quote;
    blank(out, chars[i]);
    i += 1;
    while i < chars.len() {
        if chars[i] == '"' && chars[i + 1..].iter().take(hashes).all(|c| *c == '#') {
            for _ in 0..=hashes {
                if i < chars.len() {
                    blank(out, chars[i]);
                    i += 1;
                }
            }
            break;
        }
        blank(out, chars[i]);
        i += 1;
    }
    i
}

/// `terminator` 로 닫히는 리터럴(문자열·문자)을 덮는다. 역슬래시 이스케이프를 따른다.
fn mask_quoted(chars: &[char], mut i: usize, terminator: char, out: &mut String) -> usize {
    blank(out, chars[i]);
    i += 1;
    while i < chars.len() {
        if chars[i] == '\\' {
            blank(out, chars[i]);
            i += 1;
            if i < chars.len() {
                blank(out, chars[i]);
                i += 1;
            }
            continue;
        }
        let done = chars[i] == terminator;
        blank(out, chars[i]);
        i += 1;
        if done {
            break;
        }
    }
    i
}

/// `i` 가 raw string 접두사(`r"`, `r#"`, `br"`, `br#"` …)의 시작이면 여는 `"` 의
/// 인덱스와 `#` 개수를 돌려준다.
fn raw_string_hashes(chars: &[char], i: usize) -> Option<(usize, usize)> {
    let mut j = i;
    if chars.get(j) == Some(&'b') {
        j += 1;
    }
    if chars.get(j) != Some(&'r') {
        return None;
    }
    j += 1;
    let hash_start = j;
    while chars.get(j) == Some(&'#') {
        j += 1;
    }
    if chars.get(j) == Some(&'"') {
        Some((j, j - hash_start))
    } else {
        None
    }
}

/// `'` 가 문자 리터럴의 시작인지(아니면 라이프타임 틱인지) 가른다.
/// `'\n'` 처럼 이스케이프로 시작하거나, 두 칸 뒤가 닫는 따옴표면 문자 리터럴이다.
fn is_char_literal(chars: &[char], i: usize) -> bool {
    chars.get(i + 1) == Some(&'\\') || chars.get(i + 2) == Some(&'\'')
}

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

/// 이 파일 자신이 스캔 모수에 들어 있는지 못박는다 — 자기 제외 면제를 두지 않았다는
/// 근거다. 빠지면 이 파일 안의 진짜 위반을 어떤 가드도 못 잡게 된다.
#[test]
fn the_guard_file_scans_itself() {
    let self_path: PathBuf = ["src", "source_guards.rs"].iter().collect();
    assert!(
        rust_sources().iter().any(|(path, _)| *path == self_path),
        "가드 파일 자신이 스캔 모수에서 빠졌다 — 자기 제외 면제가 다시 생겼는지 확인해라"
    );
}

mod define_class_return {
    //! `objc2` 의 `define_class!` / `declare_class!` 본문에서 **값을 돌려주는
    //! `return`** 을 금지한다.
    //!
    //! 그 매크로는 본문을 `let __objc2_result = { ...본문... };` 로 감싸 자기가 만든
    //! `extern "C-unwind"` shim 에 심는다. shim 의 반환 타입은 소스에 적힌 타입이
    //! 아니라 **변환된** `<T as ConvertReturn<_>>::Inner` 다(`bool` → `Bool`,
    //! `Retained<_>` → 별도 표현). 그래서 `return <값>` 은 사용자가 쓴 함수가 아니라
    //! shim 을 빠져나가며 변환 후 타입으로 검사돼 컴파일이 깨진다 — 반면 꼬리
    //! 표현식은 변환 전 타입으로 추론되므로 멀쩡하다. 한 함수 안에서 두 경로의 기대
    //! 타입이 다르다.
    //!
    //! **이 함정은 macOS 에서만 컴파일된다** — Linux·Windows 개발자는 로컬에서 볼 수
    //! 없고 CI 의 macOS 잡만 본다. 그래서 소스 스캔으로 전 플랫폼에서 막는다.
    //!
    //! ## 면제와 그 근거
    //!
    //! - **값 없는 `return;` 은 허용한다**(A-1). 반환 타입이 없는 메서드에서는 매크로가
    //!   변환 없이 전개하므로 합법이다. 이 면제의 경계는
    //!   `catches_a_value_return_split_across_lines` 가 지킨다 — 줄바꿈이 끼어도 값
    //!   반환은 값 반환이다.
    //! - **주석·문자열 안은 보지 않는다**(A-2, `mask_non_code` 공통). 그 창 안쪽에
    //!   진짜 위반을 심어도 잡히는지는
    //!   `catches_a_real_return_next_to_a_commented_one` 이 확인한다.
    //!
    //! ## 의도적으로 넓게 잡는 곳
    //!
    //! 반환 타입이 `EncodeReturn` 을 그대로 만족하는 타입(예: `NSRect`)이면
    //! `return <값>` 도 사실은 합법이지만, 텍스트로는 그 구분을 못 하므로 **일괄
    //! 금지**한다. 과검출 방향이라 면제가 아니고, 표현식 형태로 쓰면 어느 경우든 옳다.

    use super::*;

    /// 스캔 하한 — 이 레포에는 `define_class!` 블록이 실제로 존재한다. 0 개가 되면
    /// 가드가 아무것도 안 보고 통과하는 것이므로, 그때는 이 하한을 의도적으로 고쳐야 한다.
    const MIN_BLOCKS: usize = 1;

    const MACROS: &[&str] = &["define_class!", "declare_class!"];
    const RETURN: &str = "return";

    /// 마스킹된 소스 하나에 대한 판정 결과. 줄 번호는 1-based.
    struct Scan {
        /// 찾은 매크로 블록 수(스캔 하한용).
        blocks: usize,
        /// 값 반환 `return` 이 있는 줄.
        violations: Vec<usize>,
        /// 구분자가 닫히지 않은 매크로 시작 줄 — 마스킹이 깨졌다는 신호다.
        unclosed: Vec<usize>,
    }

    /// 레포 전수 테스트와 합성 입력 테스트가 함께 부르는 판정기.
    fn scan(masked: &str) -> Scan {
        let mut out = Scan {
            blocks: 0,
            violations: Vec::new(),
            unclosed: Vec::new(),
        };
        for mac in MACROS {
            for start in word_positions(masked, mac) {
                let Some(open) = next_opening_delim(masked, start) else {
                    out.unclosed.push(line_of(masked, start));
                    continue;
                };
                let Some(end) = matching_delim(masked, open) else {
                    out.unclosed.push(line_of(masked, start));
                    continue;
                };
                out.blocks += 1;
                let body = &masked[open..end];
                for rel in word_positions(body, RETURN) {
                    let rest = body[rel + RETURN.len()..].trim_start();
                    if !rest.starts_with(';') {
                        out.violations.push(line_of(masked, open + rel));
                    }
                }
            }
        }
        out
    }

    #[test]
    fn no_value_returning_return_inside_define_class() {
        let mut blocks = 0usize;
        let mut violations = Vec::new();
        let mut unclosed = Vec::new();
        for (path, text) in rust_sources() {
            let found = scan(&mask_non_code(&text));
            blocks += found.blocks;
            for line in found.violations {
                violations.push(format!("{}:{line}", path.display()));
            }
            for line in found.unclosed {
                unclosed.push(format!("{}:{line}", path.display()));
            }
        }
        assert!(
            unclosed.is_empty(),
            "매크로 호출의 구분자가 닫히지 않는다 — 마스킹이 깨졌을 수 있다.\n  {}",
            unclosed.join("\n  ")
        );
        assert!(
            blocks >= MIN_BLOCKS,
            "스캔 하한 미달: {mac_list} 블록을 {blocks} 개 찾았다(하한 {MIN_BLOCKS}). \
             블록이 정말 사라졌다면 이 하한을 함께 고쳐라",
            mac_list = MACROS.join(" / "),
        );
        assert!(
            violations.is_empty(),
            "define_class!/declare_class! 본문에서 값을 돌려주는 `return` 은 매크로가 만든 \
             shim 을 빠져나가 변환 후 타입(예: bool → objc2::runtime::Bool)으로 검사된다 — \
             macOS 에서만 컴파일이 깨진다. 값은 표현식으로 흘려라(if/else 또는 match).\n  {}",
            violations.join("\n  ")
        );
    }

    /// 이 가드가 겨냥하는 유일한 실물. 이 파일은 부모(`src/host_api/webview.rs`)의
    /// `#[cfg(target_os = "macos")]` 와 조부모(`src/host_api.rs`)의
    /// `#[cfg(feature = "gui")]` 두 게이트 아래 있어 **Linux 의 어느 조합에서도
    /// 컴파일되지 않는다.** 게이트가 파일 자신에도 부모에도 없고 조부모에 있으므로,
    /// 이 파일만 열어 `cfg` 를 찾으면 "게이트 없음" 으로 잘못 읽힌다.
    const GATED_FILE: &str = "src/host_api/webview/macos.rs";

    /// 스캔이 실제로 읽어온 그 파일의 내용. 없으면 스캔이 거기 못 닿은 것이다.
    fn gated_source() -> String {
        rust_sources()
            .into_iter()
            .find(|(path, _)| path.to_string_lossy().replace('\\', "/") == GATED_FILE)
            .map(|(_, text)| text)
            .unwrap_or_else(|| panic!("스캔 결과에 {GATED_FILE} 이 없다"))
    }

    /// `MIN_BLOCKS` 하한은 **어딘가에서** 1개를 봤다는 것까지만 말한다. 그 1개가
    /// **컴파일된 적 없는 이 파일에서 왔다**는 것은 하한이 못 보여준다 — 여기서 직접
    /// 못박는다. 이 단정이 통과하는 것은 소스 스캔이 `cfg` 와 무관하게 대상을 본다는
    /// 뜻이고, 그것이 이 모듈을 `tests/` 가 아니라 여기 둔 이유의 절반이다.
    #[test]
    fn the_scan_reaches_a_file_no_local_build_compiles() {
        let found = scan(&mask_non_code(&gated_source()));
        assert!(
            found.blocks > 0,
            "{GATED_FILE} 에서 매크로 블록을 하나도 못 찾았다 — 스캔이 이 파일에 닿지 \
             못했거나 마스킹이 본문까지 지웠다"
        );
        assert!(
            found.violations.is_empty(),
            "실물 파일이 이미 위반을 담고 있다: {:?}",
            found.violations
        );
    }

    /// 위 단정은 "읽었다" 까지다. **읽은 것 안에 진짜 위반이 있으면 잡는가** 는 따로
    /// 물어야 한다 — 판정기가 대상을 보면서도 아무것도 못 보는 상태를 배제한다.
    /// 파일을 고치지 않고 읽어온 내용에 주입해서 확인하므로 트리는 그대로다.
    #[test]
    fn a_violation_planted_inside_that_gated_file_is_caught() {
        let raw = gated_source();
        let at = raw.find(MACROS[0]).expect("매크로 호출이 있어야 한다");
        let open = raw[at..]
            .find('(')
            .map(|rel| at + rel + 1)
            .expect("매크로 호출의 여는 구분자가 있어야 한다");
        let mut mutated = String::with_capacity(raw.len() + 32);
        mutated.push_str(&raw[..open]);
        mutated.push_str("\n    return true;\n");
        mutated.push_str(&raw[open..]);

        let before = scan(&mask_non_code(&raw));
        let after = scan(&mask_non_code(&mutated));
        assert!(
            before.violations.is_empty(),
            "원본이 이미 빨갛다 — 이 변이가 무엇을 보였는지 갈리지 않는다: {:?}",
            before.violations
        );
        assert_eq!(
            after.blocks, before.blocks,
            "주입이 블록 수를 바꾸면 안 된다(잡힌 이유가 흐려진다)"
        );
        assert_eq!(
            after.violations.len(),
            1,
            "게이트된 파일 안에 심은 값 반환을 못 잡았다(또는 과검출했다): {:?}",
            after.violations
        );
    }

    mod exemption_mutations {
        //! 이 가드의 **면제마다** 그것을 겨냥한 변이. 면제 창 안쪽에 진짜 위반을 심었을
        //! 때 잡히는지를 묻는다 — 면제를 넣기만 하고 검증하지 않으면 그 면제만큼 구멍이다.

        use super::*;

        /// A-2(주석·문자열 마스킹)를 겨냥한다. 주석 속 가짜 `return` 바로 옆에 진짜
        /// 위반을 두어, 마스킹이 진짜까지 삼키지 않는지 본다.
        #[test]
        fn catches_a_real_return_next_to_a_commented_one() {
            let src = "define_class!(\n    impl X {\n        fn f(&self) -> bool {\n            /* return false; */ return true;\n        }\n    }\n);\n";
            let found = scan(&mask_non_code(src));
            assert_eq!(found.blocks, 1);
            assert_eq!(found.violations, vec![4]);
        }

        /// 같은 면제의 정당한 쪽 — 문자열 안의 `return` 은 코드가 아니다.
        ///
        /// 스니펫에 `let _ =` 를 쓰지 않는다: pre-commit 의 `let _` 검사는 문자열
        /// 리터럴을 덮지 않아 합성 스니펫 **안쪽**을 진짜 코드로 오인한다(이 가드가
        /// 마스킹으로 피하는 바로 그 함정이다).
        #[test]
        fn ignores_a_return_that_only_appears_in_a_string_literal() {
            let src = "define_class!(\n    impl X {\n        fn f(&self) -> usize {\n            let s = \"return true;\";\n            s.len()\n        }\n    }\n);\n";
            let found = scan(&mask_non_code(src));
            assert_eq!(found.blocks, 1);
            assert!(found.violations.is_empty());
        }

        /// A-1(값 없는 `return;` 허용)의 경계 — 줄바꿈이 끼어도 값 반환은 값 반환이다.
        #[test]
        fn catches_a_value_return_split_across_lines() {
            let src = "define_class!(\n    fn f() -> bool {\n        return\n            true;\n    }\n);\n";
            assert_eq!(scan(&mask_non_code(src)).violations, vec![3]);
        }

        /// A-1 의 정당한 쪽 — 세미콜론 앞에 공백이 끼어도 값이 없으면 통과다.
        #[test]
        fn allows_a_bare_return_with_whitespace_before_the_semicolon() {
            let src = "define_class!(\n    fn f() {\n        return ;\n    }\n);\n";
            assert!(scan(&mask_non_code(src)).violations.is_empty());
        }

        /// 스캔 범위의 경계 — 블록 밖의 값 반환은 이 가드의 대상이 아니다.
        #[test]
        fn ignores_returns_outside_any_macro_block() {
            let src = "fn f() -> bool {\n    return true;\n}\n";
            let found = scan(&mask_non_code(src));
            assert_eq!(found.blocks, 0);
            assert!(found.violations.is_empty());
        }

        /// 구분자 면제 — 매크로를 중괄호로 불러도 같은 블록으로 본다.
        #[test]
        fn handles_a_brace_delimited_macro_call() {
            let src = "define_class! {\n    fn f() -> bool {\n        return true;\n    }\n}\n";
            let found = scan(&mask_non_code(src));
            assert_eq!(found.blocks, 1);
            assert_eq!(found.violations, vec![3]);
        }
    }
}

mod read_only_handle_mtime {
    //! 읽기 전용으로 연 `File` 핸들에 **mtime 을 쓰는 것**을 금지한다.
    //!
    //! `File::open` 은 읽기 접근만 얻는다. Windows 의 `SetFileTime` 은 핸들에
    //! `FILE_WRITE_ATTRIBUTES` 를 요구하므로 그 핸들로 `set_modified`/`set_times` 를
    //! 부르면 `PermissionDenied(os error 5)` 가 난다. POSIX `futimens` 는 소유자면
    //! 읽기 전용 fd 로도 통과하므로 **Linux·macOS 에서는 드러나지 않는다** — 쓰기
    //! 권한으로 열면(`OpenOptions::new().write(true).open(..)`) 양쪽 다 동작한다.
    //!
    //! 이 함정은 서로 독립인 두 crate 의 테스트에서 같은 형태로 반복됐다. 한쪽을
    //! 고쳐도 다른 쪽이 남는 부류라 텍스트로 못박는다.
    //!
    //! ## 판정 방식
    //!
    //! 이름이 아니라 **같은 표현식 체인인지**로 가른다 — 마스킹된 소스를 `;` 단위
    //! 구문으로 잘라, 한 구문 안에 `File::open(` 과 `.set_modified(`(또는
    //! `.set_times(`)가 함께 있을 때만 잡는다. 그래서 `OpenOptions` 로 연 핸들은
    //! 통과한다(B-2, `does_not_flag_a_handle_opened_for_write`).
    //!
    //! ## 면제를 더 좁히지 않는 이유 — "면제는 좁게" 의 예외
    //!
    //! 창 단위를 `;` 구문에서 **줄**로 좁히면 rustfmt 가 줄바꿈한 멀티라인 체인
    //! (이 레포의 실제 위반이 정확히 그 형태였다)을 통째로 놓친다. 즉 여기서는
    //! **좁히는 쪽이 검출을 깎는다.** "면제는 좁게" 는 창이 문법 단위보다 넓을 때의
    //! 처방이고, `;` 는 이 판정에서 곧 문법 단위(구문 = 표현식 체인의 경계)다.
    //! 다음 사람이 이 예외를 규칙의 누락으로 오해하지 않도록 여기 적어 둔다.
    //!
    //! ## 의도된 false negative — 구문 경계 밖 바인딩(cross-statement binding)
    //!
    //! 핸들을 변수에 담아 두 구문으로 나눈 형태
    //! (`let f = File::open(p)?;` / `f.set_modified(t)?;`)는 **일부러 안 잡는다**.
    //! 텍스트만으로는 그 변수가 어떻게 열렸는지 따라갈 수 없기 때문이다. 못 가르는
    //! 것을 가르는 척하지 않으려는 결정이며,
    //! `intentionally_misses_a_handle_bound_across_statements` 가 그 결정을 고정한다.
    //! 나중에 판정기가 이 형태를 잡게 된다면 그건 버그를 고친 것이 아니라 **이 결정을
    //! 바꾼 것**이므로, 그 테스트를 함께 고쳐야 한다.

    use super::*;

    /// mtime 을 쓰는 호출이 레포에서 통째로 사라지면 이 가드는 아무것도 안 보고
    /// 통과한다. 실제로 사라졌다면 이 하한을 의도적으로 고쳐야 한다.
    const MIN_MTIME_WRITE_SITES: usize = 1;

    const READ_ONLY_OPEN: &str = "File::open(";
    const MTIME_WRITES: &[&str] = &[".set_modified(", ".set_times("];

    /// 마스킹된 소스 하나에 대한 판정 결과. 줄 번호는 1-based.
    struct Scan {
        /// mtime 을 쓰는 호출 수(스캔 하한용).
        sites: usize,
        /// 읽기 전용 핸들로 쓰는 줄.
        violations: Vec<usize>,
    }

    /// 레포 전수 테스트와 합성 입력 테스트가 함께 부르는 판정기.
    fn scan(masked: &str) -> Scan {
        let mut out = Scan {
            sites: 0,
            violations: Vec::new(),
        };
        let mut stmt_start = 0usize;
        for (offset, _) in masked.match_indices(';').chain([(masked.len(), "")]) {
            let stmt = &masked[stmt_start..offset];
            for needle in MTIME_WRITES {
                let Some(rel) = stmt.find(needle) else {
                    continue;
                };
                out.sites += 1;
                if stmt.contains(READ_ONLY_OPEN) {
                    out.violations.push(line_of(masked, stmt_start + rel));
                }
            }
            stmt_start = offset + 1;
        }
        out
    }

    #[test]
    fn mtime_is_never_written_through_a_read_only_handle() {
        let mut sites = 0usize;
        let mut violations = Vec::new();
        for (path, text) in rust_sources() {
            let found = scan(&mask_non_code(&text));
            sites += found.sites;
            for line in found.violations {
                violations.push(format!("{}:{line}", path.display()));
            }
        }
        assert!(
            sites >= MIN_MTIME_WRITE_SITES,
            "스캔 하한 미달: mtime 을 쓰는 호출을 {sites} 곳 찾았다(하한 \
             {MIN_MTIME_WRITE_SITES}). 정말 사라졌다면 이 하한을 함께 고쳐라"
        );
        assert!(
            violations.is_empty(),
            "`File::open` 은 읽기 접근만 얻는다 — 그 핸들로 mtime 을 쓰면 Windows 에서 \
             `PermissionDenied(os error 5)` 가 난다(Linux·macOS 는 통과해서 안 드러난다). \
             `std::fs::OpenOptions::new().write(true).open(..)` 로 열어라.\n  {}",
            violations.join("\n  ")
        );
    }

    mod exemption_mutations {
        //! 이 가드의 **면제마다** 그것을 겨냥한 변이. 특히 B-1(`;` 구문 창)은 두 needle
        //! **사이**에 세미콜론을 숨겨야 판별력이 생긴다 — 세미콜론이 needle 뒤에 있으면
        //! 구문이 갈려도 앞 조각에 둘 다 남아 여전히 잡히므로 면제를 찌르지 못한다.

        use super::*;

        /// B-1 을 겨냥한다. 문자열 속 `;` 가 구문을 잘라 버리면 `File::open(` 과
        /// `.set_modified(` 가 서로 다른 조각으로 갈려 진짜 위반이 빠져나간다.
        #[test]
        fn catches_a_chain_whose_semicolon_hides_in_a_string() {
            let src = "std::fs::File::open(&p.join(\"a;b\")).unwrap().set_modified(t).unwrap();\n";
            let found = scan(&mask_non_code(src));
            assert_eq!(found.sites, 1);
            assert_eq!(found.violations, vec![1]);
        }

        /// 같은 면제, 주석판.
        #[test]
        fn catches_a_chain_whose_semicolon_hides_in_a_comment() {
            let src = "std::fs::File::open(&p /* ; */).unwrap().set_modified(t).unwrap();\n";
            let found = scan(&mask_non_code(src));
            assert_eq!(found.sites, 1);
            assert_eq!(found.violations, vec![1]);
        }

        /// 같은 면제, 멀티라인 체인 — 줄 단위로 좁히면 놓치는 형태(레포의 실제 위반이
        /// 이 모양이었다). 위반 줄은 mtime 을 쓰는 줄로 보고한다.
        #[test]
        fn catches_a_chain_broken_across_lines_by_rustfmt() {
            let src = "std::fs::File::open(&stale)\n    .unwrap()\n    .set_modified(old)\n    .unwrap();\n";
            assert_eq!(scan(&mask_non_code(src)).violations, vec![3]);
        }

        /// B-2 의 정당한 쪽 — 쓰기 권한으로 연 핸들은 잡지 않되, 호출 수에는 센다.
        #[test]
        fn does_not_flag_a_handle_opened_for_write() {
            let src = "std::fs::OpenOptions::new().write(true).open(&p).unwrap().set_modified(t).unwrap();\n";
            let found = scan(&mask_non_code(src));
            assert_eq!(found.sites, 1);
            assert!(found.violations.is_empty());
        }

        /// **의도된 false negative — 구문 경계 밖 바인딩.** 이 테스트가 깨졌다면
        /// 판정기가 넓어진 것이고, 그건 버그 수정이 아니라 결정 변경이다.
        #[test]
        fn intentionally_misses_a_handle_bound_across_statements() {
            let src = "let f = std::fs::File::open(&p).unwrap();\nf.set_modified(t).unwrap();\n";
            let found = scan(&mask_non_code(src));
            assert_eq!(found.sites, 1);
            assert!(
                found.violations.is_empty(),
                "구문 경계 밖 바인딩은 일부러 안 잡는다 — 판정기를 넓혔다면 이 결정을 \
                 바꾼 것이므로 가드 doc 도 함께 고쳐라"
            );
        }

        /// `set_times` 도 같은 부류다 — needle 목록이 줄어들지 않았는지 본다.
        #[test]
        fn catches_set_times_as_well_as_set_modified() {
            let src = "std::fs::File::open(&p).unwrap().set_times(times).unwrap();\n";
            assert_eq!(scan(&mask_non_code(src)).violations, vec![1]);
        }
    }
}

// ── 워크플로: 테스트를 **실행하는** 스텝은 `--no-fail-fast` 를 갖는다 ────────
//
// `cargo test` 는 기본적으로 **처음 실패한 테스트 바이너리에서 멈춘다.** 그러면 그 뒤에
// 오는 타깃이 한 번도 실행되지 않는데, 로그는 "N failed" 라고만 말한다. 실측으로 기본
// 조합 `--lib --bins` 가 이 플래그 없이는 바이너리 1 개(2017 passed)에서 멈췄고, 붙이면
// 52 개(4551 passed)가 돌았다 — 51 개 크레이트가 조용히 가려져 있었다.
//
// 이 결함은 **문서 주장이 아니라 워크플로 내부의 비대칭**이라, 문서와 워크플로를 대조하는
// `tests/ci_channel_claims_match_workflows.rs` 가 보지 못한다. 한 잡에 플래그를 넣으면서
// 같은 파일의 다른 잡을 놓치는 형태가 실제로 있었고, 그것을 막는 것이 이 가드다.

/// 워크플로 디렉토리(레포 루트 기준).
const WORKFLOW_DIR: &str = ".github/workflows";

/// 스캔 하한 — 디렉토리를 잘못 짚으면 0 개를 읽고 조용히 통과한다.
const MIN_WORKFLOW_FILES: usize = 4;

/// `cargo test` 호출 개수의 하한. 같은 이유.
const MIN_TEST_INVOCATIONS: usize = 4;

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

#[cfg(test)]
mod workflow_fail_fast_tests {
    use super::*;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn every_workflow_step_that_runs_tests_uses_no_fail_fast() {
        let dir = repo_root().join(WORKFLOW_DIR);
        let entries = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("워크플로 디렉토리를 읽지 못했다: {}: {e}", dir.display()));
        let (mut files, mut invocations, mut violations) = (0usize, 0usize, Vec::new());
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "yml" && e != "yaml") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            files += 1;
            invocations += cargo_test_invocations(&flatten_workflow(&text)).len();
            for inv in test_invocations_missing_no_fail_fast(&text) {
                violations.push(format!("{}: {inv}", path.display()));
            }
        }
        assert!(
            files >= MIN_WORKFLOW_FILES,
            "스캔 하한 미달: 워크플로 파일 {files} 개(하한 {MIN_WORKFLOW_FILES}) — 경로가 틀렸다"
        );
        assert!(
            invocations >= MIN_TEST_INVOCATIONS,
            "스캔 하한 미달: `cargo test` 호출 {invocations} 개(하한 {MIN_TEST_INVOCATIONS})"
        );
        assert!(
            violations.is_empty(),
            "테스트를 실행하는 워크플로 스텝에 `--no-fail-fast` 가 없다. 없으면 처음 실패한 \
             테스트 바이너리에서 멈춰 뒤따르는 타깃이 통째로 실행되지 않고, 로그는 그것을 \
             '실패 N 건' 으로만 보고한다. 컴파일만 하는 호출이면 `--no-run` 을 함께 써라.\n  {}",
            violations.join("\n  ")
        );
    }

    #[test]
    fn the_no_run_exemption_covers_only_compile_only_invocations() {
        // 면제를 겨냥한 변이 — 면제 창 안쪽(같은 스텝, 같은 명령 형태)에 진짜 실행 호출을
        // 심으면 잡혀야 한다.
        let compile_only =
            "      - name: Build\n        run: cargo test --workspace --no-run --locked\n";
        assert!(test_invocations_missing_no_fail_fast(compile_only).is_empty());

        let runs = "      - name: Run\n        run: cargo test --workspace --locked\n";
        assert_eq!(test_invocations_missing_no_fail_fast(runs).len(), 1);

        // 같은 스텝에 둘이 붙어 있어도 앞의 `--no-run` 이 뒤를 면제하지 않는다.
        let both = "        run: |\n          cargo test --workspace --no-run --locked\n          cargo test --workspace --locked\n";
        assert_eq!(test_invocations_missing_no_fail_fast(both).len(), 1);
    }

    #[test]
    fn a_flag_on_a_continuation_line_or_folded_scalar_still_counts() {
        // 줄 끝 `\` 이음 — 줄 단위로 보면 여기서 끊겨 있는 플래그를 놓친다.
        let cont = "        run: |\n          cargo test --workspace --locked \\\n            --no-fail-fast\n";
        assert!(test_invocations_missing_no_fail_fast(cont).is_empty());
        // `>` 접힌 스칼라.
        let folded = "        run: >\n          cargo test --locked\n          --no-fail-fast\n";
        assert!(test_invocations_missing_no_fail_fast(folded).is_empty());
    }

    #[test]
    fn a_step_name_that_quotes_the_command_is_not_an_invocation() {
        // 이 레포의 스텝 이름은 `cargo test (unit)` 처럼 명령을 그대로 쓴다. 이름을
        // 호출로 세면 오탐이고, 이름 슬라이스가 다음 `cargo ` 앞까지라 **뒤따르는 진짜
        // 명령의 플래그를 대신 물어 그 명령을 검사에서 빼 버린다** — 오탐보다 이쪽이 나쁘다.
        let yaml =
            "      - name: cargo test (unit)\n        run: cargo test --workspace --locked\n";
        assert_eq!(test_invocations_missing_no_fail_fast(yaml).len(), 1);
        let named_ok = "      - name: cargo test (unit)\n        run: cargo test --workspace --locked --no-fail-fast\n";
        assert!(test_invocations_missing_no_fail_fast(named_ok).is_empty());
    }

    #[test]
    fn a_comment_mentioning_the_flag_does_not_exempt_a_step() {
        // 주석이 면제해 주면 가드가 스스로 무력해진다 — 이 레포의 워크플로 주석에는
        // 실제로 이 플래그 이름이 나온다.
        let yaml = "      # `--no-fail-fast` 는 필수다\n      - name: unit\n        run: cargo test --workspace --locked\n";
        assert_eq!(test_invocations_missing_no_fail_fast(yaml).len(), 1);
    }
}
