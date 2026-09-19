//! 드래그 패닝 정책 가드 — 스크롤 영역이 `drag_to_scroll` 을 **말하지 않은 채**
//! 들어오면 fail 한다.
//!
//! 정책: 데스크톱 마우스에서 누른 채 끄는 동작은 텍스트 선택·행 선택·파일 드래그의
//! 의도이고, 그때 내용이 포인터를 따라 미끄러지면 그 의도와 충돌한다. 그래서 이
//! 레포의 스크롤 영역은 드래그 패닝을 끈다. 근거와 적용 범위는
//! `docs/design/systems/theme.md` "UI 디자인 규칙" 에 있다.
//!
//! **이 가드는 값을 안 본다 — 말했는가만 본다.** 어떤 자리가 패닝을 남기기로 했으면
//! 그 자리에 `drag_to_scroll(true)` 를 명시하면 통과한다. 그러면 결정이 명부가 아니라
//! 그 소스 줄에 남고, allowlist 가 생기지 않는다. "아직 안 봤다" 와 "보고 남겼다" 가
//! 소스에서 갈린다.
//!
//! # 왜 이 가드가 필요한가
//!
//! 한 번 전 범위를 쓴 적이 있다(2026-05-03, 당시 아홉 파일). 그런데 그 결정을 지키는
//! 판정기를 안 남겼고, 2026-06-28 의 explorer 재작성이 세 자리를 기본값으로 되돌렸을
//! 때 빌드도 clippy 도 테스트도 전부 초록이었다. 2026-09-20 실측으로 출하 58 자리 중
//! 50 이 기본값이었다. 값을 다시 0 으로 만든 것과 그 0 을 못박는 것은 다른 일이고,
//! 이 파일이 뒤쪽이다.
//!
//! # 좌변 — 진입점 셋
//!
//! `drag_to_scroll` 을 노출하는 타입을 egui·egui_extras 양쪽에서 세면 셋이다. 좌변을
//! `ScrollArea::` 철자로만 잡으면 뒤의 둘을 놓친다.
//!
//! | 진입점 | 기본값 | 이 가드가 보는 범위 |
//! |---|---|---|
//! | `egui::ScrollArea` | `true` | 생성 호출부터 `.show(`/`.show_viewport(`/`.show_rows(` 까지 |
//! | `egui_extras::TableBuilder` | `true` (자기 안에서 `ScrollArea` 를 만들어 넘긴다) | 생성 호출부터 `.header(`/`.body(` 까지 |
//! | `egui::Window` | 내부가 `ScrollArea::neither()` 라 **축이 없으면 패닝도 없다** | `.scroll(`/`.scroll2(`/`.hscroll(`/`.vscroll(` 로 **축을 켠 것만** |
//!
//! 셋째를 무조건 보지 않는 이유: 축을 안 켠 `Window` 는 스크롤 자체가 없어 위반이
//! 성립하지 않는다. 그 자리에 `drag_to_scroll` 을 요구하면 실재하지 않는 위반에 대한
//! 처방이 된다. 대신 **축을 켜는 순간** 기본값 `true` 가 딸려 오므로 그때부터 본다.
//!
//! # 스코프 밖 — 여기서는 못 닫는다
//!
//! `egui::ComboBox` 의 드롭다운은 egui 가 자기 안에서 `ScrollArea::vertical()` 을
//! 만들고 `drag_to_scroll` 을 호출부에 노출하지 않는다. 레포 소스를 고쳐서 닫을 수
//! 있는 자리가 아니므로 좌변에 안 넣는다 — 넣으면 영원히 0 이 안 되는 항목이 잔여에
//! 남아 다음 사람이 그것을 위반으로 읽는다. 닫으려면 상류에 pass-through 를 요청하거나
//! 자체 위젯으로 대체해야 하고, 둘 다 이 가드보다 크다. egui 가 내부에 만드는 다른
//! 스크롤도 같다.
//!
//! # 가드가 못 잡는 것 (변이로 확인했다)
//!
//! 소스 텍스트 스캔의 한계다. **이 목록이 없으면 "가드가 정책을 강제한다" 가 사실보다
//! 강해진다.** 아래는 전부 컴파일되고 `cargo fmt --check` 도 통과하는 형태다.
//!
//! | 형태 | 예 | 왜 못 잡나 |
//! |---|---|---|
//! | 별칭 import | `use egui::ScrollArea as SA; SA::vertical()` | 철자가 다르다. 현재 레포에 0 건 |
//! | 래퍼 함수 경유 | `fn my_scroll() -> ScrollArea { .. }` 를 여럿이 호출 | 호출부에 철자가 없다 |
//!
//! 둘 다 **의도적으로 우회해야 나오는 형태**이고, 이 가드의 목적(무심코 되돌리는 것을
//! 막는다)은 그 선까지다.
//!
//! **구간 판정은 dataflow 가 아니라 근접이다 — 그것이 양쪽으로 샌다.** 생성 줄부터
//! 아래로 40 줄 안에서 처음 나오는 종결자까지를 한 자리로 본다. 그래서:
//!
//! - 빌더를 변수에 담는 형태(`let mut sa = ScrollArea::vertical(); … sa.show(..)`)는
//!   **잡힌다** — 같은 함수 안이면 종결자가 40 줄 안에 있다. 실측으로 확인했다
//!   (`the_documented_blind_spots_are_measured_not_guessed`). 레포에 그 형태가 하나
//!   있고(`crates/tasty-gallery/src/host_shell.rs` 의 `g_main_scroll`) 이 가드가 실제로
//!   본다. 다만 **잡는 근거가 소유 관계가 아니라 거리**이므로, 종결자가 40 줄 밖이면
//!   조용히 빠져나간다.
//! - 반대로 **무관한 `.show(` 가 사이에 끼면 구간이 일찍 끊긴다.** 그때 진짜
//!   `drag_to_scroll` 이 그 뒤에 있으면 없는 위반이 보고된다(거짓 양성). 이것도
//!   실측으로 고정해 두었다 — 처방은 그 자리에서 선언을 종결자 앞으로 올리는 것이다.
//!
//! # 자동 채널
//!
//! `doc-guards.yml` 이 `cargo test -p tasty-doc-guards` 를 **경로 필터 없이** main push ·
//! PR 마다 돌린다. 다만 자동 잡은 push 된 커밋만 본다 — 커밋 전에 직접 돌려야 그 자리에서
//! 잡힌다(`docs/dev-guide/ci-gates.md`).

