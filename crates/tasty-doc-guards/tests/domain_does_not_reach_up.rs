//! 도메인(src/core·src/ports)의 제품 코드가 조립·어댑터·GUI를 직접 참조하는지 검사한다.
//! 같은 크레이트 안의 의존 방향은 컴파일러가 제한하지 않으므로 별도로 확인한다(ADR-0002).
//! 상위 모듈 참조, gui 조건의 개수, GUI 크레이트 경로를 각각 검사한다.
//! 기존 gui 조건 안에 새 GUI 참조를 넣으면 조건 수는 그대로라 경로 검사도 필요하다.
//!
//! 파일·인라인 테스트 코드와 주석·문자열은 공유 파서로 제외한다.
//! 중괄호 import와 여러 줄 경로를 읽고 모듈의 앞부분이 일치하는지 확인한다.
//! 전이 의존과 형제 모듈을 통한 재노출은 추적하지 않는다.
//! 다른 크레이트의 GUI feature는 크레이트 이름만으로 구분하지 못한다.
//! 외부 크레이트와 같은 이름의 지역 모듈도 구별하지 못한다.
//! super 깊이는 파일 경로로 계산하므로 #[path]로 배치한 모듈에서는 실제 깊이와 다를 수 있다.

use std::path::{Path, PathBuf};

use tasty_doc_guards::cargo_manifest::feature_enabled_deps;
use tasty_doc_guards::cfg_predicate::cfg_gated_lines;
use tasty_doc_guards::crate_paths::{
    module_depth, path_is_under, shipped_external_references, shipped_references,
};
use tasty_doc_guards::repo_root;
use tasty_doc_guards::shipping_scope::test_only_files;
use tasty_doc_guards::source_text::{mask_comments, rust_sources};

const DOMAIN_ROOTS: &[&str] = &["src/core/", "src/ports/"];

/// 금지할 루트 모듈과 lib 재노출 별칭을 함께 등록한다. 별칭만 막으면 정식 경로로, 정식 경로만 막으면 별칭으로 참조할 수 있다.
const UPPER: &[(&str, &str)] = &[
    ("app", "창·이벤트 루프 조립(`App`)"),
    ("AppEvent", "`app::event::AppEvent` 의 lib 루트 별칭"),
    ("App", "`app::App` 의 lib 루트 별칭"),
    ("adapters", "어댑터 전체(IPC 핸들러 · UI · production 구현)"),
    ("ipc", "`adapters::ipc` 의 별칭 — 요청 핸들러 트리"),
    ("cli", "`adapters::cli` 의 별칭 — CLI 진입 계층"),
    ("plugin_bridge", "plugin 매니저와 본체 GUI 를 잇는 glue"),
    (
        "plugin",
        "`adapters::plugin` 의 별칭 — 매니페스트 타입은 `tasty_plugin_manifest` 로 직접 닿는다",
    ),
    (
        "state",
        "창 상태(`AppState`) — 도메인은 `core::cascade_window` 포트로만 닿는다",
    ),
    ("search_state", "`state::search` 의 별칭"),
    ("selection", "`state::selection` 의 별칭"),
    (
        "intent",
        "GUI intent 큐. 요청의 기원 정보는 core::origin에 정의한다.",
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
    (
        "file::identify_worker",
        "`identify_worker` 의 정식 경로 — `core::identify_port` 로만 닿는다",
    ),
    ("file_dispatch", "`file::dispatch` 의 gui 별칭"),
    (
        "file::dispatch",
        "창 상태를 사용하는 파일 열기 동작. 도메인의 요청 기원 타입은 core::origin에 있다.",
    ),
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
    ("host_api::webview", "`webview` 의 정식 경로 — GUI webview"),
    ("ClipboardContext", "GUI 클립보드 컨텍스트"),
    (
        "clipboard",
        "`ClipboardContext` 의 정식 모듈 — 도메인은 `ports::clipboard` 로 닿는다",
    ),
    ("waker_factory_winit", "winit waker"),
    ("debug_info", "`app::debug_info` 의 별칭"),
];

/// 제품 도메인 코드의 gui 조건 수. gui_gates와 같은 판독으로 측정한 기준값이다.
/// headless에 소비자가 없는 정의를 제외하는 조건 자체는 허용한다(ADR-0003).
/// 증가·감소를 모두 확인해 변경 이유를 검토한다. GUI 동작을 조건부로 숨기는 데 사용하면 안 된다.
const GUI_GATES_IN_DOMAIN: usize = 262;

/// 2026-09-21 실측 92파일(core84·ports8, test 전용이 아닌 파일 91)을 기준으로 둔 수집 하한.
const MIN_DOMAIN_FILES: usize = 80;

/// 도메인 모듈 루트까지 수집됐는지 확인할 파일.
const DOMAIN_ANCHOR: &str = "src/core/mod.rs";

fn in_domain(rel: &Path) -> bool {
    let rel = rel.to_string_lossy().replace('\\', "/");
    DOMAIN_ROOTS.iter().any(|r| rel.starts_with(r))
}

fn upper_match(path: &[String]) -> Option<&'static str> {
    UPPER
        .iter()
        .map(|(n, _)| *n)
        .find(|n| path_is_under(n, path))
}

