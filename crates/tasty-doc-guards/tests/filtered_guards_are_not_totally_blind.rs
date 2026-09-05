//! `crossplatform-check.yml` 의 `paths-ignore` 뒤에 사는 스캔 가드 중, **읽는 경로가 전부
//! 무시 대상인 것**이 없는지 본다.
//!
//! ADR-0138 이 가른 것이 이 구분이다. 읽는 것이 전부 `docs/**` 인 가드는 **문서만 담은
//! push 가 위반의 유일한 경로**라, 그 push 에서 워크플로가 안 뜨면 그 가드는 자기가
//! 깨지는 그 순간에만 정확히 안 도는 형태가 된다(총체적 사각). 그런 가드 셋을
//! `crates/tasty-doc-guards/tests/`(경로 필터 없는 `doc-guards.yml`)로 옮긴 것이 그
//! 결정이고, 이 테스트는 **그 결정이 새 가드에도 계속 적용되는지**를 본다.
//!
//! 일부만 무시 대상인 것은 다르다 — 코드 쪽 위반이 여전히 잡히고 문서 쪽 위반도 다음
//! 소스 push 에서 잡힌다. 그래서 옮기지 않고 [`PARTIALLY_FILTERED`] 에 사유와 함께
//! 적어 둔다. 그 명부는 **양방향으로** 고정된다: 새로 생기면 실패하고, 사라졌는데
//! 명부에 남아 있어도 실패한다.
//!
//! **이 테스트가 답하지 못하는 것**: "그 창이 실제로 열렸나", 그리고 "0 이 깨졌나".
//! push 단위 노출은 레포
//! 안에서 셀 수 없다(어느 push 가 어떤 커밋을 묶었는지가 git 에 없다). 그 수는 필터
//! 없는 워크플로의 run 목록으로만 재고, 재는 법과 실측은
//! `docs/dev-guide/ci-gates.md` 의 인구조사 절에 있다. 여기서 보는 것은 **옮김을
//! 강제하는 조건** — 읽는 경로가 전부 무시 대상이 되는 순간 — 하나다.
//!
//! 그 경계가 중요하다. 아래 [`PARTIALLY_FILTERED`] 를 "옮기지 않아도 된다" 로 만든 근거는
//! **push 가 문서와 소스를 함께 묶어 왔다**는 관찰이고, 그것은 구조가 아니라 습관이다.
//! 습관이 바뀌면 아무도 안 알리고 그 근거가 깨지는데 **이 가드는 그것을 못 본다** — 어느
//! 커밋들이 한 push 였는지가 git 에 없기 때문이다. 그 축을 재는 법은 위 문서에 명령으로
//! 적어 뒀고, 자동 채널은 없다.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 필터 뒤에 남는 것이 승인된 가드와 그 사유. **일부만** 무시 대상인 것만 온다.
const PARTIALLY_FILTERED: &[(&str, &str)] = &[(
    "tests/cli_method_table_parity.rs",
    "입력의 대부분이 crates/tasty-cli/src/** 라 코드 push 마다 돈다. \
     문서 쪽(api-conventions.md) 위반도 다음 소스 push 에서 잡힌다.",
)];

/// 스캔 가드 모수의 하한. 스캔이 깨져 목록이 비면 "위반 0 건" 이 조용히 참이 된다.
const MIN_SCANNED: usize = 20;

const WORKFLOW: &str = ".github/workflows/crossplatform-check.yml";

/// 필터 없는 채널을 가진 자리 — 여기 사는 가드는 이 판정의 대상이 아니다.
const FILTER_FREE_DIR: &str = "crates/tasty-doc-guards/tests";

/// 워크플로의 `paths-ignore` 목록을 읽는다. 못 읽으면 **실패한다** — 목록을 못 읽은 채
/// "무시 대상이 없다" 로 진행하면 모든 가드가 통과로 분류된다.
fn ignore_globs(root: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(root.join(WORKFLOW))
        .unwrap_or_else(|e| panic!("read {WORKFLOW}: {e}"));
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("paths-ignore:") {
            inside = true;
            continue;
        }
        if inside {
            if let Some(rest) = t.strip_prefix("- ") {
                out.push(rest.trim_matches('\'').trim_matches('"').to_string());
            } else if !t.is_empty() {
                break;
            }
        }
    }
    assert!(
        !out.is_empty(),
        "{WORKFLOW} 에서 paths-ignore 를 읽지 못했다 — 형식이 바뀌었거나 필터가 사라졌다. \
         둘 다 이 테스트의 전제가 무너진 것이라 통과로 읽지 않는다"
    );
    out
}

fn is_ignored(path: &str, globs: &[String]) -> bool {
    globs.iter().any(|g| match g.strip_suffix("/**") {
        Some(prefix) => path.starts_with(prefix) && path[prefix.len()..].starts_with('/'),
        None => match g.strip_prefix("**/*") {
            Some(suffix) => path.ends_with(suffix),
            None => path == g,
        },
    })
}

