//! 루트 tests의 락 획득 뒤에 바로 붙은 unwrap을 찾아 등록한 예외 외에는 거절한다.
//! MutexGuard를 가진 채 panic하면 락이 poison되어 다른 테스트의 unwrap도 실패할 수 있다.
//! 특히 panic 인자에서 임시 가드를 만들면 복원이 끝나기 전에 패닉할 수 있다.
//! 필요한 값을 복사하고 가드를 먼저 해제해 실패가 다른 시험으로 전파되지 않게 한다.
//!
//! lock/read/write 표기를 한 줄씩 검사하며, 타입이나 모든 호출 형식을 해석하지는 않는다.
//! crates 하위 테스트는 이 검사에 포함하지 않는다. poison 복구 중 보고 누락은 별도 poison_recovery 검사다.
//! 락 선언 위치를 제한하지 않으며 근거는 docs/dev-guide/unit-test-isolation.md에 있다.
//!
//! 획득 수에는 하한을, 예외 목록에는 예산을 두고 쓰이지 않는 예외도 찾는다.
//! 통과할 때의 측정 수는 다음 명령으로 볼 수 있다.
//! ```text
//! cargo test -p tasty-doc-guards --test test_harness_lock_unwrap_ratchet -- --nocapture
//! ```

use std::path::PathBuf;
use tasty_doc_guards::repo_root;
use tasty_doc_guards::source_text::{mask_non_code, rust_sources};

const SCAN_ROOTS: &[&str] = &["tests"];

const ACQUIRE: &[&str] = &[".lock()", ".read()", ".write()"];

/// (상대 경로, 해당 줄의 코드 조각, 사유). 다른 줄까지 면제하지 않도록 정확한 파일과 조각을 등록한다.
/// 락을 가진 채 단정하지 않는 경우의 예외이며, 다른 곳에서 poison되면 이 unwrap도 실패한다.
/// 항목은 스캔 범위 안에 있어야 하며 실제 일치하는 줄이 없으면 제거한다.
const EXEMPT: &[(&str, &str, &str)] = &[(
    "tests/shared_instance_harness.rs",
    "let mut observed = OBSERVED.lock().unwrap();",
    "관찰 기록을 추가하고 락을 해제한 뒤 단정한다. 락을 가진 채 단정 실패로 panic하지 않는다.",
)];

/// 예외가 늘 때 검사 범위가 줄어드는 결정을 함께 검토하도록 별도 예산을 둔다.
const EXEMPT_BUDGET: usize = 1;

/// 2026-09-08 하네스 정리 뒤 46파일에서 획득11곳을 측정하고 하한8로 정했다.
/// 정상적인 감소를 허용하면서 수집·인식 실패를 찾기 위한 값이며 정확한 개수를 고정하지 않는다.
const MIN_ACQUISITIONS: usize = 8;

/// 한 파일의 소스에서 (획득 자리, 생 `.unwrap()` 자리) 를 센다. 좌표는 1 기반 줄이다.
fn classify(masked: &str) -> (Vec<usize>, Vec<usize>) {
    let mut acquired = Vec::new();
    let mut bare = Vec::new();
    for (i, line) in masked.lines().enumerate() {
        for tok in ACQUIRE {
            if let Some(pos) = line.find(tok) {
                acquired.push(i + 1);
                let rest = &line[pos + tok.len()..];
                if rest.trim_start().starts_with(".unwrap()") {
                    bare.push(i + 1);
                }
                break;
            }
        }
    }
    (acquired, bare)
}

pub struct Scan {
    /// 락 획득을 인식한 수.
    pub acquisitions: usize,
    /// 예외에 해당하지 않는 unwrap.
    pub sites: Vec<String>,
    /// 예외 항목별 일치 수.
    pub exempt_hits: Vec<usize>,
}

/// 합성 소스와 예외 목록으로도 같은 수집·예외 적용을 확인할 수 있게 분리한다.
fn scan(sources: &[(PathBuf, String)], exempt: &[(&str, &str, &str)]) -> Scan {
    let mut acquisitions = 0usize;
    let mut sites: Vec<String> = Vec::new();
    let mut exempt_hits = vec![0usize; exempt.len()];

    for (rel, raw) in sources {
        let rel_s = rel.display().to_string();
        let masked = mask_non_code(raw);
        let lines: Vec<&str> = masked.lines().collect();
        let (acq, bare) = classify(&masked);
        acquisitions += acq.len();
        for ln in bare {
            let text = lines.get(ln - 1).map(|s| s.trim()).unwrap_or("");
            let mut exempted = false;
            for (i, (path, needle, _why)) in exempt.iter().enumerate() {
                if &rel_s == path && text.contains(needle) {
                    exempt_hits[i] += 1;
                    exempted = true;
                }
            }
            if !exempted {
                sites.push(format!("{rel_s}:{ln}: {text}"));
            }
        }
    }
    Scan {
        acquisitions,
        sites,
        exempt_hits,
    }
}

#[test]
fn a_bare_unwrap_on_a_shared_lock_is_caught() {
    let src = "fn probe() {\n    let g = SHARED.lock().unwrap();\n}\n";
    let (acq, bare) = classify(&mask_non_code(src));
    assert_eq!(acq, vec![2], "합성 코드의 락 획득을 찾지 못했다");
    assert_eq!(bare, vec![2], "획득 직후 unwrap을 찾지 못했다");
}

#[test]
fn the_fixed_shape_is_not_flagged() {
    let src = "fn probe() {\n    let g = SHARED.lock().unwrap_or_else(|e| e.into_inner());\n}\n";
    let (acq, bare) = classify(&mask_non_code(src));
    assert_eq!(
        acq,
        vec![2],
        "복구 처리한 호출도 락 획득 수에 포함해야 한다"
    );
    assert!(
        bare.is_empty(),
        "poison 복구 형태를 직접 unwrap으로 오인했다"
    );
}

