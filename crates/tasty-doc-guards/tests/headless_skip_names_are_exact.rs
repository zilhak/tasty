//! 헤드리스 테스트의 각 --skip 문자열이 소스에서 정확히 하나의 테스트 이름과 일치하는지 확인한다.
//! 부분일치이므로 이름 변경은 누락을, 유사한 새 이름은 의도하지 않은 제외를 만들 수 있다.
//! 주석·문자열을 마스킹하고 #[test] 함수의 출현 수를 센다. 같은 이름도 파일별로 따로 센다.
//! Cargo의 실제 테스트 목록을 실행해 얻지 않으므로 cfg·매크로로 달라지는 빌드 결과까지 보장하지는 않는다.
//! workspace exclude 경로는 매니페스트에서 읽어 제외한다.
//! 모듈 이름과도 일치하면 전체 경로의 제외 수를 복원하지 못하므로 오류로 처리한다.

use tasty_doc_guards::source_text::mask_non_code;

use std::fs;
use std::path::{Path, PathBuf};

const WORKFLOW: &str = ".github/workflows/crossplatform-check.yml";
const STEP_ANCHOR: &str = "- name: cargo test (headless)";

/// 합성 문자열이 실제 테스트로 수집되지 않는지 자기 소스를 읽어 확인한다.
const SELF_PATH: &str = "crates/tasty-doc-guards/tests/headless_skip_names_are_exact.rs";

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

/// 워크플로의 헤드리스 스텝에서 skip 인자를 읽는다. 스텝을 못 찾거나 결과가 비면 별도로 확인한다.
fn skips_from_workflow() -> Vec<String> {
    let text = fs::read_to_string(repo_root().join(WORKFLOW))
        .unwrap_or_else(|e| panic!("{WORKFLOW} 를 읽을 수 없다: {e}"));
    let start = text
        .find(STEP_ANCHOR)
        .unwrap_or_else(|| panic!("워크플로에서 `{STEP_ANCHOR}` 스텝을 못 찾았다 — 앵커가 깨졌다"));
    let rest = &text[start + STEP_ANCHOR.len()..];
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
        "{STEP_ANCHOR}에서 --skip을 읽지 못했다. 스텝 이름·인자 형식이 바뀌었는지 확인한다. 실제로 제외가 없어졌다면 ADR-0045의 격리·헤드리스 실행 기준을 재검토하고 이 검사를 갱신한다."
    );
    out
}

/// 저장소 Rust 파일 중 workspace exclude를 제외한다. 실제 Cargo 모듈 그래프를 복원하는 것은 아니다.
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
        "workspace exclude를 읽지 못했다. 제외 목록과 파서를 확인해 비대상 파일이 검사에 섞이지 않게 한다."
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
                let rel = tasty_doc_guards::floored_walk::normalized_rel(&p, root);
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

