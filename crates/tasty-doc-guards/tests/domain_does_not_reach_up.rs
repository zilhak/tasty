//! 도메인 경계 가드 — 도메인(`src/core/` · `src/ports/`)의 **출하되는** 코드가 본체의
//! 조립·어댑터·GUI 모듈을 이름으로 부르면 fail 한다. 그리고 도메인 안의 gui feature 게이트
//! 수를 양방향으로 고정한다.
//!
//! # 왜 컴파일러가 아니라 이 가드인가
//!
//! 도메인은 본체와 **같은 크레이트**에 산다(크레이트를 떼지 않은 이유와 대안은
//! [ADR-0440](../../../docs/adr/0440-the-domain-boundary-is-a-module-boundary-with-a-guard-not-a-crate.md)).
//! 같은 크레이트 안에서는 `crate::app::…` 이 언제나 이름 해석된다 — 도메인이 창 조립부를
//! 거꾸로 불러도 컴파일은 통과한다. 크레이트 경계가 해 주었을 일을 이 가드가 대신한다.
//!
//! # 두 물음
//!
//! 1. **상위 참조** — [`UPPER`] 의 모듈을 출하 코드가 부르는가. 기대값 0, 베이스라인 없음.
//!    착수 시점(2026-09-21, `17e2a7f56`)에 34 자리였고 이동·포트 역전으로 0 이 된 뒤에
//!    세웠다 — 그래서 "줄기만 하는 한시 허용" 명부가 필요 없다. 새 자리는 곧 위반이다.
//! 2. **gui 게이트 수** — 도메인 출하 코드에 코드로 쓰인 `feature = "gui"` 의 개수. 도메인에
//!    GUI 전용 항목을 cfg 로 숨겨 들여오면 상위 참조가 없어 보여도 **도메인이 GUI 를 안다**.
//!    그 수를 [`GUI_GATES_IN_DOMAIN`] 에 고정한다. 늘어도 줄어도 실패한다 — 줄면 상수를 같이
//!    내린다(남는 여유가 곧 안 보는 구간이다).
//!
//! # 좌변
//!
//! `src/core/**` · `src/ports/**` 의 `.rs` 중 **출하되는 것** — 파일 단위 test-only
//! ([`test_only_files`])를 빼고, 인라인 `#[cfg(test)]` 줄([`cfg_gated_lines`])을 뺀다.
//! 테스트는 픽스처(`adapters::test`, `state::tests`)를 부르는 것이 정상이고 출하 산출물에
//! 안 들어간다(layering 가드가 같은 이유로 테스트를 뺀다 — ADR-0123).
//!
//! 주석과 문자열은 [`mask_non_code`] 로 지운다. 도메인의 문서 주석은 "이 일은 창 쪽
//! `AppState` 가 한다" 처럼 상위 모듈을 **설명으로** 말하는 것이 정상이다.
//!
//! # 이 가드가 안 보는 것
//!
//! - **전이 의존.** 도메인이 부르는 형제 모듈(`file`·`store`·`hook_handler` 등)이 다시 상위
//!   모듈을 부르는 경로는 안 센다. 실측 2026-09-21: `file::dispatch` 가 `AppState` 를
//!   받는다(`open_picker` 등 gui 전용). 크레이트를 떼는 날 그 경로가 경계를 넘는다 —
//!   ADR-0440 의 재검토 조건이 그 값을 잰다.
//! - **`#[path]` 로 옮겨 붙인 모듈의 `super::` 깊이.** `super::` 이탈 판정은 파일 경로로
//!   모듈 깊이를 계산한다. 오늘 도메인에 `#[path]` 는 test 모듈에만 있다.

use std::path::{Path, PathBuf};

use tasty_doc_guards::cfg_predicate::cfg_gated_lines;
use tasty_doc_guards::repo_root;
use tasty_doc_guards::shipping_scope::test_only_files;
use tasty_doc_guards::source_text::{mask_comments, mask_non_code, rust_sources};

/// 도메인 뿌리. `/` 로 끝나는 접두사다.
const DOMAIN_ROOTS: &[&str] = &["src/core/", "src/ports/"];

