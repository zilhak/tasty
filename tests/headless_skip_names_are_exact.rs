//! `check-headless` 의 명명 `--skip` 이 **정확히 하나의 테스트**를 가리키는지 고정한다.
//!
//! libtest 의 `--skip` 은 **부분일치**다 — 테스트 경로(`모듈::이름`) 어디에든 그 문자열이
//! 들어 있으면 빠진다. 그래서 두 방향으로 조용히 어긋난다.
//!
//! - **과소(0 건)**: 이름이 바뀌거나 사라지면 그 `--skip` 은 아무것도 안 잡는다. rc 에도
//!   본문에도 안 나온다 — 오타난 `--skip` 은 경고 없이 무시된다(실측).
//! - **과대(2 건 이상)**: 나중에 그 문자열을 품는 이름이 생기면 **의도 없이 함께 빠진다.**
//!   초록인데 검증 범위가 준다. `check-headless` 는 이 저장소에서 전체 스위트를 자동으로
//!   도는 **유일한** 조합이라, 여기서 조용히 빠지면 어디서도 안 돈다.
//!
//! **이 가드는 `--skip` 하나가 테스트 하나를 가리킨다는 것을 불변식으로 박는다.** 언젠가
//! 모듈을 통째로 빼고 싶어지면 여기가 먼저 빨개진다 — **조용히 넓어지는 것보다 시끄럽게
//! 막히는 쪽**을 고른 것이다. 그때는 skip 이 아니라 `#[ignore]` 나 cfg 로 가르거나, 이
//! 가드의 불변식을 의도적으로 고쳐야 한다.
//!
//! # 무엇을 세는가 — 식별자가 아니라 테스트다
//!
//! 첫 판은 `fn `/`mod ` 뒤의 토큰을 긁어 **이름 집합**의 크기를 셌다. 표방한 명제는
//! "테스트 하나" 인데 잰 양은 "그 문자열을 품는 **식별자 이름의 가짓수**" 였고, 둘은
//! 두 방향으로 어긋난다.
//!
//! - **과소**: 크레이트가 달라도 이름이 같으면 집합이 하나로 합친다. 같은 이름의 테스트가
//!   두 크레이트에 있으면 실제로는 둘이 빠지는데 하나로 센다.
//! - **과대**: 테스트가 아닌 **제품 함수**도 이름만 품으면 센다. 테스트 이름을 그 테스트가
//!   검증하는 함수 이름으로 짓는 것은 흔한 관례라, 이 오차는 드문 경우가 아니다.
//!
//! 두 오차는 **서로 상쇄**할 수 있다. 그러면 나오는 것은 틀린 값이 아니라 **그럴듯한 값**
//! 이고, 그런 값은 검산되지 않는다. `--skip` 의 사거리가 1 인 것과 그 1 을 만든 계산이
//! 옳은 것은 **독립 명제**다 — 지금 초록인 이유가 "이름이 우연히 안 겹쳐서" 이면, 다음
//! `--skip` 이 제품 함수와 같은 이름일 때 **위반 없이 빨개진다.**
//!
//! 그래서 지금은 `#[test]` 가 붙은 fn 만, 이름 집합이 아니라 **건수**로, 주석·문자열을
//! 지운 사본에서 센다.
//!
//! # 사거리 (R16)
//!
//! - 테스트 이름 집합을 `cargo test -- --list` 가 아니라 **소스 텍스트**에서 얻는다.
//!   실측(2026-09-05): `macro_rules!` 를 가진 `.rs` 11 개 중 본문에 `#[test]` 를 담은 것이
//!   **0 건**이라, 이 저장소의 테스트 이름은 전부 소스에 리터럴 `fn` 으로 있다. 매크로가
//!   테스트를 만들기 시작하면 이 전제가 깨지고, 그때 이 가드는 **말없이 약해진다.**
//! - 모수는 workspace 안으로 한정한다. `[workspace] exclude` 는 `cargo test --workspace`
//!   가 아예 안 보므로 그 안의 이름은 `--skip` 의 사거리가 아니다. 제외 목록은 외우지 않고
//!   `Cargo.toml` 에서 읽는다.
//! - **모듈 경로는 세지 않고 막는다.** `--skip` 은 `모듈::이름` 전체를 보므로 모듈 이름이
//!   그 문자열을 품으면 그 아래 전부가 빠지는데, 전체 경로를 텍스트로 복원하는 것은 이
//!   가드의 값보다 비싸다. 대신 "모듈 이름 중 품는 것이 없다" 를 별도로 단정한다 —
//!   있으면 사거리를 셀 수 없으므로 통과가 아니라 **실패**로 다룬다. 상한을 닫는 쪽이다.