/// 제품 코드의 상위 참조를 줄 번호·이름·원문으로 반환한다.
fn upper_references(rel: &str, text: &str) -> Vec<(usize, &'static str, String)> {
    shipped_references(rel, text, upper_match)
}

/// 한 파일의 test 전용이 아닌 줄에서 코드로 쓰인 `feature = "gui"` 개수.
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

/// test 전용이 아닌 도메인 파일 `(레포 상대 경로, 원문)`. 하한·앵커를 여기서 확인한다.
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
        "도메인 루트({})에서 Rust 파일을 {}개만 수집했다(하한 {MIN_DOMAIN_FILES}). 실제 이동이라면 DOMAIN_ROOTS를 갱신하고, 수집 실패라면 하한을 낮추지 말고 원인을 고친다.",
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
        "[도메인 경계] 제품 도메인 파일 {} 개 · 상위 참조 {} 자리",
        sources.len(),
        offenders.len()
    );
    assert!(
        offenders.is_empty(),
        "도메인 제품 코드가 상위 계층을 참조한다:\n{}\nADR-0002에 따라 도메인 타입은 도메인에서 정의하고, 창 연산은 도메인이 선언한 trait을 창 쪽에서 구현한다. GUI 동작은 GUI 쪽으로 옮긴다. 예외 목록을 추가해 통과시키지 않는다.",
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
        "도메인의 gui 조건이 {total}개로 기준 {GUI_GATES_IN_DOMAIN}보다 늘었다.\n{listing}\nheadless 소비자가 없는 정의를 제외하는 조건인지(ADR-0003), GUI 동작을 숨긴 것인지 확인한다. 정당한 조건이면 기준을 {total}로 갱신하고 이유를 남긴다.",
    );
    assert!(
        total >= GUI_GATES_IN_DOMAIN,
        "도메인의 gui 조건이 {total}개로 기준 {GUI_GATES_IN_DOMAIN}보다 줄었다. 실제 감소인지 확인한 뒤 기준을 {total}로 낮춘다.\n{listing}",
    );
}

/// gui feature가 dep:로 활성화하는 optional 의존을 매니페스트에서 읽는다.
/// 다른 크레이트의 feature를 활성화하는 항목은 제외한다. 그 크레이트는 headless에도 있어 이름만으로 GUI 사용을 구분할 수 없다.
fn gui_crates() -> Vec<String> {
    let manifest = std::fs::read_to_string(repo_root().join("Cargo.toml"))
        .expect("워크스페이스 Cargo.toml 을 읽을 수 없다");
    let crates: Vec<String> = feature_enabled_deps(&manifest, "gui")
        .into_iter()
        .map(|d| d.replace('-', "_"))
        .collect();
    for anchor in [
        "egui",
        "winit",
        "wgpu",
        "webkit2gtk",
        "gtk",
        "objc2_app_kit",
        "webview2_com",
    ] {
        assert!(
            crates.iter().any(|c| c == anchor),
            "gui feature의 의존 목록에 {anchor}가 없다: {crates:?}. 매니페스트 변경과 판독 오류를 구별한다."
        );
    }
    crates
}

/// windows 크레이트는 프로세스·콘솔·파일 API에도 필요하므로 창·그리기 경로만 막는다.
/// 금지 경로의 상위 모듈 import도 포함해 별칭·glob으로 그 아래에 접근하는 경우를 놓치지 않는다.
/// Foundation에는 비 GUI 타입도 있어 창 핸들 관련 항목만 개별 등록한다.
const OS_WINDOW_PATHS: &[&str] = &[
    "windows::Win32::UI",
    "windows::Win32::Graphics",
    "windows::Win32::Foundation::HWND",
    "windows::Win32::Foundation::HINSTANCE",
    "windows::Win32::Foundation::LPARAM",
    "windows::Win32::Foundation::WPARAM",
];