/// 동명 테스트가 여러 파일에 있으면 각각 제외되므로 이름 집합으로 합치지 않는다.
fn test_fn_names(masked: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = masked;
    while let Some(i) = cursor.find("#[test]") {
        let after = &cursor[i + "#[test]".len()..];
        // 다음 test 속성 전에 나오는 fn만 현재 속성의 테스트로 읽는다.
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

/// skip과 겹치는 모듈 이름을 찾기 위한 목록. 테스트 수에는 합치지 않는다.
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

/// 워크플로가 아닌 소스 파일의 이름 때문에 충돌할 수도 있어 이름의 출처 경로를 함께 보관한다.
struct Named {
    /// repo-relative 경로.
    rel: String,
    name: String,
}

impl Named {
    fn at(&self) -> String {
        format!("{}::{}", self.rel, self.name)
    }
}

fn named(root: &Path, path: &Path, names: Vec<String>) -> Vec<Named> {
    let rel = tasty_doc_guards::floored_walk::normalized_rel(path, root);
    names
        .into_iter()
        .map(|name| Named {
            rel: rel.clone(),
            name,
        })
        .collect()
}

fn at_list(hits: &[&Named]) -> String {
    hits.iter()
        .map(|h| format!("  {}", h.at()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 미추적 폴더도 수집될 수 있어 오류 위치의 추적 여부를 함께 알린다.
fn outside_note(root: &Path, hits: &[&Named]) -> String {
    let rels: Vec<String> = hits.iter().map(|h| h.rel.clone()).collect();
    tasty_doc_guards::tracked_scope::outside_repo_note(root, &rels)
}

#[test]
fn every_named_skip_matches_exactly_one_test() {
    let root = repo_root();
    let sources = workspace_sources();
    let tests: Vec<Named> = sources
        .iter()
        .flat_map(|(p, m)| named(&root, p, test_fn_names(m)))
        .collect();
    let modules: Vec<Named> = sources
        .iter()
        .flat_map(|(p, m)| named(&root, p, module_names(m)))
        .collect();

    assert!(
        tests.len() > 1000,
        "소스에서 #[test] 함수를 {}개만 읽었다. 파일 수집과 함수 판독을 확인한다.",
        tests.len()
    );
    assert!(
        modules.len() > 100,
        "`mod` 선언을 {} 개밖에 못 찾았다 — 모듈 쪽 상한 단정이 무의미해진다",
        modules.len()
    );

    for skip in skips_from_workflow() {
        // 모듈명과 겹치면 그 아래 테스트 수를 셀 수 없으므로 이름 검사 전에 거부한다.
        let module_hits: Vec<&Named> = modules.iter().filter(|m| m.name.contains(&skip)).collect();
        assert!(
            module_hits.is_empty(),
            "--skip {skip}이 모듈 이름과도 일치한다. 전체 테스트 경로를 복원하지 않아 제외 개수를 셀 수 없다. 모듈명과 겹치지 않도록 skip을 정한다.\n{}{}",
            at_list(&module_hits),
            outside_note(&root, &module_hits)
        );

        let hits: Vec<&Named> = tests.iter().filter(|t| t.name.contains(&skip)).collect();
        assert!(
            !hits.is_empty(),
            "--skip {skip}과 일치하는 테스트 이름이 없다(수집한 함수 {}개). 이름 변경·삭제·판독 실패를 확인하고 워크플로를 갱신한다.",
            tests.len()
        );
        assert_eq!(
            hits.len(),
            1,
            "--skip {skip}이 테스트 {}개와 일치한다. 더 구체적인 이름을 쓰거나 여러 테스트를 제외해야 한다면 먼저 단일 테스트 규칙을 재검토한다.\n{}{}",
            hits.len(),
            at_list(&hits),
            outside_note(&root, &hits)
        );
    }
}

#[test]
fn the_parser_reads_the_workflow_rather_than_a_hardcoded_list() {
    // 워크플로의 skip 이름을 이 검사 소스에 복제하지 않았는지 확인한다.
    let skips = skips_from_workflow();
    assert!(!skips.is_empty());
    let own_source =
        fs::read_to_string(repo_root().join(SELF_PATH)).expect("자기 소스를 읽을 수 있어야 한다");
    for s in &skips {
        assert!(
            !own_source.contains(s.as_str()),
            "skip 이름 `{s}` 이 이 가드 소스에 박혀 있다 — 워크플로에서 읽는 의미가 없어진다"
        );
    }
}

/// 동명 테스트와 비테스트 함수를 함께 넣어 단순 이름 집계가 우연히 같은 수를 내는 경우를 구별한다.
#[test]
fn counting_identifiers_is_not_counting_tests() {
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

    // 비교용으로 일반 함수 이름을 집합으로 모은다.
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
    assert!(
        ident_set.contains("sweeps_the_thing_impl"),
        "첫 판은 제품 함수를 센다 — 이게 과대 방향이다: {ident_set:?}"
    );
}

/// assets를 이름으로 제외하므로 그 안에 Rust 파일이 추가되면 범위를 다시 검토해야 한다.
/// 현재 제외 대상이 없는 상태만 확인하며 Cargo의 실제 컴파일 범위를 판독하지는 않는다.
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

    // 같은 순회가 src의 파일은 찾는지 확인해 빈 결과와 수집 실패를 구별한다.
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
    let raw =
        fs::read_to_string(repo_root().join(SELF_PATH)).expect("자기 소스를 읽을 수 있어야 한다");
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
    assert!(
        counted.len() >= 4,
        "이 파일의 진짜 `#[test]` 를 {} 개밖에 못 셌다 — 마스킹이 과하게 지웠다",
        counted.len()
    );
}

/// 헤드리스 skip 개수가 늘면 격리·GUI 요구 조건을 다시 검토한다(ADR-0045).
/// 0개는 skips_from_workflow가 먼저 거부하므로 이 검사는 증가만 확인한다.
/// 이 조건만 검증하려면 새 이름 대신 기존 skip을 중복해 개수만 바꾼다.
#[test]
fn the_named_skip_count_is_still_one() {
    let skips = skips_from_workflow();
    assert_eq!(
        skips.len(),
        1,
        "헤드리스 --skip이 {}건이다: {:?}. ADR-0045의 실행·격리 기준을 재검토한다. GUI가 필요한 테스트인지 헤드리스 구성 누락인지 구별하고, 구성 누락은 skip으로 숨기지 말고 고친다. 제외를 늘리기로 결정한 경우에만 기준값과 CLAUDE.md·ci-gates.md의 관련 설명을 함께 갱신한다.",
        skips.len(),
        skips
    );
}