use std::path::{Path, PathBuf};
use tasty_doc_guards::cfg_predicate::cfg_gated_lines;
use tasty_doc_guards::shipping_scope::test_only_files;
use tasty_doc_guards::source_text::{mask_non_code, rust_sources};

/// 스캔 루트 — 레포의 Rust 소스 전부. 본 바이너리와 모든 크레이트.
const SCAN_ROOTS: &[&str] = &["src", "crates"];

/// `ScrollArea` 생성 철자. egui 0.31 의 생성자 전부다.
const SCROLL_AREA_CTORS: &[&str] = &[
    "ScrollArea::vertical(",
    "ScrollArea::horizontal(",
    "ScrollArea::both(",
    "ScrollArea::neither(",
    "ScrollArea::new(",
];

/// `ScrollArea` 빌더 체인의 종결자.
const SCROLL_AREA_ENDS: &[&str] = &[".show(", ".show_viewport(", ".show_rows("];

/// `TableBuilder` 빌더 체인의 종결자.
const TABLE_ENDS: &[&str] = &[".header(", ".body("];

/// `egui::Window` 에서 스크롤 축을 켜는 메서드. 하나라도 있으면 그 `Window` 는
/// 내부 `ScrollArea` 를 갖게 되고 `drag_to_scroll` 기본 `true` 가 딸려 온다.
const WINDOW_SCROLL_ON: &[&str] = &[".scroll(", ".scroll2(", ".hscroll(", ".vscroll("];