/// 도메인이 이름으로 부르면 안 되는 크레이트 루트 항목 — `(이름, 무엇이라 안 되는가)`.
///
/// 모듈과 함께 **lib 루트의 별칭**(`src/lib.rs` 의 `pub(crate) use …`)도 적는다. 별칭은
/// 같은 모듈의 다른 이름이라, 모듈만 막으면 별칭으로 우회된다.
const UPPER: &[(&str, &str)] = &[
    ("app", "창·이벤트 루프 조립(`App`)"),
    ("AppEvent", "`app::event::AppEvent` 의 lib 루트 별칭"),
    ("App", "`app::App` 의 lib 루트 별칭"),
    ("adapters", "어댑터 전체(IPC 핸들러 · UI · production 구현)"),
    ("ipc", "`adapters::ipc` 의 별칭 — 요청 핸들러 트리"),
    ("cli", "`adapters::cli` 의 별칭 — CLI 진입 계층"),
    ("plugin_bridge", "plugin 매니저와 본체 GUI 를 잇는 glue"),
    (
        "state",
        "창 상태(`AppState`) — 도메인은 `core::cascade_window` 포트로만 닿는다",
    ),
    ("search_state", "`state::search` 의 별칭"),
    ("selection", "`state::selection` 의 별칭"),
    (
        "intent",
        "GUI intent 큐 — 발화 주체는 `core::origin` 에 있다",
    ),
    ("view", "창 view 트리"),
    ("window", "`view` 의 별칭"),
    ("gfx", "GPU 렌더링"),
    ("gpu", "`gfx::gpu` 의 별칭"),
    ("renderer", "`gfx::renderer` 의 별칭"),
    ("boot", "프로세스 기동·조립"),
    ("hub", "IPC 서버 조립"),
    (
        "identify_worker",
        "winit proxy 로 결과를 보내는 GUI worker — `core::identify_port` 로만 닿는다",
    ),
    ("file_dispatch", "`file::dispatch` 의 gui 별칭"),
    ("shortcuts", "UI 입력 단축키"),
    ("click_cursor", "UI 입력"),
    ("double_tap", "UI 입력"),
    ("preset_ui", "UI"),
    ("empty_ui", "UI"),
    ("explorer_ui", "UI"),
    ("webview_chrome_ui", "UI"),
    ("terminal_link", "UI"),
    ("plugins_ui", "UI"),
    ("settings_ui", "UI"),
    ("webview", "`host_api::webview` 의 gui 별칭"),
    ("ClipboardContext", "GUI 클립보드 컨텍스트"),
    ("waker_factory_winit", "winit waker"),
    ("debug_info", "`app::debug_info` 의 별칭"),
];

/// 도메인 출하 코드의 `feature = "gui"` 개수. 실측 2026-09-21, 이 가드와 같은 판정기로:
/// 이 가드를 들인 트리(`39a092f4f` 위) **245** · 경계 작업 착수 트리(`17e2a7f56`) **239**.
///
/// 경계 작업이 이 수를 6 **올렸다**. 분해: `core/file.rs` −6(GUI 동작인 picker 적용을
/// `file::dispatch::picker_apply` 로 뺐다) · `core/cascade_window.rs` +9(포트 메서드 중 호출
/// 자리가 이미 gui 로 가려진 통지·튜토리얼 관찰 여덟 개와 그 import 하나) ·
/// `core/host_event.rs` +2(`state.rs` 의 모듈 단위 `allow(dead_code)` 가 가리던 headless
/// `expect` 가 드러났다) · `core/mod.rs` +1(`identify_port` 선언). 늘어난 여덟은 새 GUI
/// 의존이 아니라 **이미 있던 게이트가 도메인 쪽 선언에 옮겨 적힌 것**이다 — 그 메서드들은 GUI
/// 타입을 하나도 안 부른다.
///
/// 이 수는 **목표가 아니라 현재 상태의 못**이다. 도메인이 GUI 전용 항목을 갖는 이유는
/// 대부분 "headless 에 소비자가 없다"(ADR-0346)이고 그 판정 자체는 정당하다. 이 못이 막는
/// 것은 **새 게이트가 조용히 들어오는 것**이다 — 들어올 때 이 수를 올리는 커밋이 그
/// 판단을 드러낸다.
const GUI_GATES_IN_DOMAIN: usize = 245;