#[test]
fn a_mention_in_a_comment_is_not_a_site() {
    let src = "fn probe() {\n    // let g = SHARED.lock().unwrap();\n}\n";
    let (acq, bare) = classify(&mask_non_code(src));
    assert!(
        acq.is_empty() && bare.is_empty(),
        "주석 안의 언급은 자리가 아니다"
    );
}

#[test]
fn test_harness_locks_do_not_unwrap_bare() {
    let root = repo_root();
    let sources = rust_sources(&root, SCAN_ROOTS);

    let Scan {
        acquisitions,
        sites,
        exempt_hits,
    } = scan(&sources, EXEMPT);

    // 실패해도 수집·분류 결과를 볼 수 있도록 단언 전에 출력한다.
    eprintln!(
        "[harness-lock] 파일 {} · 획득 {} · 면제 {}/{} (예산 {}) · 생 unwrap {}",
        sources.len(),
        acquisitions,
        exempt_hits.iter().filter(|n| **n > 0).count(),
        EXEMPT.len(),
        EXEMPT_BUDGET,
        sites.len()
    );

    assert!(
        !sources.is_empty(),
        "{:?} 아래에서 Rust 파일을 찾지 못했다. 스캔 경로와 실제 파일을 확인한다.",
        SCAN_ROOTS
    );
    assert!(
        acquisitions >= MIN_ACQUISITIONS,
        "락 획득을 {acquisitions}곳만 찾았다(하한 {MIN_ACQUISITIONS}). lock/read/write 표기와 마스킹·순회를 확인한다. 관련 합성 시험과 실제 코드 감소를 대조하고 하한을 바꾸면 측정 근거도 남긴다."
    );

    let dead: Vec<String> = EXEMPT
        .iter()
        .zip(&exempt_hits)
        .filter(|(_, n)| **n == 0)
        .map(|((path, needle, _), _)| format!("{path} :: {needle}"))
        .collect();
    assert!(
        dead.is_empty(),
        "일치하는 코드가 없는 예외다: {dead:?}. 항목은 스캔 범위 안에 있어야 한다. 이동·삭제를 확인하고 불필요한 항목과 예산을 함께 줄인다."
    );
    assert!(
        EXEMPT.len() <= EXEMPT_BUDGET,
        "예외{}개가 예산{EXEMPT_BUDGET}을 넘었다. 실제 예외가 필요한지 검토하고 근거를 기록한다. 의도한 추가라면 같은 커밋에서 예산도 갱신한다.",
        EXEMPT.len()
    );

    assert!(
        sites.is_empty(),
        "락 획득 직후 unwrap이 {}곳 있다. poison된 락을 만나면 다른 테스트의 실패가 전파될 수 있다. 필요한 값을 복사한 뒤 가드를 해제하고 단정한다. poison 복구를 쓸 때는 보호값을 계속 써도 되는 이유를 적는다. 같은 문장에서 같은 Mutex를 다시 잡아 교착시키지 않는다:\n{}",
        sites.len(),
        sites
            .iter()
            .map(|s| format!("  {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// 예외의 일치 범위가 넓어져도 획득 수·예외 개수 하한으로는 찾지 못한다.
/// 서로 다른 두 파일에 같은 줄을 넣어 등록한 경로에서만 면제되는지 확인한다.
#[test]
fn the_exempt_routing_answers_on_a_substituted_corpus() {
    let shared_line = "    let zeta = SHARED.lock().unwrap();";
    let corpus = vec![
        (
            PathBuf::from("zone/alpha.rs"),
            "fn a() {\n    let g = SHARED.lock().unwrap();\n}\n".to_string(),
        ),
        (
            PathBuf::from("zone/beta.rs"),
            format!("fn b() {{\n{shared_line}\n    let h = OTHER.write().unwrap();\n}}\n"),
        ),
        (
            PathBuf::from("zone/gamma.rs"),
            format!("fn c() {{\n{shared_line}\n}}\n"),
        ),
        (
            PathBuf::from("zone/delta.rs"),
            "fn d() {\n    let g = SHARED.lock().unwrap_or_else(|p| p.into_inner());\n}\n"
                .to_string(),
        ),
    ];
    let exempt: &[(&str, &str, &str)] = &[(
        "zone/beta.rs",
        "let zeta = SHARED.lock().unwrap();",
        "합성 사유",
    )];

    let got = scan(&corpus, exempt);

    assert_eq!(got.acquisitions, 5, "합성 코드의 락 획득 수가 다르다");
    assert_eq!(got.exempt_hits, vec![1], "면제 적중 수가 다르다");

    let mut sites = got.sites.clone();
    sites.sort();
    assert_eq!(
        sites,
        vec![
            "zone/alpha.rs:2: let g = SHARED.lock().unwrap();".to_string(),
            "zone/beta.rs:3: let h = OTHER.write().unwrap();".to_string(),
            "zone/gamma.rs:2: let zeta = SHARED.lock().unwrap();".to_string(),
        ],
        "면제 라우팅이 합성 코퍼스에서 다른 답을 냈다"
    );
    assert!(
        sites.iter().any(|s| s.starts_with("zone/gamma.rs")),
        "등록하지 않은 다른 경로의 같은 줄까지 면제했다"
    );
    assert!(
        sites.iter().any(|s| s.contains("beta.rs:3")),
        "예외 파일의 다른 줄까지 면제했다. 경로와 코드 조각을 함께 비교해야 한다."
    );
    assert!(
        !sites.iter().any(|s| s.contains("delta.rs")),
        "고쳐진 모양(`unwrap_or_else`)을 생 `.unwrap()` 으로 셌다"
    );
}