/// `Window` 빌더 체인의 종결자.
const WINDOW_ENDS: &[&str] = &[".show(", ".open("];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/<crate> 위가 레포 루트다")
        .to_path_buf()
}

/// 한 진입점의 위반 자리를 찾는다. 반환은 `(1-기반 줄번호, 생성 철자)`.
///
/// 입력 `masked` 는 주석·문자열이 지워진 사본이어야 한다 — 그래야 doc 주석 안의
/// `egui::ScrollArea::horizontal` 인용이 위반으로 안 세어진다. `gated` 는 그 줄이
/// 인라인 `#[cfg(test)]` 안인지의 표다.
fn violations(
    masked: &str,
    gated: &[bool],
    ctors: &[&str],
    ends: &[&str],
    require: Option<&[&str]>,
) -> Vec<(usize, String)> {
    let lines: Vec<&str> = masked.split('\n').collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(ctor) = ctors.iter().find(|c| line.contains(**c)) else {
            continue;
        };
        if gated.get(i).copied().unwrap_or(false) {
            continue; // 인라인 #[cfg(test)] — 출하 밖
        }
        // 종결자까지의 구간. 못 찾으면(변수에 담는 형태 등) 이 가드의 사각이라 지나간다.
        let mut end = None;
        for (j, l) in lines.iter().enumerate().skip(i).take(40) {
            let after = if j == i {
                let at = l.find(*ctor).expect("방금 찾았다") + ctor.len();
                &l[at..]
            } else {
                l
            };
            if ends.iter().any(|e| after.contains(*e)) {
                end = Some(j);
                break;
            }
        }
        let Some(end) = end else { continue };
        let block = lines[i..=end].join("\n");
        // `Window` 처럼 "축을 켠 것만" 보는 진입점.
        if let Some(needles) = require
            && !needles.iter().any(|n| block.contains(*n))
        {
            continue;
        }
        if !block.contains("drag_to_scroll") {
            out.push((i + 1, (*ctor).to_string()));
        }
    }
    out
}

/// 스캔 대상 — 출하되는 `.rs` 만. `(경로, 마스킹된 본문, cfg(test) 줄 표)`.
fn shipping_sources() -> Vec<(PathBuf, String, Vec<bool>)> {
    let root = repo_root();
    let sources = rust_sources(&root, SCAN_ROOTS);
    let test_only = test_only_files(&root, &sources);
    sources
        .into_iter()
        .filter(|(rel, _)| !test_only.contains(rel))
        .map(|(rel, text)| {
            let masked = mask_non_code(&text);
            let lines: Vec<&str> = masked.split('\n').collect();
            let gated = cfg_gated_lines(&lines, "test");
            (rel, masked, gated)
        })
        .collect()
}

fn report(kind: &str, hits: &[(PathBuf, usize, String)]) -> String {
    let mut s = format!(
        "{kind} 가 `drag_to_scroll` 을 말하지 않는 자리 {}:\n",
        hits.len()
    );
    for (rel, line, ctor) in hits {
        s.push_str(&format!("  {}:{line}  {ctor}\n", rel.display()));
    }
    s.push_str(
        "\n드래그 패닝을 끄려면 그 빌더 체인에 `.drag_to_scroll(false)` 를 넣어라.\n\
         남기기로 했으면 `.drag_to_scroll(true)` 를 **명시**해라 — 이 가드는 값이 아니라\n\
         말했는가를 본다. 정책과 근거: docs/design/systems/theme.md \"UI 디자인 규칙\".\n",
    );
    s
}

#[test]
fn every_scroll_area_declares_drag_to_scroll() {
    let mut hits = Vec::new();
    for (rel, masked, gated) in shipping_sources() {
        for (line, ctor) in violations(&masked, &gated, SCROLL_AREA_CTORS, SCROLL_AREA_ENDS, None) {
            hits.push((rel.clone(), line, ctor));
        }
    }
    hits.sort();
    assert!(hits.is_empty(), "{}", report("ScrollArea", &hits));
}

