//! 창·모달 생성 경로에 패닉이 다시 들어오는 것을 막는 가드.
//!
//! 배경: 창을 여는 모든 경로가 실패를 `expect` 로 처리하던 시절, 사용자 `config.toml`
//! 의 셸 경로 오타 하나가 **실행 중인 모든 창의 세션**을 함께 날렸다. 이미 터미널이 떠
//! 있는 상태에서 패닉하기 때문이다. 그 결함은 실패를 `Result` 로 돌려 창만 취소하도록
//! 고쳤다(`docs/adr/0117-window-and-modal-creation-failure-policy.md`).
//!
//! 그런데 "고쳤다" 와 "고쳐진 채로 유지된다" 는 다른 문제다. 이 경로들은 winit
//! `ActiveEventLoop` 와 GPU 상태가 있어야 돌아가 단위 테스트로 감쌀 수 없고, 실제로
//! `create_new_window` 의 실패 분기를 `panic!` 으로 되돌리는 변이를 넣어도 기존 테스트가
//! **하나도 깨지지 않는 것**을 확인했다. 그래서 행동 테스트 대신 소스 형태를 고정한다 —
//! 이 파일들에서 `panic!` / `.expect(` / `.unwrap()` 이 보이면 fail 한다.
//!
//! 선례: `crates/tasty-doc-guards/tests/no_todo_file_citation.rs` · `crates/tasty-doc-guards/tests/no_checkbox_in_docs.rs`(소스/문서 스캔
//! + allowlist 구조).

use std::path::{Path, PathBuf};

use tasty_doc_guards::cfg_predicate::cfg_gated_lines;

/// 스캔 대상 — 창·모달을 만드는 경로만. `event_handler.rs` 는 제외한다: 부팅 GPU
/// 초기화의 "어댑터 부재가 **아닌** 예상 밖 실패" 는 크래시 리포팅을 유지하는 것이
/// 의도된 결정이라 `panic!` 이 남아 있어야 한다.
const SCANNED: &[&str] = &[
    "src/app/window_lifecycle.rs",
    "src/app/modal/settings.rs",
    "src/app/modal/plugins.rs",
    "src/app/modal/quit.rs",
];

/// 창 생성 실패와 무관한 **불변식** 단언. 이 문자열들은 "창이 안 열렸다" 가 아니라
/// "코드 순서가 보장하는 상태가 깨졌다" 를 뜻하므로 패닉이 옳다. 새로 추가하려면
/// 그것이 정말 불변식인지(= 사용자 입력이나 환경으로 도달할 수 없는지) 먼저 따진다.
const INVARIANT_ALLOWLIST: &[&str] = &[
    "core_state must be initialized before layout restore",
    "App.core_state must be present to register a main window",
];

fn read(rel: &str) -> String {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// 줄에서 코드 부분만 남긴다 — 주석(`//`)에 적힌 설명이 오탐되지 않게.
/// 문자열 리터럴 안의 `//` 는 이 파일들에 없으므로 단순 절단으로 충분하다.
fn code_of(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

#[test]
fn window_and_modal_creation_paths_contain_no_panics() {
    let mut hits: Vec<String> = Vec::new();

    for rel in SCANNED {
        let src = read(rel);
        // `#[cfg(test)]` 로 게이트된 줄은 대상이 아니다(테스트 코드의 unwrap 은 관례). 게이트
        // 판정은 정본 `cfg_gated_lines` 에 위임한다 — `split("#[cfg(test)]")` 는 첫 리터럴에서
        // 자를 뿐이라 복합 cfg(`all(test, …)`)·여러 블록·블록 뒤의 실코드를 모두 놓쳤다.
        let lines: Vec<&str> = src.lines().collect();
        let gated = cfg_gated_lines(&lines, "test");
        for (i, line) in lines.iter().enumerate() {
            if gated[i] {
                continue;
            }
            let code = code_of(line);
            if INVARIANT_ALLOWLIST.iter().any(|a| code.contains(a)) {
                continue;
            }
            if code.contains("panic!(") || code.contains(".expect(") || code.contains(".unwrap()") {
                hits.push(format!("{rel}:{}: {}", i + 1, line.trim()));
            }
        }
    }

    assert!(
        hits.is_empty(),
        "창·모달 생성 경로에 패닉이 들어왔다. 창 생성 실패는 그 창만 취소하고 나머지 창의 \
         세션을 살려야 한다(ADR-0117). 실패를 `Result` 로 돌리고 \
         `notify_window_creation_failed` 로 알려라. 정말 불변식이라면 \
         `INVARIANT_ALLOWLIST` 에 근거와 함께 등록한다.\n{}",
        hits.join("\n")
    );
}

#[test]
fn the_scanned_files_all_exist() {
    // 파일이 이름을 바꾸면 위 테스트가 조용히 0 건을 스캔하며 통과한다 — 그 상태를 막는다.
    for rel in SCANNED {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
        assert!(p.is_file(), "스캔 대상이 사라졌다: {rel}");
    }
}

/// **이 초록이 뜻하는 것은 문구가 아직 소스에 있다는 것까지다.** 그 자리가 여전히
/// 위반인지는 안 본다 — 문구가 남아 있어도 주변이 바뀌어 더 이상 잡히지 않을 수 있다.
/// 그 경우까지 가르려면 항목을 빼고 돌려야 하고, 절차는
/// `docs/dev-guide/guard-population.md`.
#[test]
fn the_allowlist_entries_still_appear_in_the_sources() {
    // 쓰이지 않는 allowlist 항목은 다음 사람에게 "여기는 원래 패닉해도 된다" 는 잘못된
    // 신호를 준다.
    let all: String = SCANNED.iter().map(|r| read(r)).collect();
    for entry in INVARIANT_ALLOWLIST {
        assert!(
            all.contains(entry),
            "allowlist 항목이 더 이상 소스에 없다 — 제거하라: {entry}"
        );
    }
}