/// 도메인 뿌리 아래 `.rs` 수의 하한. 실측 2026-09-21: 92 개(`src/core` 84 · `src/ports` 8),
/// 그중 출하되는 것 91.
/// 수집이 죽거나 뿌리가 옮겨지면 두 판정이 빈 집합을 훑고 초록이 된다.
const MIN_DOMAIN_FILES: usize = 80;

/// 순회가 도메인에 닿았음을 고정하는 앵커 — `core` 모듈의 루트 파일.
const DOMAIN_ANCHOR: &str = "src/core/mod.rs";

fn in_domain(rel: &Path) -> bool {
    let rel = rel.to_string_lossy().replace('\\', "/");
    DOMAIN_ROOTS.iter().any(|r| rel.starts_with(r))
}

/// 파일 경로로 계산한 모듈 깊이 — 크레이트 루트 바로 아래 모듈이 1.
/// `src/core/mod.rs` → 1 · `src/core/attach.rs` → 2 · `src/core/agent/mod.rs` → 2 ·
/// `src/core/agent/task.rs` → 3.
fn module_depth(rel: &str) -> usize {
    let parts: Vec<&str> = rel.trim_start_matches("src/").split('/').collect();
    let last = parts.last().copied().unwrap_or("");
    if last == "mod.rs" {
        parts.len() - 1
    } else {
        parts.len()
    }
}

/// `code` 에서 `head` 바로 뒤에 오는 식별자들(`head` 앞은 식별자 경계여야 한다).
fn idents_after<'a>(code: &'a str, head: &str) -> Vec<(usize, &'a str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(off) = code[from..].find(head) {
        let at = from + off;
        from = at + head.len();
        let boundary = code[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        if !boundary {
            continue;
        }
        let rest = &code[from..];
        let end = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        out.push((at, &rest[..end]));
    }
    out
}

/// 한 줄(주석·문자열을 지운 코드)이 부르는 상위 항목 이름. `super::` 사슬이 크레이트
/// 루트까지 올라가 상위 항목을 부르는 형태도 잡는다.
fn upper_names_in(code: &str, depth: usize) -> Vec<&'static str> {
    let upper = |name: &str| UPPER.iter().find(|(n, _)| *n == name).map(|(n, _)| *n);
    let mut out = Vec::new();
    for (_, name) in idents_after(code, "crate::") {
        out.extend(upper(name));
    }
    // `super::super::…::X` — 사슬 길이가 모듈 깊이와 같으면 X 는 크레이트 루트 항목이다.
    let mut from = 0;
    while let Some(off) = code[from..].find("super::") {
        let at = from + off;
        let boundary = code[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == ':'));
        let mut k = 0;
        let mut i = at;
        while code[i..].starts_with("super::") {
            k += 1;
            i += "super::".len();
        }
        from = i;
        if !boundary || k != depth {
            continue;
        }
        let rest = &code[i..];
        let end = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        out.extend(upper(&rest[..end]));
    }
    out
}

/// 한 파일의 출하되는 줄 가운데 상위 항목을 부르는 줄. `(1-기준 줄번호, 이름, 원문)`.
fn upper_references(rel: &str, text: &str) -> Vec<(usize, &'static str, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let gated = cfg_gated_lines(&lines, "test");
    let masked = mask_non_code(text);
    let depth = module_depth(rel);
    let mut out = Vec::new();
    for (i, code) in masked.lines().enumerate() {
        if gated.get(i).copied().unwrap_or(false) {
            continue;
        }
        for name in upper_names_in(code, depth) {
            out.push((i + 1, name, lines[i].trim().to_string()));
        }
    }
    out
}

/// 한 파일의 출하되는 줄에서 코드로 쓰인 `feature = "gui"` 개수.
fn gui_gates(text: &str) -> usize {
    let lines: Vec<&str> = text.lines().collect();
    let gated = cfg_gated_lines(&lines, "test");
    mask_comments(text)
        .lines()
        .enumerate()
        .filter(|(i, _)| !gated.get(*i).copied().unwrap_or(false))
        .map(|(_, code)| code.matches("feature = \"gui\"").count())
        .sum()
}