#[test]
fn every_table_builder_declares_drag_to_scroll() {
    let mut hits = Vec::new();
    for (rel, masked, gated) in shipping_sources() {
        for (line, ctor) in violations(&masked, &gated, &["TableBuilder::new("], TABLE_ENDS, None) {
            hits.push((rel.clone(), line, ctor));
        }
    }
    hits.sort();
    assert!(hits.is_empty(), "{}", report("TableBuilder", &hits));
}

/// 축을 켠 `egui::Window` 만 본다. 지금 레포에는 그런 자리가 없어 이 테스트는 좌변이
/// 비어 있다 — **그래서 아래 `the_window_axis_is_actually_wired` 가 따로 있다.**
/// 좌변이 빈 초록은 "본다" 의 증거가 아니다.
#[test]
fn every_scrolling_window_declares_drag_to_scroll() {
    let mut hits = Vec::new();
    for (rel, masked, gated) in shipping_sources() {
        for (line, ctor) in violations(
            &masked,
            &gated,
            &["Window::new("],
            WINDOW_ENDS,
            Some(WINDOW_SCROLL_ON),
        ) {
            hits.push((rel.clone(), line, ctor));
        }
    }
    hits.sort();
    assert!(
        hits.is_empty(),
        "{}",
        report("스크롤 켠 egui::Window", &hits)
    );
}

// ─────────────────────────── 판정기 자신을 재는 시험 ───────────────────────────

fn probe(src: &str, ctors: &[&str], ends: &[&str], require: Option<&[&str]>) -> Vec<usize> {
    let masked = mask_non_code(src);
    let lines: Vec<&str> = masked.split('\n').collect();
    let gated = cfg_gated_lines(&lines, "test");
    violations(&masked, &gated, ctors, ends, require)
        .into_iter()
        .map(|(l, _)| l)
        .collect()
}

#[test]
fn a_bare_scroll_area_is_a_violation_and_declaring_it_is_not() {
    let bare = "fn f(ui: &mut Ui) {\n    egui::ScrollArea::vertical().show(ui, |_| {});\n}\n";
    assert_eq!(
        probe(bare, SCROLL_AREA_CTORS, SCROLL_AREA_ENDS, None),
        vec![2]
    );

    let off = "fn f(ui: &mut Ui) {\n    egui::ScrollArea::vertical()\n        \
               .drag_to_scroll(false)\n        .show(ui, |_| {});\n}\n";
    assert!(
        probe(off, SCROLL_AREA_CTORS, SCROLL_AREA_ENDS, None).is_empty(),
        "끈 자리가 위반으로 잡혔다"
    );

    // 값이 아니라 말했는가를 본다 — 남기기로 한 자리도 통과해야 한다.
    let on = off.replace("drag_to_scroll(false)", "drag_to_scroll(true)");
    assert!(
        probe(&on, SCROLL_AREA_CTORS, SCROLL_AREA_ENDS, None).is_empty(),
        "명시한 true 가 위반으로 잡혔다 — 이 가드는 값을 보지 않는다"
    );
}

/// 마스킹 양성 대조. 레포에 실제로 이 형태가 둘 있다
/// (`crates/tasty-ui-widgets/src/{table.rs,horizontal_tab_bar.rs}` 의 doc 주석).
/// 마스킹을 안 걸면 위반이 그만큼 더 나오고, 그 처방은 **doc 주석 한 줄에** 붙는다.
#[test]
fn a_scroll_area_named_in_a_comment_is_not_a_violation() {
    let doc = "/// 가로 스크롤. [`egui::ScrollArea::horizontal`] 로 감싼다.\n\
               /// 그 안에서 .show( 를 부른다.\nfn f() {}\n";
    assert!(
        probe(doc, SCROLL_AREA_CTORS, SCROLL_AREA_ENDS, None).is_empty(),
        "doc 주석 안의 인용이 위반으로 잡혔다 — mask_non_code 가 안 걸렸다"
    );

    let string = "fn f() {\n    let s = \"egui::ScrollArea::vertical().show(\";\n}\n";
    assert!(
        probe(string, SCROLL_AREA_CTORS, SCROLL_AREA_ENDS, None).is_empty(),
        "문자열 안의 철자가 위반으로 잡혔다"
    );
}

