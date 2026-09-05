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
//! **덮는 채널이 있으면 사각이 아니다.** 읽는 것이 전부 무시 대상이어도, 경로 필터 없는
//! 다른 워크플로가 그 타깃을 `--test <이름>` 으로 부르면 그 push 에서 돈다.
//! [`targets_covered_by_unfiltered_workflows`] 가 그 명부를 워크플로 파일에서 읽는다 —
//! 손으로 든 명부는 낡는 순간 거짓 양성(이미 덮인 것을 옮기라고 한다)이 된다.
//! 그래서 **여기서 옮기라는 요구가 나오면 그것은 옮길 자리다**: 그 타깃을 이름으로 부르는
//! 필터 없는 잡이 하나도 없다는 뜻이다.
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
/// 실측 34 (2026-09-05) — 술어를 넓힌 뒤 값이다. 여유를 4 만 둔다: 하한이 실제보다
/// 한참 낮으면 술어가 절반 죽어도 통과한다.
const MIN_SCANNED: usize = 30;

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

/// 경로 필터 **없이** push 마다 도는 워크플로가 `--test <이름>` 으로 지목하는 타깃.
///
/// 이런 타깃은 읽는 것이 전부 무시 대상이어도 사각이 아니다 — `crossplatform-check` 가
/// 안 떠도 그 워크플로가 뜬다. `changelog_unreleased` 가 그 형태이고, ADR-0138 이 그
/// 이유로 옮기지 않기로 한 자리다. 명부를 손으로 들지 않는 이유는 명부가 낡으면
/// 그 순간 거짓 양성(옮기라는 요구)이나 거짓 음성(덮인 줄 아는 사각)이 되기 때문이다 —
/// 워크플로 파일이 답을 갖고 있으므로 거기서 읽는다.
///
/// **`workflow_dispatch` 전용 잡은 세지 않는다.** 사람이 눌러야만 도는 것은 채널이
/// 아니다. 잡 경계는 두 칸 들여쓴 `<이름>:` 으로 가른다.
fn targets_covered_by_unfiltered_workflows(root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let dir = root.join(".github/workflows");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        panic!("read {}: 워크플로 디렉토리를 못 읽었다", dir.display());
    };
    for e in entries.flatten() {
        let p = e.path();
        if !p.extension().is_some_and(|x| x == "yml" || x == "yaml") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        // push 트리거가 있고 경로 필터가 없어야 "필터 없는 채널" 이다.
        if !text.contains("push:") || text.contains("paths-ignore:") || text.contains("paths:") {
            continue;
        }
        let mut dispatch_only = false;
        for line in text.lines() {
            let t = line.trim_start();
            let indent = line.len() - t.len();
            // 잡 헤더에서 상태를 리셋한다.
            if indent == 2 && t.ends_with(':') && !t.starts_with('-') && !t.starts_with('#') {
                dispatch_only = false;
            }
            if t.starts_with("if:") && t.contains("workflow_dispatch") {
                dispatch_only = true;
            }
            if dispatch_only {
                continue;
            }
            if let Some(rest) = t.strip_prefix("--test ") {
                out.insert(rest.trim().to_string());
            }
        }
    }
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

fn path_literals(root: &Path, src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (i, part) in src.split('"').enumerate() {
        if i % 2 == 0 || part.contains('\n') {
            continue;
        }
        if TOP_DIRS.iter().any(|d| part.starts_with(d)) {
            out.insert(part.to_string());
            continue;
        }
        // 레포 **최상위** 파일은 앞에 디렉토리가 없어 위 판정에 안 걸린다 — `CHANGELOG.md`
        // 가 그 형태이고 `**/*.md` 무시 대상이다. 아직 실현된 사각은 아니다(지금 그 가드는
        // `crates/…/CHANGELOG.md` 도 함께 읽어 위 판정에 걸린다). 다만 루트 파일 **하나만**
        // 읽는 가드가 생기면 그때는 조용히 통과한다 — 변이로 확인했다.
        // 픽스처 이름("charlie.md")과 가르는 것은 **레포에 실재하는가** 하나다.
        if !part.contains('/') && part.contains('.') && root.join(part).is_file() {
            out.insert(part.to_string());
        }
    }
    out
}

/// 레포 파일을 런타임에 읽고 프로세스는 안 띄우는 통합 타깃 — `ci-gates.md` 의 인구조사와
/// 같은 판별식이다.
fn is_pure_source_scan(src: &str) -> bool {
    // 루트를 어떻게 얻는지로 세지 않는다 — `changelog_unreleased` 는 상대 경로로 읽어
    // 이 술어의 옛 형태(`CARGO_MANIFEST_DIR || repo_root()`)에서 **모수 자체에 없었다.**
    // 모수 밖은 위반 0 으로도 안 보이고 아예 안 보인다. 읽는 **행위**로 센다.
    let reads = src.contains("CARGO_MANIFEST_DIR")
        || src.contains("repo_root()")
        || src.contains("read_to_string")
        || src.contains("read_dir");
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
    let covered = targets_covered_by_unfiltered_workflows(&root);
    assert!(
        !covered.is_empty(),
        "경로 필터 없는 워크플로가 `--test` 로 지목하는 타깃을 하나도 못 읽었다 — \
         판독이 깨졌다. 그대로 두면 이미 덮인 가드를 '옮겨라' 로 잡는다"
    );

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
        let paths = path_literals(&root, &src);
        if paths.is_empty() {
            continue;
        }
        let ignored: Vec<&String> = paths.iter().filter(|p| is_ignored(p, &globs)).collect();
        if ignored.is_empty() {
            continue;
        }
        let stem = file.file_stem().map(|x| x.to_string_lossy().to_string());
        if stem.is_some_and(|n| covered.contains(&n)) {
            // 필터 없는 다른 워크플로가 이름으로 부른다 — 이 필터 뒤에 있어도 사각이 아니다.
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
