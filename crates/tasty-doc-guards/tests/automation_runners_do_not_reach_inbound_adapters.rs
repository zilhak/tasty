//! 자동화 실행부 경계 가드 — webhook(`src/webhook/`)과 hook handler(`src/hook_handler/`)의
//! **출하되는** 코드가 inbound adapter(IPC 요청 핸들러 · IPC 서버 · CLI 진입)나 메인 루프를
//! 이름으로 부르면 fail 한다.
//!
//! # 왜 이 가드인가
//!
//! 두 실행부는 외부 사건(HTTP 요청 · 훅 발화)을 받아 **호스트 내부 IPC 호출**을 만든다. 그
//! 호출은 공용 통신 계약(`tasty_ipc::host_call::HostIpcInjector`)으로 메인 루프에 주입된다 —
//! 요청을 받아 처리하는 쪽(`adapters::ipc` 핸들러 트리, `tcp_ipc_server`)의 파일 배치를 알면
//! 안 된다. 그 방향이 리팩토링 마스터플랜의 공용 경계 단위가 없앤 역참조다. 같은 크레이트 안이라
//! 컴파일러는 이 방향을 못 막는다(도메인 가드와 같은 사정 — ADR-0440).
//!
//! 도메인 가드(`domain_does_not_reach_up`)의 좌변은 `src/core` · `src/ports` 뿐이라 이 두
//! 디렉토리를 안 본다. 변이 검증이 그 빈자리를 쟀다 — `src/webhook/mod.rs` 에
//! `use crate::adapters::production::tcp_ipc_server` 를 더해도 아무것도 안 빨개졌다
//! ([ADR-0490](../../../docs/adr/0490-boundary-guards-close-three-holes-found-by-mutation.md)).
//!
//! # 좌변과 판정기
//!
//! 두 디렉토리 아래 `.rs` 중 출하되는 것 — 파일 단위 test-only 를 빼고, 인라인 `#[cfg(test)]`
//! 줄을 뺀다. 테스트가 픽스처를 부르는 것은 정상이다. 경로 읽기(마스킹 · 중괄호 import ·
//! 줄을 넘는 경로 · `super::` 사슬)와 앞마디 일치는 도메인 가드와 **같은 판정기**
//! (`tasty_doc_guards::crate_paths`)다.
//!
//! 기대값은 0 이고 명부가 없다. 세운 날(2026-09-22) 출하 코드의 적중이 0 이었다.
//!
//! # 이 가드가 안 보는 것
//!
//! - **전이 의존.** 실행부가 부르는 형제 모듈이 다시 핸들러를 부르는 경로는 안 센다.
//! - **outbound adapter**(`adapters::production` 의 fs · clock · process 구현 등). 이 가드의
//!   물음은 "요청을 받는 쪽을 아는가" 이고, 실행부가 포트 대신 구현을 직접 쓰는 것은 다른
//!   물음이다. 오늘 적중은 0 이다.

use tasty_doc_guards::crate_paths::{path_is_under, shipped_references};
use tasty_doc_guards::repo_root;
use tasty_doc_guards::shipping_scope::test_only_files;
use tasty_doc_guards::source_text::rust_sources;

/// 자동화 실행부 뿌리.
const RUNNER_ROOTS: &[&str] = &["src/webhook", "src/hook_handler"];

/// 순회가 두 뿌리에 닿았음을 고정하는 앵커 — 각 모듈의 루트 파일.
const RUNNER_ANCHORS: &[&str] = &["src/webhook/mod.rs", "src/hook_handler/mod.rs"];

/// 두 뿌리 아래 `.rs` 수의 하한. 실측 2026-09-22: 18 개(`src/webhook` 10 · `src/hook_handler` 8).
/// 수집이 죽으면 판정이 빈 집합을 훑고 초록이 된다.
const MIN_RUNNER_FILES: usize = 14;

/// 실행부가 이름으로 부르면 안 되는 크레이트 루트 항목 — `(이름, 무엇이라 안 되는가)`.
///
/// 모듈과 함께 lib 루트의 별칭(`src/lib.rs` 의 `pub(crate) use …`)도 적는다 — 별칭만 남기면
/// 별칭으로 우회된다.
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
        "자동화 실행부 뿌리({}) 아래 `.rs` 를 {} 개만 모았다(하한 {MIN_RUNNER_FILES}) — 수집이 \
         죽었거나 뿌리가 옮겨졌다.\n\
         ★ 하한을 내려서 통과시키지 마라. 실행부가 정말 옮겨졌으면 `RUNNER_ROOTS` 를 새 자리로 \
         바꿔라.",
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
        "자동화 실행부(`src/webhook` · `src/hook_handler`) 출하 코드가 inbound adapter 나 메인 \
         루프를 이름으로 부른다:\n{}\n\
         실행부는 공용 통신 계약(`tasty_ipc::host_call`)으로 호출을 주입한다 — 요청을 받는 쪽의 \
         파일 배치를 모른다(ADR-0490). 필요한 타입이 핸들러 쪽에 정의돼 있으면 공용 계약 쪽으로 \
         옮기고 핸들러가 그것을 쓴다.\n\
         ★ 이 가드에 면제 명부를 만들어 통과시키지 마라 — 명부가 비어 있는 것이 이 경계의 \
         현재 상태다.",
        offenders.join("\n")
    );
}

/// 표의 합성 양성·음성 대조 — 생산 트리는 0 자리라 보고 갈래에 오늘 입력이 없다.
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
        ],
        "잡혀야 하는 것: IPC 서버(1) · 핸들러 별칭(3) · 중괄호 항목(5) · 메인 루프 별칭(6) · \
         `super::` 사슬로 루트에 올라간 서버 조립(7 — 깊이 2 파일). 2 행(outbound adapter) · \
         4 행(공용 계약)이 잡히면 표가 넓어진 것이고, 8 행이면 마스킹이, 11 행이면 test 필터가 \
         죽은 것이다."
    );
}