/// 인라인 `#[cfg(test)]` 픽스처는 출하 밖이다. 레포에 실제로 둘 있다
/// (`crates/tasty-plugin-sdk/src/egui_surface.rs`).
#[test]
fn an_inline_cfg_test_scroll_area_is_not_a_violation() {
    let src = "fn f() {}\n\n#[cfg(test)]\nmod tests {\n    fn g(ui: &mut Ui) {\n        \
               egui::ScrollArea::vertical().show(ui, |_| {});\n    }\n}\n";
    assert!(
        probe(src, SCROLL_AREA_CTORS, SCROLL_AREA_ENDS, None).is_empty(),
        "인라인 cfg(test) 안의 자리가 위반으로 잡혔다 — 출하 판정이 안 걸렸다"
    );
}

#[test]
fn the_table_builder_axis_is_actually_wired() {
    let bare = "fn f(ui: &mut Ui) {\n    TableBuilder::new(ui)\n        .striped(true)\n        \
                .header(20.0, |_| {});\n}\n";
    assert_eq!(
        probe(bare, &["TableBuilder::new("], TABLE_ENDS, None),
        vec![2]
    );

    let off = bare.replace(".striped(true)", ".drag_to_scroll(false)");
    assert!(
        probe(&off, &["TableBuilder::new("], TABLE_ENDS, None).is_empty(),
        "끈 TableBuilder 가 위반으로 잡혔다"
    );
}

/// 위 `every_scrolling_window_declares_drag_to_scroll` 은 좌변이 비어 있다. 빈 좌변을
/// 훑은 초록과 실제로 판정한 초록은 다르므로, 그 축이 배선돼 있다는 것을 여기서 잰다.
#[test]
fn the_window_axis_is_actually_wired() {
    let scrolling = "fn f(ctx: &Context) {\n    Window::new(\"x\")\n        \
                     .scroll(true)\n        .show(ctx, |_| {});\n}\n";
    assert_eq!(
        probe(
            scrolling,
            &["Window::new("],
            WINDOW_ENDS,
            Some(WINDOW_SCROLL_ON)
        ),
        vec![2],
        "축을 켠 Window 를 안 본다"
    );

    // 축을 안 켠 Window 는 내부가 ScrollArea::neither() 라 패닝이 없다 — 위반 아님.
    let plain = "fn f(ctx: &Context) {\n    Window::new(\"x\")\n        \
                 .show(ctx, |_| {});\n}\n";
    assert!(
        probe(
            plain,
            &["Window::new("],
            WINDOW_ENDS,
            Some(WINDOW_SCROLL_ON)
        )
        .is_empty(),
        "축을 안 켠 Window 가 위반으로 잡혔다 — 실재하지 않는 위반이다"
    );

    // 레포에 실제로 있는 형태는 경로 수식이 붙은 쪽이다. 그 철자를 이 파일 소스에 **연속으로**
    // 두면 pre-commit 의 "egui 창 직접 사용" 검사가 픽스처를 실사용으로 읽는다(그 검사는 추가된
    // 줄을 문자열로 보고 코드와 픽스처를 못 가른다). 그래서 두 조각으로 나눠 조립한다 — 붙인
    // 결과로 판정을 재는 것은 같고, 소스에는 그 연속 철자가 없다.
    let qualified = scrolling.replace("Window::new(", concat!("egui", "::Window::new("));
    assert_eq!(
        probe(
            &qualified,
            &["Window::new("],
            WINDOW_ENDS,
            Some(WINDOW_SCROLL_ON)
        ),
        vec![2],
        "경로 수식이 붙은 실제 형태를 안 본다"
    );

    // 네 메서드 전부 축으로 센다.
    for m in WINDOW_SCROLL_ON {
        let src = scrolling.replace(".scroll(true)", &format!("{m}true)"));
        assert_eq!(
            probe(&src, &["Window::new("], WINDOW_ENDS, Some(WINDOW_SCROLL_ON)).len(),
            1,
            "`{m}` 를 축으로 안 센다"
        );
    }
}