/// 출하되는 도메인 파일 `(레포 상대 경로, 원문)`. 하한·앵커를 여기서 확인한다.
fn shipped_domain_sources() -> Vec<(PathBuf, String)> {
    let root = repo_root();
    // 부모 선언을 따라가야 test-only 여부가 정해지므로 `src` 전체를 모은 뒤 거른다.
    let sources = rust_sources(&root, &["src"]);
    let not_shipped = test_only_files(&root, &sources);
    let domain: Vec<(PathBuf, String)> = sources
        .into_iter()
        .filter(|(rel, _)| in_domain(rel))
        .collect();
    assert!(
        domain.len() >= MIN_DOMAIN_FILES,
        "도메인 뿌리({}) 아래 `.rs` 를 {} 개만 모았다(하한 {MIN_DOMAIN_FILES}) — 수집이 죽었거나 \
         뿌리가 옮겨졌다. 빈 집합을 훑으면 아래 판정은 조용히 통과한다.\n\
         ★ 하한을 내려서 통과시키지 마라. 도메인이 정말 옮겨졌으면 `DOMAIN_ROOTS` 를 새 \
         자리로 바꿔라.",
        DOMAIN_ROOTS.join(" · "),
        domain.len()
    );
    assert!(
        domain
            .iter()
            .any(|(rel, _)| rel.to_string_lossy().replace('\\', "/") == DOMAIN_ANCHOR),
        "순회가 `{DOMAIN_ANCHOR}` 에 닿지 않았다 — `core` 모듈의 루트라 실재가 보장된다."
    );
    domain
        .into_iter()
        .filter(|(rel, _)| !not_shipped.contains(rel))
        .collect()
}

#[test]
fn the_domain_does_not_name_an_upper_layer() {
    let sources = shipped_domain_sources();
    let mut offenders = Vec::new();
    for (rel, text) in &sources {
        let rel = rel.to_string_lossy().replace('\\', "/");
        for (line, name, raw) in upper_references(&rel, text) {
            let why = UPPER
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, w)| *w)
                .unwrap_or("");
            offenders.push(format!("  {rel}:{line} — `{name}` ({why}): {raw}"));
        }
    }
    println!(
        "[도메인 경계] 출하 도메인 파일 {} 개 · 상위 참조 {} 자리",
        sources.len(),
        offenders.len()
    );
    assert!(
        offenders.is_empty(),
        "도메인(`src/core` · `src/ports`) 출하 코드가 상위 계층을 이름으로 부른다:\n{}\n\
         도메인은 조립·어댑터·GUI 를 모른다(ADR-0440). 처방은 방향을 뒤집는 것이다:\n\
         - 도메인이 쓰는 타입이 상위 모듈에 정의돼 있으면 **정의를 도메인으로 옮기고** \
           상위 모듈이 재수출한다(`core::origin` · `core::host_event` 가 그렇게 왔다).\n\
         - 도메인이 창 쪽 연산이 필요하면 **도메인이 trait 을 선언하고** 창 쪽이 구현한다 \
           (`core::cascade_window` · `core::identify_port`).\n\
         - GUI 동작이 도메인 안에 cfg 로 숨어 있으면 **GUI 쪽으로 옮긴다** \
           (`file::dispatch::picker_apply`).\n\
         ★ 이 가드에 면제 명부를 만들어 통과시키지 마라 — 명부가 비어 있는 것이 이 경계의 \
         현재 상태다.",
        offenders.join("\n")
    );
}