/// 어휘 마스킹은 공용 모듈이 한 벌로 갖는다 — 사본이 둘이면 갈리고, 갈린 쪽은 조용하다.
/// 이 파일이 아래에서 합성 픽스처를 문자열로 들고 있으므로, 마스킹이 없으면 **가드가 자기
/// 픽스처를 진짜 테스트로 센다.**
mod rust_mask;
use rust_mask::mask_non_code;

use std::fs;
use std::path::{Path, PathBuf};

const WORKFLOW: &str = ".github/workflows/crossplatform-check.yml";
const STEP_ANCHOR: &str = "- name: cargo test (headless)";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 워크플로의 `cargo test (headless)` 스텝에서 `--skip` 인자를 **읽어온다**.
///
/// 값을 이 파일에 박아두면 워크플로가 바뀌는 순간 만료되고, 만료된 가드는 CI 보다 약한
/// 초록을 낸다. 앵커를 못 찾으면 0 건이 되는데, 그 0 은 "skip 이 없다" 가 아니라
/// "파서가 죽었다" 이므로 구분해서 죽는다.
fn skips_from_workflow() -> Vec<String> {
    let text = fs::read_to_string(repo_root().join(WORKFLOW))
        .unwrap_or_else(|e| panic!("{WORKFLOW} 를 읽을 수 없다: {e}"));
    let start = text
        .find(STEP_ANCHOR)
        .unwrap_or_else(|| panic!("워크플로에서 `{STEP_ANCHOR}` 스텝을 못 찾았다 — 앵커가 깨졌다"));
    let rest = &text[start + STEP_ANCHOR.len()..];
    // 다음 스텝(`- name:`) 전까지가 이 스텝의 블록이다.
    let block = match rest.find("- name:") {
        Some(i) => &rest[..i],
        None => rest,
    };

    let mut out = Vec::new();
    let mut cursor = block;
    while let Some(i) = cursor.find("--skip") {
        let after = &cursor[i + "--skip".len()..];
        let name: String = after
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            out.push(name);
        }
        cursor = after;
    }
    assert!(
        !out.is_empty(),
        "`{STEP_ANCHOR}` 블록에서 `--skip` 을 하나도 못 읽었다 — \
         진짜 0 건인지 파서가 죽은 건지 구분되지 않으므로 실패로 다룬다"
    );
    out
}

