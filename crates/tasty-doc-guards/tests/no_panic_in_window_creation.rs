//! 창·모달 생성 경로의 panic/expect/unwrap을 찾는다.
//! 새 창을 열지 못해도 이미 열린 창의 세션은 유지해야 한다(ADR-0016).
//! 실제 창 생성 실패를 실행하는 검사가 아니라 지정한 소스의 형태를 검사한다.

use std::path::PathBuf;

use tasty_doc_guards::cfg_predicate::cfg_gated_lines;

/// event_handler.rs의 부팅 GPU 초기화는 제외한다.
/// 어댑터 부재 외의 예상하지 못한 실패는 크래시 보고를 유지하기로 한 예외다.
const SCANNED: &[&str] = &[
    "src/app/window_lifecycle.rs",
    "src/app/modal/settings.rs",
    "src/app/modal/plugins.rs",
    "src/app/modal/quit.rs",
];

/// 사용자 입력·환경 오류가 아닌 코드 불변식의 단언만 허용한다.
const INVARIANT_ALLOWLIST: &[&str] = &[
    "core_state must be initialized before layout restore",
    "App.core_state must be present to register a main window",
];

fn read(rel: &str) -> String {
    let p: PathBuf = tasty_doc_guards::repo_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// 첫 // 뒤를 잘라낸다. 문자열 안의 //를 구별하지 못하는 단순 판정이다.
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
        // 공용 cfg 판정으로 테스트 아이템을 제외한다.
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
         세션을 살려야 한다(ADR-0016). 실패를 `Result` 로 돌리고 \
         `notify_window_creation_failed` 로 알려라. 정말 불변식이라면 \
         `INVARIANT_ALLOWLIST` 에 근거와 함께 등록한다.\n{}",
        hits.join("\n")
    );
}

#[test]
fn the_scanned_files_all_exist() {
    for rel in SCANNED {
        let p = tasty_doc_guards::repo_root().join(rel);
        assert!(p.is_file(), "스캔 대상이 사라졌다: {rel}");
    }
}

/// 허용 문구가 남았는지만 확인한다. 해당 예외가 여전히 필요한지는 별도 검토해야 한다.
#[test]
fn the_allowlist_entries_still_appear_in_the_sources() {
    let all: String = SCANNED.iter().map(|r| read(r)).collect();
    for entry in INVARIANT_ALLOWLIST {
        assert!(
            all.contains(entry),
            "allowlist 항목이 더 이상 소스에 없다 — 제거하라: {entry}"
        );
    }
}