/// 기존 GUI 참조를 (파일, 경로)로 기록한다. 조건 수가 그대로여도 새 참조가 생겼는지 확인한다.
const GUI_CRATE_PATHS_IN_DOMAIN: &[(&str, &str)] = &[];

/// 제품 코드의 GUI 크레이트 경로를 줄 번호·경로·원문으로 반환한다.
fn gui_crate_references(text: &str, crates: &[String]) -> Vec<(usize, String, String)> {
    let mut roots: Vec<&str> = crates.iter().map(String::as_str).collect();
    roots.extend(OS_WINDOW_PATHS.iter().filter_map(|p| p.split("::").next()));
    shipped_external_references(text, &roots)
        .into_iter()
        .filter_map(|(line, path, raw)| {
            let path = path
                .strip_suffix("::self")
                .map(str::to_string)
                .unwrap_or(path);
            let head = path.split("::").next().unwrap_or("");
            let hit = crates.iter().any(|c| c == head)
                || OS_WINDOW_PATHS.iter().any(|p| {
                    path == *p
                        || path.starts_with(&format!("{p}::"))
                        || p.starts_with(&format!("{path}::"))
                });
            hit.then_some((line, path, raw))
        })
        .collect()
}

#[test]
fn the_domain_names_no_gui_crate_beyond_the_pinned_list() {
    let crates = gui_crates();
    let sources = shipped_domain_sources();
    let mut found: Vec<(String, String, usize, String)> = Vec::new();
    for (rel, text) in &sources {
        let rel = rel.to_string_lossy().replace('\\', "/");
        for (line, path, raw) in gui_crate_references(text, &crates) {
            found.push((rel.clone(), path, line, raw));
        }
    }
    println!(
        "[도메인 GUI 크레이트] 크레이트 {} 개 · 제품 도메인 파일 {} 개 · 자리 {} (고정 목록 {})",
        crates.len(),
        sources.len(),
        found.len(),
        GUI_CRATE_PATHS_IN_DOMAIN.len()
    );
    let new: Vec<String> = found
        .iter()
        .filter(|(f, p, _, _)| !GUI_CRATE_PATHS_IN_DOMAIN.contains(&(f.as_str(), p.as_str())))
        .map(|(f, p, l, raw)| format!("  {f}:{l} — `{p}`: {raw}"))
        .collect();
    assert!(
        new.is_empty(),
        "도메인에서 GUI 크레이트를 참조한다:\n{}\ngui 조건 뒤에 있어도 금지된다(ADR-0002). GUI 동작을 옮기거나 도메인 포트로 분리한다. GUI_CRATE_PATHS_IN_DOMAIN에 새 예외를 넣어 통과시키지 않는다.",
        new.join("\n")
    );
    let stale: Vec<String> = GUI_CRATE_PATHS_IN_DOMAIN
        .iter()
        .filter(|(f, p)| !found.iter().any(|(ff, pp, _, _)| ff == f && pp == p))
        .map(|(f, p)| format!("  {f} — `{p}`"))
        .collect();
    assert!(
        stale.is_empty(),
        "소스에서 사라진 GUI 참조가 기준 목록에 남았다. 같은 참조가 다시 들어와도 통과하지 않도록 항목을 제거한다:\n{}",
        stale.join("\n")
    );
}

