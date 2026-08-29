//! 계층 가드 — 본체 GUI/런타임 코드(`src/`)가 `tasty-cli` 크레이트를 직접
//! 참조하면 fail 한다.
//!
//! 배경: `tasty-cli` 는 **바이너리의 진입 계층**(인자 파싱 + 그 파싱 결과로
//! 실행되는 커맨드 구현)이다. GUI 런타임·IPC 핸들러·앱 상태가 그 크레이트
//! 내부를 들여다보면 의존 방향이 뒤집힌다 — 런타임이 CLI 를 소비하는 형태가
//! 되어, CLI 쪽 타입 변경이 GUI 를 깨고 GUI 재사용 목적의 로직이 CLI 안에
//! 눌러앉는다. 재사용되는 코어(ssh / remote browse / stream 등)는 CLI 가
//! 아니라 양쪽이 함께 쓰는 별도 크레이트에 있어야 한다.
//!
//! 이 가드가 없으면 위반이 컴파일 에러로 잡히지 않는다. `src/adapters/cli.rs`
//! 가 `pub use tasty_cli::*;` 로 **와일드카드 재수출**을 하고 있어 본체 어디서든
//! `crate::cli::` 경로로 CLI 크레이트 전체에 닿기 때문이다. 경계를 만드는
//! 리팩터는 가드를 먼저 세워야 이행 중에 새 위반이 안 들어온다.
//!
//! **두 목록은 성격이 다르다 (합치지 말 것)**:
//! - [`ALLOWED_PATHS`] — **영구 허용**. 바이너리가 CLI 파서를 소유하는 정당한
//!   의존(진입점 / boot 경로 / 재수출 지점). 비울 대상이 아니다.
//! - [`BASELINE_FILES`] — **한시 허용**. 이행 중인 기존 위반의 스냅샷.
//!   **줄어들기만 해야 한다.** 실제 위반이 사라지면 목록에서도 지워야 통과한다
//!   (역방향 검사).
//!
//! 주석 안의 언급도 위반으로 본다 — 주석이 옛 경로를 가리키면 그것도 실제
//! 오정보이므로 코드와 같이 갱신되어야 한다.
//!
//! 선례: `tests/no_todo_file_citation.rs`(구조 템플릿) · `tests/no_emoji_in_source.rs`.

use std::path::{Path, PathBuf};

/// 금지 패턴 — 크레이트 경로 참조. `use` / 타입 위치 / 주석 어디에 있든 잡는다.
const FORBIDDEN: &str = "tasty_cli::";

/// **영구 허용** 경로(repo-relative). `/` 로 끝나면 접두사(디렉토리) 매칭.
///
/// - `src/main.rs`: 바이너리 진입점. CLI 파서를 소유하는 주체다.
/// - `src/boot.rs`, `src/boot/`: 프로세스 기동 경로. 파싱된 커맨드를 분기하고
///   종료 시 CLI 측 집계를 회수하는, 진입점의 연장선이다.
/// - `src/adapters/cli.rs`: 재수출 지점 그 자체. 참조가 여기 한 곳에 모이는 것이
///   목표 상태라, 이 파일은 비울 대상이 아니다.
const ALLOWED_PATHS: &[&str] = &[
    "src/main.rs",
    "src/boot.rs",
    "src/boot/",
    "src/adapters/cli.rs",
];

/// **한시 허용** — 이행 중인 위반의 스냅샷. 줄어들기만 한다.
///
/// **현재 비어 있다.** 재사용 코어는 전부 별도 크레이트로 분리됐다 —
/// ssh 위임은 `tasty-ssh`, 원격 조회/생성은 `tasty-remote`, 클라이언트 IPC
/// 연결은 `tasty_ipc::client`. 본체는 그쪽을 직접 참조한다.
///
/// **새 항목을 추가해서는 안 된다.** 여기에 이름을 적어 통과시키는 것은
/// 위반을 해소한 게 아니라 가드를 끄는 것이다.
const BASELINE_FILES: &[&str] = &[];

/// 순회에서 통째로 가지치기할 디렉토리명. 빌드 산출물·VCS 가 `src/` 안에
/// 섞여 들어와도 스캔이 새지 않게 한다.
const PRUNE_DIRS: &[&str] = &["target", ".git"];

fn is_allowed(rel: &str) -> bool {
    ALLOWED_PATHS.iter().any(|p| {
        if let Some(dir) = p.strip_suffix('/') {
            rel.starts_with(dir) && rel.as_bytes().get(dir.len()) == Some(&b'/')
        } else {
            rel == *p
        }
    })
}

/// `path` 하위를 재귀 순회하며 스캔 대상(`.rs`)을 모은다.
fn gather(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if PRUNE_DIRS.contains(&name) {
                continue;
            }
        }
        gather(&p, out);
    }
}

fn rel_of(file: &Path, root: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn src_does_not_reference_tasty_cli() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    gather(&root.join("src"), &mut files);
    files.sort();

    // 두 목록이 겹치면 "베이스라인을 비운다" 가 성립하지 않는다 — 영구 허용
    // 항목은 실제 위반이 남아 있어도 역방향 검사에 걸리지 않기 때문이다.
    let overlap: Vec<&str> = BASELINE_FILES
        .iter()
        .copied()
        .filter(|f| is_allowed(f))
        .collect();
    assert!(
        overlap.is_empty(),
        "BASELINE_FILES 항목이 ALLOWED_PATHS 에도 걸린다 — 한시 허용과 영구 허용은 \
         분리되어야 한다(겹치면 그 항목은 영원히 비울 수 없다):\n  {}",
        overlap.join("\n  ")
    );

    let mut new_violations = Vec::new();
    let mut baseline_hit = Vec::new();
    for file in files {
        let rel = rel_of(&file, root);
        if is_allowed(&rel) {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&file) else {
            continue; // 비-UTF8 은 경로 참조를 담을 수 없다.
        };
        let mut hits = Vec::new();
        for (i, line) in contents.lines().enumerate() {
            if line.contains(FORBIDDEN) {
                hits.push(format!("  {}:{} — `{}`", rel, i + 1, line.trim()));
            }
        }
        if hits.is_empty() {
            continue;
        }
        if BASELINE_FILES.contains(&rel.as_str()) {
            baseline_hit.push(rel);
        } else {
            new_violations.extend(hits);
        }
    }

    assert!(
        new_violations.is_empty(),
        "본체(`src/`)가 `tasty-cli` 크레이트를 직접 참조한다 — 의존 방향이 뒤집힌다.\n\
         재사용이 필요한 코어(ssh / remote browse / stream 등)는 CLI 가 아니라 양쪽이 \
         함께 쓰는 별도 크레이트에 두고, 본체는 그쪽을 참조할 것.\n\
         BASELINE_FILES 는 이행 중인 기존 위반의 스냅샷이라 **줄어들기만 해야 한다** — \
         새 항목 추가 금지.\n{}",
        new_violations.join("\n")
    );

    // 역방향 — 베이스라인이 실제보다 넓으면 위반이 사라져도 가드가 느슨한 채로 남는다.
    let stale: Vec<&str> = BASELINE_FILES
        .iter()
        .copied()
        .filter(|f| !baseline_hit.iter().any(|h| h == f))
        .collect();
    assert!(
        stale.is_empty(),
        "BASELINE_FILES 에 있으나 실제 위반이 없다 — 참조를 걷어냈으면 목록에서도 지울 것 \
         (남겨두면 그 파일에 위반이 다시 들어와도 통과한다). 파일이 사라졌거나 이름이 \
         바뀐 경우도 같다:\n  {}",
        stale.join("\n  ")
    );
}