/// 레포 상대 경로처럼 보이는 문자열 리터럴만 뽑는다. 최상위 디렉토리 이름으로 시작하는
/// 것만 세므로, 메시지 안의 산문이나 메서드 이름은 걸리지 않는다.
const TOP_DIRS: &[&str] = &[
    "docs/", "site/", "src/", "crates/", "scripts/", ".github/", "tests/", "assets/", "lang/",
];

fn path_literals(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (i, part) in src.split('"').enumerate() {
        if i % 2 == 0 || part.contains('\n') {
            continue;
        }
        if TOP_DIRS.iter().any(|d| part.starts_with(d)) {
            out.insert(part.to_string());
        }
    }
    out
}

/// 레포 파일을 런타임에 읽고 프로세스는 안 띄우는 통합 타깃 — `ci-gates.md` 의 인구조사와
/// 같은 판별식이다.
fn is_pure_source_scan(src: &str) -> bool {
    let reads = src.contains("CARGO_MANIFEST_DIR") || src.contains("repo_root()");
    let spawns = [
        "Command::new",
        "spawn_diag",
        "TASTY_E2E_BIN",
        "CARGO_BIN_EXE",
    ]
    .iter()
    .any(|m| src.contains(m));
    reads && !spawns
}

fn integration_targets(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut dirs = vec![root.join("tests")];
    if let Ok(entries) = std::fs::read_dir(root.join("crates")) {
        for e in entries.flatten() {
            dirs.push(e.path().join("tests"));
        }
    }
    for d in dirs {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn rel(p: &Path, root: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn no_filtered_scan_guard_reads_only_ignored_paths() {
    let root = tasty_doc_guards::repo_root();
    let globs = ignore_globs(&root);

    let mut scanned = 0usize;
    let mut blind: Vec<String> = Vec::new();
    let mut partial: BTreeSet<String> = BTreeSet::new();

    for file in integration_targets(&root) {
        let r = rel(&file, &root);
        if r.starts_with(FILTER_FREE_DIR) {
            continue;
        }
        let src = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("read {r}: {e}"));
        if !is_pure_source_scan(&src) {
            continue;
        }
        scanned += 1;
        let paths = path_literals(&src);
        if paths.is_empty() {
            continue;
        }
        let ignored: Vec<&String> = paths.iter().filter(|p| is_ignored(p, &globs)).collect();
        if ignored.is_empty() {
            continue;
        }
        if ignored.len() == paths.len() {
            blind.push(format!(
                "  {r} — 읽는 경로가 전부 무시 대상이다: {}",
                ignored
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        } else {
            partial.insert(r);
        }
    }

    assert!(
        scanned >= MIN_SCANNED,
        "필터 뒤 순수 스캔 가드를 {scanned} 개밖에 못 셌다 — 스캔이 깨졌다. \
         모수가 줄면 '위반 0 건' 은 언제나 참이다"
    );

    assert!(
        blind.is_empty(),
        "아래 가드는 읽는 경로가 **전부** `{WORKFLOW}` 의 paths-ignore 안이라, 자기가 \
         깨질 수 있는 유일한 종류의 push(문서만 담은 push)에서 워크플로가 뜨지 않는다. \
         `{FILTER_FREE_DIR}` 로 옮겨라 — 그 자리의 doc-guards.yml 은 경로 필터가 없다 \
         (ADR-0138). 크레이트 상수를 링크해야 해서 못 옮기겠으면, 그 상수를 소스 텍스트로 \
         읽는 길이 이미 있다(`tasty_doc_guards::method_table` 와 그 판독을 런타임 열거와 \
         대조하는 `tests/method_table_readings_agree.rs`):\n{}",
        blind.join("\n")
    );

    let declared: BTreeSet<String> = PARTIALLY_FILTERED
        .iter()
        .map(|(p, _)| (*p).to_string())
        .collect();
    let added: Vec<&String> = partial.difference(&declared).collect();
    let stale: Vec<&String> = declared.difference(&partial).collect();
    assert!(
        added.is_empty(),
        "입력의 일부가 무시 대상인 가드가 새로 생겼다. 옮길지 남길지는 판단이 필요하다 — \
         남기기로 했으면 `PARTIALLY_FILTERED` 에 **사유와 함께** 등재해라:\n  {}",
        added
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    assert!(
        stale.is_empty(),
        "`PARTIALLY_FILTERED` 에 있는데 실제로는 그 형태가 아니다 — 옮겼거나 입력이 \
         바뀌었으면 명부에서 지워라. 명부가 실제보다 넓으면 다음에 진짜가 생겨도 \
         이미 등재된 것으로 읽힌다:\n  {}",
        stale
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}