/// `cargo test --workspace` 가 실제로 컴파일하는 `.rs`. `[workspace] exclude` 는 **읽어서**
/// 뺀다 — 여기에 박아 두면 `Cargo.toml` 이 바뀌는 순간 모수가 조용히 어긋난다.
fn workspace_sources() -> Vec<(PathBuf, String)> {
    let root = repo_root();
    let manifest =
        fs::read_to_string(root.join("Cargo.toml")).expect("루트 Cargo.toml 을 읽어야 한다");
    let excluded: Vec<String> = manifest
        .lines()
        .find(|l| l.trim_start().starts_with("exclude"))
        .map(|l| l.split('"').skip(1).step_by(2).map(str::to_owned).collect())
        .unwrap_or_default();
    assert!(
        !excluded.is_empty(),
        "`[workspace] exclude` 를 한 항목도 못 읽었다 — 파서가 죽으면 모수가 넓어져 \
         회차에 들어가지도 않는 이름을 사거리로 센다"
    );

    fn walk(dir: &Path, root: &Path, excluded: &[String], out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name();
            let name = name.to_string_lossy().into_owned();
            if p.is_dir() {
                if name == "target" || name == ".git" || name == "assets" || name.starts_with('.') {
                    continue;
                }
                let rel = p
                    .strip_prefix(root)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");
                if excluded.contains(&rel) {
                    continue;
                }
                walk(&p, root, excluded, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(&root, &root, &excluded, &mut files);
    files.sort();
    files
        .into_iter()
        .filter_map(|p| {
            let text = fs::read_to_string(&p).ok()?;
            Some((p, mask_non_code(&text)))
        })
        .collect()
}

/// `#[test]` 가 붙은 fn 의 이름. **건수를 보존한다** — 같은 이름이 여러 크레이트에 있으면
/// 그만큼 실제로 빠지므로 집합으로 합치지 않는다.
fn test_fn_names(masked: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = masked;
    while let Some(i) = cursor.find("#[test]") {
        let after = &cursor[i + "#[test]".len()..];
        // `#[test]` 와 `fn` 사이에는 다른 속성(`#[ignore]` 등)이 올 수 있다. 다음 `fn` 을
        // 찾되, 그 전에 다른 `#[test]` 가 나오면 이 자리는 fn 없이 끝난 것으로 본다.
        if let Some(f) = after.find("fn ") {
            let stop = after.find("#[test]").unwrap_or(usize::MAX);
            if f < stop {
                let ident: String = after[f + 3..]
                    .chars()
                    .skip_while(|c| c.is_whitespace())
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !ident.is_empty() {
                    out.push(ident);
                }
            }
        }
        cursor = after;
    }
    out
}

/// `mod <이름>` 선언의 이름들. 사거리를 세는 데 쓰지 않고, **품는 것이 있는지**만 본다.
fn module_names(masked: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = masked;
    while let Some(i) = cursor.find("mod ") {
        let before_ok = cursor[..i]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
        let after = &cursor[i + "mod ".len()..];
        if before_ok {
            let ident: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !ident.is_empty() {
                out.push(ident);
            }
        }
        cursor = after;
    }
    out
}

#[test]
fn every_named_skip_matches_exactly_one_test() {
    let sources = workspace_sources();
    let tests: Vec<String> = sources.iter().flat_map(|(_, m)| test_fn_names(m)).collect();
    let modules: Vec<String> = sources.iter().flat_map(|(_, m)| module_names(m)).collect();

    // 비영 대조 — 파서가 죽으면 0 이 되고, 0 은 "일치가 없다" 로 읽혀 조용히 통과한다.
    assert!(
        tests.len() > 1000,
        "workspace 에서 `#[test]` fn 을 {} 개밖에 못 찾았다 — 파서나 모수가 깨졌다. \
         이 수가 작으면 아래 '정확히 하나' 판정은 아무것도 안 본 결과다",
        tests.len()
    );
    assert!(
        modules.len() > 100,
        "`mod` 선언을 {} 개밖에 못 찾았다 — 모듈 쪽 상한 단정이 무의미해진다",
        modules.len()
    );

    for skip in skips_from_workflow() {
        // 상한을 먼저 닫는다: 모듈 이름이 품으면 그 아래 전부가 빠지는데, 전체 경로를
        // 복원하지 않는 이 가드는 그 수를 셀 수 없다. 셀 수 없으면 통과가 아니다.
        let module_hits: Vec<&String> = modules.iter().filter(|m| m.contains(&skip)).collect();
        assert!(
            module_hits.is_empty(),
            "`--skip {skip}` 이 **모듈 이름** {module_hits:?} 과도 부분일치한다 — libtest 는 \
             `모듈::이름` 전체를 보므로 그 모듈 아래 테스트가 통째로 빠진다. 이 가드는 전체 \
             경로를 복원하지 않아 몇 개가 빠지는지 셀 수 없다. 셀 수 없는 것을 통과로 세지 \
             않는다 — skip 문자열을 모듈 이름과 겹치지 않게 고쳐라."
        );

        let hits: Vec<&String> = tests.iter().filter(|t| t.contains(&skip)).collect();
        assert!(
            !hits.is_empty(),
            "`--skip {skip}` 이 아무 테스트 이름과도 일치하지 않는다 — 죽은 skip 이다. \
             이름이 바뀌었거나 테스트가 사라졌고, 그동안 그 skip 은 조용히 무시돼 왔다. \
             워크플로에서 지우거나 현재 이름으로 고쳐라. (`#[test]` fn {} 개를 봤다)",
            tests.len()
        );
        assert_eq!(
            hits.len(),
            1,
            "`--skip {skip}` 이 테스트 {}개와 일치한다: {hits:?} — 부분일치라 의도하지 않은 \
             테스트까지 함께 빠진다. skip 문자열을 더 길게 적거나, 정말 여럿을 빼야 한다면 \
             이 가드의 불변식(skip 하나 = 테스트 하나)을 먼저 고쳐라 — 조용히 넓어지게 두지 \
             않는다.",
            hits.len()
        );
    }
}

#[test]
fn the_parser_reads_the_workflow_rather_than_a_hardcoded_list() {
    // 이 가드 자신이 만료되지 않는지 본다: 워크플로에서 읽은 목록이 비어 있지 않아야 하고,
    // 그 이름들이 이 파일 안에 리터럴로 박혀 있지 않아야 한다.
    let skips = skips_from_workflow();
    assert!(!skips.is_empty());
    let own_source = fs::read_to_string(repo_root().join("tests/headless_skip_names_are_exact.rs"))
        .expect("자기 소스를 읽을 수 있어야 한다");
    for s in &skips {
        assert!(
            !own_source.contains(s.as_str()),
            "skip 이름 `{s}` 이 이 가드 소스에 박혀 있다 — 워크플로에서 읽는 의미가 없어진다"
        );
    }
}

/// **식별자를 세는 것과 테스트를 세는 것은 같지 않다.** 첫 판이 무엇을 놓쳤는지 합성
/// 입력으로 못박는다 — 진짜 이름에 기대면 그 테스트가 개명되는 날 이 대조가 조용히 죽는다.
///
/// 두 오차가 **서로 상쇄**해 그럴듯한 수가 나오는 것까지 같은 자리에서 보인다.
#[test]
fn counting_identifiers_is_not_counting_tests() {
    // 파일 둘에 같은 이름의 테스트가 하나씩(→ 실제로는 둘이 빠진다) + 이름을 품는 제품 함수
    // 하나(→ 테스트가 아니라 빠지지 않는다).
    let file_a = "#[test]\nfn sweeps_the_thing() {}\n";
    let file_b = "#[test]\nfn sweeps_the_thing() {}\n";
    let file_c = "fn sweeps_the_thing_impl() -> u8 { 0 }\n";
    let masked: Vec<String> = [file_a, file_b, file_c]
        .iter()
        .map(|t| mask_non_code(t))
        .collect();

    let needle = "sweeps_the_thing";
    let tests: Vec<String> = masked.iter().flat_map(|m| test_fn_names(m)).collect();
    let hits = tests.iter().filter(|t| t.contains(needle)).count();

    // 첫 판의 계산: `fn `/`mod ` 뒤 토큰을 이름 **집합**으로 모은다.
    let mut ident_set = std::collections::BTreeSet::new();
    for m in &masked {
        let mut cursor = m.as_str();
        while let Some(i) = cursor.find("fn ") {
            let after = &cursor[i + 3..];
            let ident: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if ident.contains(needle) {
                ident_set.insert(ident);
            }
            cursor = after;
        }
    }

    assert_eq!(
        hits, 2,
        "실제로 빠지는 테스트는 둘이다 (파일 둘에 같은 이름)"
    );
    assert_eq!(
        ident_set.len(),
        2,
        "첫 판은 이름 집합이라 같은 이름을 하나로 합치고(과소), 대신 제품 함수를 \
         더한다(과대): {ident_set:?}"
    );
    assert_ne!(
        ident_set.iter().collect::<Vec<_>>(),
        vec![
            &"sweeps_the_thing".to_string(),
            &"sweeps_the_thing".to_string()
        ],
        "두 계산이 같은 수를 내더라도 같은 것을 센 것이 아니다"
    );
    // 상쇄의 실물: 수는 둘 다 2 인데 **구성이 다르다.** 첫 판의 2 에는 빠지지 않는 제품
    // 함수가 들어 있고, 실제로 빠지는 둘째 테스트는 빠져 있다.
    assert!(
        ident_set.contains("sweeps_the_thing_impl"),
        "첫 판은 제품 함수를 센다 — 이게 과대 방향이다: {ident_set:?}"
    );
}

/// 마스킹이 실제로 듣는가 — **이 파일 자신이 시험대다.** 위 합성 픽스처는 문자열 리터럴
/// 안에 `#[test]` 와 `fn` 을 담고 있어서, 마스킹이 없으면 가드가 자기 픽스처를 진짜
/// 테스트로 센다.
/// `assets` 이름 면제가 지금 덮는 것이 0 이라는 사실을 박는다.
///
/// 모수 워커는 레포 전체를 훑으므로, cargo 가 컴파일하지 않는 `.rs` 가 섞이면
/// 회차에 들어가지도 않는 이름을 사거리로 세게 된다. `assets` 면제는 그것을
/// 막으려고 있는데, 근거가 이름이라 강제되는 것이 없다.
///
/// 지금 그 면제가 덮는 파일은 없다 — 그리고 **0 은 안전한 것이 아니라 관측되지
/// 않은 것이다.** 면제가 옳은지 그른지 아무도 볼 수 없는 상태이고, 누군가
/// `assets/` 아래에 `.rs` 를 두면 그 순간 조용히 활성화된다. 그 사람은 면제
/// 목록에 손을 안 댔으므로 자기가 무엇을 가렸는지 모른다.
///
/// 그래서 이름 면제를 지우지도(지우면 컴파일 안 되는 `.rs` 를 세게 된다),
/// 성질로 바꾸지도(갈래도 대상도 하나라 틀을 세울 일이 아니다) 않는다. 대신
/// 조건이 깨지는 순간 여기서 시끄럽게 터지게 한다.
#[test]
fn the_assets_name_exemption_still_covers_nothing() {
    fn scan(dir: &Path, in_assets: bool, hits: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name();
            let name = name.to_string_lossy().into_owned();
            if p.is_dir() {
                if name == "target" || name == ".git" || name.starts_with('.') {
                    continue;
                }
                scan(&p, in_assets || name == "assets", hits);
            } else if in_assets && p.extension().is_some_and(|x| x == "rs") {
                hits.push(p);
            }
        }
    }
    let root = repo_root();
    let mut hits = Vec::new();
    scan(&root, false, &mut hits);

    // 비영 대조 — 스캐너가 죽어서 0 이 나온 것이 아님을 같은 회차에서 확인한다.
    let mut any = Vec::new();
    scan(&root.join("src"), true, &mut any);
    assert!(
        any.len() > 100,
        "스캐너가 `.rs` 를 {} 개밖에 못 찾았다 — 0 이 면제의 성질이 아니라 측정 실패다",
        any.len()
    );

    assert!(
        hits.is_empty(),
        "`assets/` 아래에 `.rs` 가 생겼다: {hits:?}\n\
         이름 면제가 방금부터 이 파일들을 모수에서 가린다. 가리는 것이 맞는지 \
         정하고, 맞다면 근거를 이름이 아닌 성질(cargo 가 컴파일하는가)로 다시 써라."
    );
}

#[test]
fn the_fixtures_in_this_file_are_not_counted_as_tests() {
    let raw = fs::read_to_string(repo_root().join("tests/headless_skip_names_are_exact.rs"))
        .expect("자기 소스를 읽을 수 있어야 한다");
    let fixture = "sweeps_the_thing";
    assert!(
        raw.contains(fixture),
        "픽스처 이름이 이 파일에서 사라졌다 — 이 대조가 아무것도 안 본다"
    );

    let counted = test_fn_names(&mask_non_code(&raw));
    assert!(
        !counted.iter().any(|t| t.contains(fixture)),
        "문자열 리터럴 안의 픽스처가 실제 테스트로 세어졌다 — 마스킹이 듣지 않는다: \
         {:?}",
        counted
            .iter()
            .filter(|t| t.contains(fixture))
            .collect::<Vec<_>>()
    );
    // 비영 대조: 같은 산출물에서 이 파일의 진짜 테스트들은 세어져야 한다.
    assert!(
        counted.len() >= 4,
        "이 파일의 진짜 `#[test]` 를 {} 개밖에 못 셌다 — 마스킹이 과하게 지웠다",
        counted.len()
    );
}