#[test]
fn the_gui_crate_reader_catches_the_spellings() {
    let crates: Vec<String> = ["egui", "winit", "image"].map(String::from).to_vec();
    let src = "\
#[cfg(feature = \"gui\")]
use egui::Context;
use ::winit::window::Window;
use egui as e;
use winit::{
    event::WindowEvent,
    dpi::PhysicalSize,
};
use {egui::Ui, std::fmt};
fn a() -> ::egui::Rect { todo!() }
use crate::image::Thumb;
fn b(s: &S) { s.egui.x(); super::image::y(); }
// egui::Painter 는 주석이다
fn c() { let _ = \"winit::x\"; }
use windows::Win32::UI::WindowsAndMessaging::SetFocus;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::{Graphics::Gdi::HDC, System::Console::X};
extern crate image;
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32 as w32;
use windows::Win32::*;
use windows::Win32::System::*;
use windows::Win32::Foundation::{self, BOOL};
#[cfg(test)]
mod tests {
    use egui::Pos2;
}
";
    let got: Vec<(usize, String)> = gui_crate_references(src, &crates)
        .into_iter()
        .map(|(l, p, _)| (l, p))
        .collect();
    let want: Vec<(usize, String)> = vec![
        (2, "egui::Context"),
        (3, "winit::window::Window"),
        (4, "egui"),
        (6, "winit::event::WindowEvent"),
        (7, "winit::dpi::PhysicalSize"),
        (9, "egui::Ui"),
        (10, "egui::Rect"),
        (15, "windows::Win32::UI::WindowsAndMessaging::SetFocus"),
        (17, "windows::Win32::Graphics::Gdi::HDC"),
        (18, "image"),
        (19, "windows::Win32::Foundation::HWND"),
        (20, "windows::Win32"),
        (21, "windows::Win32"),
        (23, "windows::Win32::Foundation"),
    ]
    .into_iter()
    .map(|(l, p)| (l, p.to_string()))
    .collect();
    assert_eq!(
        got, want,
        "GUI 크레이트·창 경로와 상위 모듈 import만 검출해야 한다. 필드 접근·주석·문자열·비 GUI Windows 경로·테스트 전용 참조는 제외한다."
    );
}

#[test]
fn the_gui_crate_list_is_read_from_the_feature() {
    let manifest = "\
[dependencies]
gui = { version = \"1\" }

[features]
default = [\"gui\"]
gui = [
    \"dep:winit\",
    # \"dep:commented-out\"
    \"tasty-platform/gui\",
    \"dep:egui-wgpu\", \"dep:gtk\",
]
other = [\"dep:not-gui\"]
";
    assert_eq!(
        feature_enabled_deps(manifest, "gui"),
        vec!["winit", "egui-wgpu", "gtk"],
        "`[features]` 의 `gui` 배열에서 `dep:` 항목만 읽어야 한다 — 주석 속 항목 · \
         다른 크레이트의 feature · 다른 feature · 의존 절의 같은 이름은 빼고."
    );
}

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
    assert_eq!(
        got,
        vec![(2, "state"), (5, "AppEvent")],
        "상위 참조는 2행의 state와 5행의 AppEvent다. 주석·문자열·인라인 테스트 참조는 제외한다."
    );
    let got_root: Vec<(usize, &str)> = upper_references("src/core/attach.rs", src)
        .into_iter()
        .map(|(l, n, _)| (l, n))
        .collect();
    assert_eq!(
        got_root,
        vec![(2, "state"), (5, "AppEvent"), (6, "adapters")],
        "`super::` 사슬로 크레이트 루트에 올라가 상위 모듈을 부르는 형태가 안 잡힌다."
    );
    assert_eq!(
        gui_gates(src),
        1,
        "제품 코드의 gui 조건은 8행 하나다. 주석과 테스트 블록의 조건은 세지 않는다."
    );
    assert_eq!(module_depth("src/core/mod.rs"), 1);
    assert_eq!(module_depth("src/core/attach.rs"), 2);
    assert_eq!(module_depth("src/core/agent/mod.rs"), 2);
    assert_eq!(module_depth("src/core/agent/task.rs"), 3);
    assert_eq!(module_depth("src/ports/clock.rs"), 2);

    let src2 = "\
use crate::file::identify_worker::IdentifyWorker;
use crate::file::dispatch;
use crate::host_api::webview;
use crate::clipboard::ClipboardContext;
use crate::plugin::manager::PluginManager;
use crate::file::format::FileTarget;
use crate::host_api::hooks::global::GlobalHookManager;
use crate::ports::clipboard::ClipboardSystem;
use crate::{core::CoreState, state::AppState};
use crate::{file::{format::DetectDepth, dispatch::open_picker}};
use crate::
    app::App;
use crate::{
    core::origin::IntentOrigin,
    intent::Intent,
};
fn m() { crate :: view :: x(); }
fn n() -> crate::core::origin::FileDispatchOrigin { todo!() }
";
    let got2: Vec<(usize, &str)> = upper_references("src/core/attach.rs", src2)
        .into_iter()
        .map(|(l, n, _)| (l, n))
        .collect();
    assert_eq!(
        got2,
        vec![
            (1, "file::identify_worker"),
            (2, "file::dispatch"),
            (3, "host_api::webview"),
            (4, "clipboard"),
            (5, "plugin"),
            (9, "state"),
            (10, "file::dispatch"),
            (12, "app"),
            (15, "intent"),
            (17, "view"),
        ],
        "정식 경로·별칭·중괄호 import·여러 줄 경로를 검출해야 한다. 같은 형제 모듈의 비 GUI 하위 경로는 제외한다."
    );
}