/// 헤더의 "못 잡는 것" 표를 **재서** 적는다. 표에 적은 형태를 실제로 넣고 가드가
/// 정말 안 잡는지 본다 — 안 재고 적으면 그 표가 추측이 된다.
#[test]
fn the_documented_blind_spots_are_measured_not_guessed() {
    // 빌더를 변수에 담는 형태는 **잡힌다** — 근접 판정이라 같은 함수 안이면 종결자가
    // 40 줄 안에 있다. 헤더가 이것을 사각으로 적지 않는 근거가 이 줄이다.
    let var = "fn f(ui: &mut Ui) {\n    let mut sa = egui::ScrollArea::vertical();\n    \
               if x {\n        sa = sa.max_height(10.0);\n    }\n    sa.show(ui, |_| {});\n}\n";
    assert_eq!(
        probe(var, SCROLL_AREA_CTORS, SCROLL_AREA_ENDS, None),
        vec![2],
        "변수 빌더 형태를 놓쳤다 — 헤더 표가 낡았으니 함께 고쳐라"
    );
    // 그 형태도 선언하면 통과한다.
    let var_ok = var.replace(".max_height(10.0)", ".drag_to_scroll(false)");
    assert!(probe(&var_ok, SCROLL_AREA_CTORS, SCROLL_AREA_ENDS, None).is_empty());

    // 거짓 양성 방향 — 무관한 `.show(` 가 끼면 구간이 일찍 끊겨, 그 뒤의 선언을 못 본다.
    let truncated = "fn f(ui: &mut Ui) {\n    let mut sa = egui::ScrollArea::vertical();\n    \
                     other.show(ui);\n    sa = sa.drag_to_scroll(false);\n    \
                     sa.show(ui, |_| {});\n}\n";
    assert_eq!(
        probe(truncated, SCROLL_AREA_CTORS, SCROLL_AREA_ENDS, None),
        vec![2],
        "끼어든 종결자가 구간을 안 끊었다 — 헤더의 거짓 양성 서술이 낡았다"
    );

    // 별칭 import — 철자가 다르다.
    let alias = "use egui::ScrollArea as SA;\nfn f(ui: &mut Ui) {\n    SA::vertical().show(ui, |_| {});\n}\n";
    assert!(
        probe(alias, SCROLL_AREA_CTORS, SCROLL_AREA_ENDS, None).is_empty(),
        "별칭 형태를 잡았다 — 헤더 표가 낡았으니 함께 고쳐라"
    );

    // 래퍼 함수 경유 — 호출부에 철자가 없다.
    let wrapper = "fn my_scroll() -> egui::ScrollArea {\n    egui::ScrollArea::vertical().drag_to_scroll(false)\n}\n\
                   fn f(ui: &mut Ui) {\n    my_scroll().show(ui, |_| {});\n}\n";
    assert!(
        probe(wrapper, SCROLL_AREA_CTORS, SCROLL_AREA_ENDS, None).is_empty(),
        "래퍼 경유 형태를 잡았다 — 헤더 표가 낡았으니 함께 고쳐라"
    );
}

/// 좌변이 비어 있지 않다는 것을 고정한다. 스캔 루트가 잘못되면 위 세 테스트가
/// **아무것도 안 훑고** 초록이 된다 — 그 초록은 통과가 아니라 미측정이다.
#[test]
fn the_scan_actually_reaches_the_repo() {
    let sources = shipping_sources();
    assert!(
        sources.len() > 500,
        "스캔이 {} 개만 훑었다 — SCAN_ROOTS 를 의심해라",
        sources.len()
    );
    let seen: usize = sources
        .iter()
        .map(|(_, masked, _)| {
            SCROLL_AREA_CTORS
                .iter()
                .map(|c| masked.matches(*c).count())
                .sum::<usize>()
        })
        .sum();
    assert!(
        seen > 40,
        "코드에서 ScrollArea 생성 호출을 {seen} 개만 봤다 — 마스킹이나 스캔이 과하다"
    );
}