#[test]
fn gui_gates_in_the_domain_are_pinned() {
    let sources = shipped_domain_sources();
    let mut per_file: Vec<(String, usize)> = sources
        .iter()
        .map(|(rel, text)| (rel.to_string_lossy().replace('\\', "/"), gui_gates(text)))
        .filter(|(_, n)| *n > 0)
        .collect();
    per_file.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let total: usize = per_file.iter().map(|(_, n)| n).sum();
    println!(
        "[도메인 gui 게이트] {total} 개 / {} 파일 (고정값 {GUI_GATES_IN_DOMAIN})",
        per_file.len()
    );
    let listing = per_file
        .iter()
        .map(|(f, n)| format!("  {n:>3}  {f}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        total <= GUI_GATES_IN_DOMAIN,
        "도메인 출하 코드의 `feature = \"gui\"` 가 {total} 개로 늘었다(고정값 \
         {GUI_GATES_IN_DOMAIN}).\n{listing}\n\
         도메인에 GUI 전용 항목을 cfg 로 숨기면 상위 참조 가드가 못 본다 — 그 항목이 \
         창 상태나 GUI 부품을 부르지 않아도 **도메인이 GUI 를 안다**.\n\
         먼저 물어라: 이 항목은 headless 에 소비자가 없어서 가리는가(ADR-0346 ① — 정당), \
         아니면 GUI 동작이 도메인에 들어와서 가리는가(그러면 GUI 쪽으로 옮겨라).\n\
         앞쪽이면 이 상수를 {total} 으로 올리고 그 커밋 본문에 판정을 적어라.",
    );
    assert!(
        total >= GUI_GATES_IN_DOMAIN,
        "도메인 출하 코드의 `feature = \"gui\"` 가 {total} 개로 줄었다(고정값 \
         {GUI_GATES_IN_DOMAIN}). 좋은 일이다 — 상수를 {total} 으로 **같이 내려라**. \
         남겨 두면 그 차이만큼 새 게이트가 조용히 들어올 여유가 된다.\n{listing}",
    );
}

/// 술어의 합성 양성 대조 — 생산 트리는 위반 0 이라 보고 갈래에 오늘 입력이 없다.
#[test]
fn the_predicates_catch_what_they_claim() {
    let src = "\
use crate::core::CoreState;
use crate::state::AppState;
// crate::app::App 은 주석이다
fn f() { let _s = \"crate::intent\"; }
fn g() -> crate::AppEvent { todo!() }
fn h() { super::super::adapters::x(); }
fn k() { super::super::core::x(); }
#[cfg(feature = \"gui\")]
fn gui_only() {}
// #[cfg(feature = \"gui\")] 주석 속 언급

#[cfg(test)]
mod tests {
    use crate::adapters::test::x;
    #[cfg(feature = \"gui\")]
    fn t() {}
}
";
    let got: Vec<(usize, &str)> = upper_references("src/core/agent/task.rs", src)
        .into_iter()
        .map(|(l, n, _)| (l, n))
        .collect();
    // `src/core/agent/task.rs` 의 모듈 깊이는 3 이라 `super::super::` 는 루트에 못 닿는다.
    assert_eq!(
        got,
        vec![(2, "state"), (5, "AppEvent")],
        "잡혀야 하는 것: 2 행(`crate::state`) · 5 행(`crate::AppEvent`). 3 행이 잡히면 주석 \
         마스킹이, 4 행이면 문자열 마스킹이, 14 행이면 인라인 test 필터가 죽은 것이다."
    );
    let got_root: Vec<(usize, &str)> = upper_references("src/core/attach.rs", src)
        .into_iter()
        .map(|(l, n, _)| (l, n))
        .collect();
    // 깊이 2 에서는 `super::super::` 가 크레이트 루트다 — 6 행만 추가로 잡히고 7 행
    // (`core`)은 도메인 자신이라 안 잡힌다.
    assert_eq!(
        got_root,
        vec![(2, "state"), (5, "AppEvent"), (6, "adapters")],
        "`super::` 사슬로 크레이트 루트에 올라가 상위 모듈을 부르는 형태가 안 잡힌다."
    );
    assert_eq!(
        gui_gates(src),
        1,
        "출하되는 코드의 gui 게이트는 8 행 하나다. 주석(10 행)이나 test 블록(15 행)이 세어지면 \
         마스킹이나 test 필터가 죽은 것이다."
    );
    assert_eq!(module_depth("src/core/mod.rs"), 1);
    assert_eq!(module_depth("src/core/attach.rs"), 2);
    assert_eq!(module_depth("src/core/agent/mod.rs"), 2);
    assert_eq!(module_depth("src/core/agent/task.rs"), 3);
    assert_eq!(module_depth("src/ports/clock.rs"), 2);
}
