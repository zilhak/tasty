//! webhook과 hook handler의 출하 코드가 IPC 수신부·CLI 진입부·메인 루프를 직접 참조하는지 검사한다.
//! 내부 IPC 호출은 tasty_ipc::host_call::HostIpcInjector로 주입해야 한다(ADR-0002).
//! 같은 크레이트 안의 경계이므로 컴파일러가 의존 방향을 제한하지 않는다.
//! 공유 crate_paths 파서로 경로를 읽고 파일·인라인 test 전용 코드를 제외한다.
//! 전이 의존과 outbound adapter 사용은 검사하지 않는다. 허용 목록은 두지 않는다.

use tasty_doc_guards::crate_paths::{path_is_under, shipped_references};
use tasty_doc_guards::repo_root;
use tasty_doc_guards::shipping_scope::test_only_files;
use tasty_doc_guards::source_text::rust_sources;

const RUNNER_ROOTS: &[&str] = &["src/webhook", "src/hook_handler"];

/// 두 실행부를 모두 수집했는지 확인할 파일.
const RUNNER_ANCHORS: &[&str] = &["src/webhook/mod.rs", "src/hook_handler/mod.rs"];

/// 수집 누락을 찾는 하한. 2026-09-22 실측 18개(webhook10·hook_handler8).
const MIN_RUNNER_FILES: usize = 14;

/// 금지할 루트 이름과 사유. src/lib.rs의 재노출 별칭도 포함한다.
const INBOUND: &[(&str, &str)] = &[
    (
        "adapters::ipc",
        "IPC 요청 핸들러 트리 — 내부 호출은 `tasty_ipc::host_call::HostIpcInjector` 로 주입한다",
    ),
    ("ipc", "`adapters::ipc` 의 lib 루트 별칭"),
    (
        "adapters::production::tcp_ipc_server",
        "IPC 서버 구현 — 실행부는 서버가 아니라 주입기를 받는다",
    ),
    ("adapters::cli", "CLI 진입 계층"),
    ("cli", "`adapters::cli` 의 lib 루트 별칭"),
    ("hub", "IPC 서버 조립"),
    (
        "app",
        "메인 루프 — 실행부의 요청은 주입기를 거쳐서만 메인 루프에 닿는다",
    ),
    ("App", "`app::App` 의 lib 루트 별칭"),
    ("AppEvent", "`app::event::AppEvent` 의 lib 루트 별칭"),
    ("debug_info", "`app::debug_info` 의 lib 루트 별칭"),
];

fn inbound_match(path: &[String]) -> Option<&'static str> {
    INBOUND
        .iter()
        .map(|(n, _)| *n)
        .find(|n| path_is_under(n, path))
}

#[test]
fn automation_runners_do_not_name_an_inbound_adapter() {
    let root = repo_root();
    let sources = rust_sources(&root, RUNNER_ROOTS);
    let rels: Vec<String> = sources
        .iter()
        .map(|(rel, _)| rel.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        sources.len() >= MIN_RUNNER_FILES,
        "자동화 실행부({})에서 .rs 파일을 {}개만 수집했다(하한 {MIN_RUNNER_FILES}). 수집 범위를 확인하고, 실행부가 이동했다면 RUNNER_ROOTS를 갱신한다.",
        RUNNER_ROOTS.join(" · "),
        sources.len()
    );
    for anchor in RUNNER_ANCHORS {
        assert!(
            rels.iter().any(|r| r == anchor),
            "순회가 `{anchor}` 에 닿지 않았다 — 실행부 모듈의 루트라 실재가 보장된다."
        );
    }
    let not_shipped = test_only_files(&root, &sources);
    let mut shipped = 0;
    let mut offenders = Vec::new();
    for ((rel, text), rel_s) in sources.iter().zip(&rels) {
        if not_shipped.contains(rel) {
            continue;
        }
        shipped += 1;
        for (line, name, raw) in shipped_references(rel_s, text, inbound_match) {
            let why = INBOUND
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, w)| *w)
                .unwrap_or("");
            offenders.push(format!("  {rel_s}:{line} — `{name}` ({why}): {raw}"));
        }
    }
    println!(
        "[자동화 실행부 경계] 출하 파일 {shipped} 개 · inbound adapter 참조 {} 자리",
        offenders.len()
    );
    assert!(
        offenders.is_empty(),
        "자동화 실행부가 inbound adapter나 메인 루프를 직접 참조한다:\n{}\n내부 호출은 tasty_ipc::host_call로 주입한다(ADR-0002). 필요한 타입은 공용 계약으로 옮기고 핸들러가 사용하게 한다. 허용 목록을 추가해 통과시키지 않는다.",
        offenders.join("\n")
    );
}

/// 금지 참조와 허용 참조, 주석·테스트 제외를 합성 입력으로 확인한다.
#[test]
fn the_inbound_table_catches_what_it_claims() {
    let src = "\
use crate::adapters::production::tcp_ipc_server as _a;
use crate::adapters::production::std_fs::StdFs;
use crate::ipc::handler;
use tasty_ipc::host_call::HostIpcInjector;
use crate::{hook_handler::IpcCall, adapters::ipc::host_call};
fn f() -> crate::AppEvent { todo!() }
fn g() { super::super::hub::x(); }
fn h() { crate::debug_info::dump(); }
// crate::app::App 은 주석이다
#[cfg(test)]
mod tests {
    use crate::adapters::ipc::handler::x;
}
";
    let got: Vec<(usize, &str)> = shipped_references("src/webhook/listener.rs", src, inbound_match)
        .into_iter()
        .map(|(l, n, _)| (l, n))
        .collect();
    assert_eq!(
        got,
        vec![
            (1, "adapters::production::tcp_ipc_server"),
            (3, "ipc"),
            (5, "adapters::ipc"),
            (6, "AppEvent"),
            (7, "hub"),
            (8, "debug_info"),
        ],
        "금지 참조는 1·3·5·6·7·8행이다. outbound adapter(2)와 공용 계약(4), 주석(9), 테스트(12)는 제외돼야 한다."
    );
}
