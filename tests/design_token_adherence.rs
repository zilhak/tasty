//! 시각 토큰 준수 가드 — 간격/폰트 크기/선 굵기 리터럴의 재유입을 차단한다.
//!
//! `CLAUDE.md` "UI 디자인 (필수)" 가 강제하는 네 축 중 색은 clippy
//! `disallowed_methods`(deny)가 컴파일 단계에서 막고, 나머지 셋(간격·폰트 크기·선
//! 굵기)을 이 파일이 맡는다. 셋 다 같은 형태(`<접두>(<숫자>`)라 판정기
//! [`violating_prefix`] 하나를 공유한다 — 토큰을 넘기는 정상 코드
//! (`Stroke::new(th.border_width.value(), ..)`)는 숫자로 시작하지 않아 걸리지 않는다.
//!
//! design-tokens-02 가 `add_space`/`Margin` 의 off-grid 리터럴을 typed 헬퍼
//! (`vspace`/`hspace`/`margin_all`/`margin_sym` + `th.spacing_*` / `STRUCT_GAP_*`)로
//! 이식했다. 이 가드는 그 결과를 되돌림 없이 유지한다 — 소스에 `add_space(8.0)` 이나
//! `Margin::same(12)` 같은 **인라인 숫자 리터럴**을 다시 넣으면 `cargo test --workspace`
//! 에서 fail 한다. 통합 테스트라 **자동 실행은 `check-headless` 잡에서만** 일어나고 기본
//! 조합에는 채널이 없다(컴파일은 자동 잡의 clippy `--all-targets` 가 본다 —
//! `docs/dev-guide/ci-gates.md`). 소스를 런타임에 스캔하는
//! 가드에게 컴파일만으로는 아무것도 검사되지 않으므로, 리터럴을 건드렸으면 직접
//! 돌려야 한다. 선례: `tests/cli_naming_count_drift.rs`.
//!
//! **스코프 밖(의도적)**: `const NAME: LogicalPx = LogicalPx(N)` 같은 **명명 구조 상수**는
//! 금지하지 않는다 — 그게 구조값(사이드바 폭·카드 크기·control nudge)의 *권장* 해결책이다
//! — structural 값은 magic number 대신 명명 const 로 둬야 의미가 이름에 남는다.
//! 이 가드가 잡는 건 4px 리듬 자리에 박힌 인라인 리터럴뿐이다.
//!
//! # 가드가 막지 못하는 것 (실측 — 회피 변이로 확인)
//!
//! 소스 텍스트 스캔이라 **한계가 있고, 그 한계를 여기 적어 둔다.** 이 목록이 없으면
//! "가드가 폰트/간격 축을 강제한다" 는 문서 문장이 사실보다 강해진다 — 실제로 이
//! 파일의 앞 라운드가 그 상태였다(`tests/let_underscore_documented.rs` 와 같은 방식).
//! 아래는 전부 **컴파일되고 `cargo fmt --check` 도 통과하는** 형태다.
//!
//! | 형태 | 예 | 왜 못 잡나 |
//! |---|---|---|
//! | 괄호 감싸기 | `.size((13.0))` | 접두 뒤 첫 문자가 `(` 라 숫자 검사에 걸리지 않는다 |
//! | 인라인 주석 | `.size(/*x*/ 13.0)` | 같음 |
//! | 개행 + 주석 줄 | `.size(` ⏎ `// c` ⏎ `13.0)` | 다음 줄 프로브가 **한 줄만** 본다. 그 줄이 주석이면 그대로 빠져나간다 |
//! | 형변환 | `.size(f32::from(13u8))` | 첫 문자가 `f` |
//! | 매크로 | `.size(sz!())` | 같음 |
//! | 단항 마이너스 | `add_space(-8.0)` | 첫 문자가 `-` |
//! | 명명 const 경유 | `const S: f32 = 10.5; .size(S)` | **설계상 허용**(위 "스코프 밖") — 단 **값이 토큰과 같은 const** 는 예외다. 그건 스케일 밖 값이 아니라 토큰의 복사본이라 `src/design_token_guard.rs` 가 따로 잡는다 |
//! | 변수 경유 | `let s = 13.0; .size(s)` | 같은 이유. 값의 출처를 소스 스캔으로 따라갈 수 없다 |
//! | egui 기본 스타일 | `Style::default()` 가 주는 크기 | 소스에 숫자가 없다 |
//!
//! 이것들을 닫으려면 텍스트 스캔이 아니라 **AST/HIR 수준 lint** 가 필요하다. 여기서
//! 막는 것은 *레포에 실제로 나타나는 관용구*이고, 위 목록은 "우연히 새는 것" 이 아니라
//! **의도적으로 우회해야 나오는 형태**다 — 가드의 목적(무심코 되돌리는 것을 막는다)은
//! 그 선까지다.

use std::path::{Path, PathBuf};

/// 간격 스캔 대상 (repo-relative). host UI 계층 + 갤러리 + 위젯 크레이트.
const SCAN_ROOTS: &[&str] = &[
    "src/view",
    "src/adapters/ui",
    "src/gfx/gpu",
    "crates/tasty-gallery/src",
    "crates/tasty-ui-widgets/src",
    // Theme 를 egui 로 잇는 어댑터 크레이트. `stroke1()` 같은 헬퍼가 여기 살아서
    // 리터럴이 들어오면 호출부 전체로 전파된다. 편입 시점 위반 0 이라 allowlist
    // 없이 그대로 넣었다 — 공짜 커버리지다.
    "crates/tasty-egui-theme/src",
];

/// primitive 색 필드 접근 스캔 대상 — host UI 계층 + 위젯 크레이트. semantic 접근자
/// 전수 이식이 끝난 범위다(남은 건수는 이 파일의
/// [`no_primitive_color_field_access_in_host_ui`] 가 실시간으로 답한다 — 여기 숫자로
/// 적으면 다음 병합에 썩는다). 위젯 크레이트도 primitive 절대 불가
/// (ADR-0033): 재사용 위젯이라도 색은 semantic role 접근자로만 읽는다. 제외:
/// - `crates/tasty-gallery/src`: 팔레트 데모가 raw primitive 를 의도적으로 노출.
const COLOR_SCAN_ROOTS: &[&str] = &[
    "src/view",
    "src/adapters/ui",
    "src/gfx/gpu",
    "crates/tasty-ui-widgets/src",
];

/// raw 픽토그래픽 글리프 스캔 대상 — **host 전용**(widgets/gallery 미포함). gallery
/// specimen 은 ↑↓↵→◀▶ 를 대량 사용하므로 SCAN_ROOTS 재사용 시 오검출된다(연구 §3).
/// 플러그인(`crates/tasty-plugin-*`)도 미포함 — S-11 과 비중첩.
const GLYPH_SCAN_ROOTS: &[&str] = &["src/view", "src/adapters/ui", "src/gfx/gpu"];

/// 호출부 길이 리터럴 축의 스캔 모수 — 본체 UI 전부에서 **갤러리만** 뺀다.
///
/// 갤러리는 `ctx.set_zoom_factor(ui_scale)` 로 egui 전역에 배율을 걸어 리터럴도 함께
/// 커지므로 거기서는 결함이 아니다(`docs/adr/0135-…`). 제외의 사유가 그것 하나이므로
/// 제외도 그것 하나로 적는다 — 남기는 쪽(`"src/"` 같은 경로 모양)으로 적으면 갤러리가
/// 아닌 위젯 크레이트까지 사유 없이 함께 빠진다.
const LENGTH_SETTER_SCAN_ROOTS: &[&str] = &[
    "src/view",
    "src/adapters/ui",
    "src/gfx/gpu",
    "crates/tasty-ui-widgets/src",
    "crates/tasty-egui-theme/src",
];

/// 길이를 직접 받는 egui 호출부. 이 접두 뒤에 숫자가 오면 그 길이는 `Theme` 밖이라
/// **본체에서 `ui_scale` 을 안 탄다**(본체는 egui `zoom_factor` 를 1.0 으로 고정하고
/// 배율을 `zoomed()` 로만 먹인다 — ADR-0135).
const LENGTH_SETTER_PREFIXES: &[&str] = &[
    "set_min_width(",
    "set_max_width(",
    "set_min_height(",
    "set_max_height(",
    "min_height(",
    "max_height(",
    "exact_width(",
    "exact_height(",
    "desired_width(",
    "desired_height(",
];

/// **한시 목록 — 줄어들기만 한다.** [`ALLOWLIST_PREFIXES`] 와 성격이 다르므로 합치지
/// 않는다(`tests/layering.rs` 가 같은 이유로 세 목록을 갈라 둔다).
///
/// - `ALLOWLIST_PREFIXES` 는 **정책** 면제다 — 사유가 "이 경로는 검사 범위 밖이다" 라
///   덮을 것이 지금 없어도 남는다(ADR-0150).
/// - 이 목록의 사유는 전부 한 가지다: **"이 자리의 값이 `field_width_*` tier 밖이라
///   아직 토큰이 없다."** 그 사유가 사라지면(= 리터럴이 없어지면) 항목도 사라져야 하고,
///   아래 단언이 그것을 양방향으로 강제한다.
///
/// 여섯 자리를 지금 고치지 않는 이유는 값이 바뀌기 때문이다 — tier 를 넓힐지 이 여섯을
/// tier 로 스냅할지는 디자인 판정이고, 자리마다 이름을 주면 드리프트가 정당해 보여
/// 눈에 안 보이게 된다(ADR-0126). **이 가드가 답하는 것은 그 질문이 아니라 "일곱째가
/// 새로 들어오는가" 다.**
const LENGTH_SETTER_BASELINE: &[(&str, &str, &str)] = &[
    (
        "src/view/settings/ui/file_handler_tab/extension_mapping.rs",
        "desired_width(",
        "120.0",
    ),
    (
        "src/view/settings/ui/file_handler_tab/handlers.rs",
        "desired_width(",
        "80.0",
    ),
    (
        "src/view/settings/ui/file_handler_tab/handlers.rs",
        "desired_width(",
        "120.0",
    ),
    (
        "src/view/settings/ui/file_handler_tab/handlers.rs",
        "desired_width(",
        "240.0",
    ),
    (
        "src/view/settings/ui/keybindings_tab/plugins.rs",
        "desired_width(",
        "180.0",
    ),
    (
        "src/view/settings/ui/tabs/appearance.rs",
        "desired_width(",
        "190.0",
    ),
];

/// 코퍼스 하한 — 스캔이 비면 위반도 0 이 되어 가드가 조용히 통과한다.
/// 실측 2026-09-07: **186**(하한 150, 여유 36). 루트별로는 `src/view` 55 ·
/// `src/adapters/ui` 90 · `src/gfx/gpu` 8 · `crates/tasty-ui-widgets/src` 32 ·
/// `crates/tasty-egui-theme/src` 1 이다.
///
/// 이 수 아래로 내려가면 **큰 루트 하나가 통째로 안 걷힌 것**이다 — 위 분해에서 36 을
/// 넘는 루트는 셋뿐이라 그중 하나가 빠져야 여기 걸린다. 작은 둘(8·1)이 사라지는 것은
/// 이 하한이 **못 본다**: 그쪽은 루트마다 따로 확인하는
/// `the_scan_roots_are_directories` 의 몫이고, 지금 그 시험은 `src/gfx/gpu` 하나만
/// 걷어 본다. 나머지 넷은 어느 쪽도 개별로는 안 본다.
const MIN_LENGTH_SETTER_SCANNED_FILES: usize = 150;

/// Theme 의 primitive(Catppuccin) 색 필드명. semantic 접근자(`text_primary()` 등)가 아닌
/// 평면 필드 직접 접근(`th.blue`/`theme.surface0`)을 host UI 에서 금지하기 위한 목록.
/// `text` 는 `text_primary`/`text_muted` 등 semantic 접근자의 접두라 경계 검사로 가른다.
const PRIMITIVE_COLOR_FIELDS: &[&str] = &[
    "crust",
    "mantle",
    "base",
    "surface0",
    "surface1",
    "surface2",
    "overlay0",
    "overlay1",
    "overlay2",
    "text",
    "subtext1",
    "subtext0",
    "blue",
    "green",
    "red",
    "yellow",
    "peach",
    "mauve",
    "teal",
    "sky",
    "lavender",
    "flamingo",
    "pink",
    "maroon",
    "rosewater",
];

/// 금지 패턴: `<prefix>` 뒤 (공백 무시) 첫 문자가 숫자면 인라인 리터럴로 본다.
/// typed 헬퍼(`margin_all(th.spacing_md)`)·토큰(`spacing_xs.value()`)은 숫자로 시작하지
/// 않으므로 걸리지 않는다.
///
/// 폰트 크기는 `th.font_size_*` 또는 component 접근자(`th.badge_font_size()` 등),
/// 선 굵기는 `th.border_width`/`focus_ring_width`/`icon_stroke_width` 로 바꾼다.
const FORBIDDEN_PREFIXES: &[&str] = &[
    "add_space(",
    "Margin::same(",
    "Margin::symmetric(",
    "inner_margin(",
    "FontId::proportional(",
    "FontId::monospace(",
    // `proportional`/`monospace` 는 **이것의 얇은 래퍼**다. 셋째 경로를 열어두면
    // 앞의 둘을 막은 의미가 없다 — 같은 폰트를 `FontId::new(13.0, Proportional)`
    // 로 그대로 만들 수 있다. 전역 폰트 치환(`Style::text_styles`)도 결국 `FontId`
    // 를 만들어야 하므로 이 셋을 다 막으면 그 경로의 리터럴도 함께 닫힌다.
    "FontId::new(",
    "Stroke::new(",
    // `RichText::new(..).size(13.5)` — `FontId::*` 와 **같은 결함의 다른 표기**다.
    // egui 에서 폰트 크기를 지정하는 두 경로가 이 둘이라 한쪽만 막으면 다른 쪽으로
    // 그대로 재유입된다. `Spinner::size()`(위젯 지름)도 같은 이름이라 함께 걸리는데,
    // 그건 폰트가 아니므로 아래 allowlist 로 그 자리만 면제한다.
    ".size(",
    // 아이콘 글리프 **크기** 축. `icon_glyph_size_xs 12` · `_sm 14` · `_row_action 15` ·
    // `_md 16` 이 실재하므로 이 자리의 리터럴은 "토큰이 없어서" 가 아니라 **토큰 우회**다.
    // 스케일 밖 값(13 · 17 · 22 · 26 · 28 · 30)은 ADR-0126 과 같게 명명 const 로 둔다 —
    // 그래서 이 접두에는 allowlist 항목이 없다(이식을 먼저 끝내고 넣었다).
    ".image(",
    // 코너 반경 축. **접두가 둘인 이유가 이 축의 교훈이다** — 다수가
    // `.corner_radius(egui::CornerRadius::same(12))` 로 한 겹 감싸여 있어서
    // `.corner_radius(` 뒤에 숫자가 오지 않는다. 한쪽만 넣으면 축의 절반이 열린 채로
    // 닫혔다고 읽힌다(`Margin::same(` 을 `inner_margin(` 과 따로 넣은 것과 같은 이유).
    //
    // 스케일은 `corner_radius_sm 2` · `corner_radius 4` · `corner_radius_lg 8` 이고
    // DTCG 도 `radius-2/4/8/full` 뿐이다. 밖의 값(3 · 6 · 12)은 스냅하지 말고
    // ADR-0126 대로 사유를 적은 명명 const 로 둔다 —
    // `tasty_ui_widgets::tokens` 의 `BOOT_CHROME_CORNER_RADIUS` ·
    // `BOOT_CARD_CORNER_RADIUS` · `TAG_PILL_CORNER_RADIUS` 가 그것이다.
    // 전부 0 이면 `CornerRadius::ZERO`(`Margin::ZERO` 와 같은 관례).
    //
    // **이 축은 굵기 축과 대가가 다르다**: `corner_radius*` 는 `zoomed()` 를 타고
    // `border_width` 는 안 탄다. 그래서 반경을 명명 const 로 빼면 그 자리만 배율에서
    // 고정된다 — 스케일 밖 여섯 자리가 감수한 값이다. 이식 후 allowlist 항목 0.
    ".corner_radius(",
    "CornerRadius::same(",
];

/// 숫자 인자 검사로는 잡히지 않는 **금지 형태**. 접두 규칙은 "접두 뒤 첫 문자가
/// 숫자인가" 를 보는데, 구조체 리터럴은 필드명이 먼저 와서 그 검사를 그대로
/// 빠져나간다(`egui::Stroke { width: 2.0, color }`). 실제 회피 변형 검증에서 이
/// 형태로 한 번 뚫렸다.
///
/// 여기 있는 형태는 **값과 무관하게** 금지다 — 토큰을 넣든 리터럴을 넣든 쓰지 말고
/// 생성자(`Stroke::new(<토큰>, ..)` · `FontId::proportional(<토큰>)`)를 쓴다. 그래야
/// 접두 규칙이 계속 유효하다.
///
/// 이 두 이름은 **반환 타입 시그니처**(`fn stroke1(..) -> egui::Stroke {`)로도 나타나는데,
/// 그건 구조체 리터럴이 아니다. 아래 `->` 규칙이 그 둘을 가르므로 **allowlist 항목이
/// 필요 없다** — 면제가 아니라 판별로 푼다.
const FORBIDDEN_FORMS: &[&str] = &["Stroke {", "FontId {"];

/// 스캔 예외 — **(경로, 접두, 구조 술어)** 셋이다.
///
/// 파일 통째로 빼면 그 파일이 *다른* 형태의 위반을 새로 들여도 잡히지 않는다. 그래서
/// 예전부터 (경로, 접두) 쌍이었는데, **접두는 이름이지 구조가 아니라서** 그것만으로는
/// 여전히 넓다 — `prim_spinner.rs` 의 `.size(` 면제는 위젯 지름을 빼주려던 것인데
/// 같은 파일의 **폰트** `.size()` 까지 덮고 있었다(Gate4 J5).
///
/// 세 번째 칸이 그 구조다: `Some(marker)` 면 **같은 줄에서 접두 앞에 그 문자열이
/// 있을 때만** 면제한다. 지름 면제는 `Some("Spinner::new()")` 라, 수신자가 스피너가
/// 아닌 `.size(` 는 같은 파일에서도 그대로 잡힌다. `None` 은 그 파일 전체에서 그
/// 접두를 면제한다 — 파일의 존재 이유 자체가 그 접두일 때만 쓴다(typed 헬퍼의 구현).
///
/// 면제의 **전제가 무너지면 가드가 알아채야** 한다:
/// [`the_spinner_exemption_discriminates_receiver`] 가 그것을 변이로 확인한다.
const ALLOWLIST_PREFIXES: &[(&str, &str, Option<&str>)] = &[
    // typed 간격 헬퍼의 구현 자체 — 내부에서 raw `add_space`/`Margin` 을 호출하고
    // doc 주석에 예시 리터럴을 담는다. 폰트·선굵기는 면제 대상이 아니다.
    ("crates/tasty-ui-widgets/src/spacing.rs", "add_space(", None),
    (
        "crates/tasty-ui-widgets/src/spacing.rs",
        "Margin::same(",
        None,
    ),
    (
        "crates/tasty-ui-widgets/src/spacing.rs",
        "Margin::symmetric(",
        None,
    ),
    (
        "crates/tasty-ui-widgets/src/spacing.rs",
        "inner_margin(",
        None,
    ),
    // spinner 크기 카탈로그 — 이 specimen 의 **내용 자체가** 여러 지름을 나란히
    // 보이는 것이다. `.size()` 는 여기서 폰트가 아니라 위젯 지름이고, 값이 하나로
    // 수렴하면 specimen 이 성립하지 않는다.
    //
    // 면제는 **수신자가 스피너인 줄에만** 걸린다 — 같은 specimen 이 라벨 텍스트에도
    // `RichText::..size()` 를 쓰기 때문이다. 파일만 보고 빼면 그 폰트 자리가 함께
    // 빠져나간다.
    (
        "crates/tasty-gallery/src/catalog/components/prim_spinner.rs",
        ".size(",
        Some("Spinner::new()"),
    ),
];

/// 면제의 **전제 검사** — 면제가 "이 파일" 이 아니라 "이 파일의 이 구조" 에 걸려
/// 있는지 가른다. 전제가 무너진 줄(수신자가 스피너가 아닌 `.size(`)에서 면제가
/// 풀리지 않으면 이 테스트가 죽는다.
///
/// 실제 소스에 변이를 주입해 확인한 판정 셋을 그대로 고정한 것이다: 스피너 지름은
/// 통과, 같은 파일의 `RichText` 폰트는 적발, 다른 파일의 스피너도 적발.
#[test]
fn the_spinner_exemption_discriminates_receiver() {
    const SPIN: &str = "crates/tasty-gallery/src/catalog/components/prim_spinner.rs";

    // 지름 — 면제된다.
    assert!(exempt(
        SPIN,
        ".size(",
        "            Spinner::new().size(12.0).show(ui, theme);"
    ));
    // 같은 파일의 폰트 — 수신자가 다르므로 면제되지 않는다.
    assert!(!exempt(
        SPIN,
        ".size(",
        "            ui.label(RichText::new(\"x\").size(13.0));"
    ));
    // 다른 파일의 스피너 — 면제는 파일 한정이므로 걸린다.
    assert!(!exempt(
        "crates/tasty-gallery/src/catalog/components/empty_surface.rs",
        ".size(",
        "    Spinner::new().size(30.0);"
    ));
    // 마커가 접두 **뒤**에 있으면 수신자가 아니다 — 주석으로 면제를 살 수 없다.
    assert!(!exempt(
        SPIN,
        ".size(",
        "    x.size(13.0); // Spinner::new()"
    ));
    // 구조 술어가 없는 면제(typed 헬퍼의 구현)는 그 파일 전체에서 걸린다.
    assert!(exempt(
        "crates/tasty-ui-widgets/src/spacing.rs",
        "add_space(",
        "    ui.add_space(8.0);"
    ));
}

/// **면제마다 그 면제를 겨냥한 변이가 붙는다.** `spacing.rs` 의 네 항목은 구조 술어가
/// `None` 이라 파일 전체에서 그 접두를 면제한다 — 가장 넓은 형태다. 그래서 "면제 창
/// 안쪽에 *다른 축의* 진짜 위반을 심으면 잡히는가" 를 여기서 묻는다.
///
/// 답은 잡혀야 한다. `None` 면제는 **접두 하나**에만 걸리지 파일에 걸리지 않는다.
#[test]
fn the_typed_helper_exemption_does_not_cover_other_axes() {
    const SP: &str = "crates/tasty-ui-widgets/src/spacing.rs";

    // 면제된 접두 — 그 파일의 존재 이유다.
    assert!(exempt(SP, "add_space(", "    ui.add_space(8.0);"));
    assert!(exempt(SP, "Margin::same(", "    Margin::same(12.0)"));

    // 같은 파일의 **다른 축**은 면제되지 않는다.
    for form in [
        ".size(",
        "FontId::proportional(",
        "Stroke::new(",
        "inner_margin(",
    ] {
        let exempted = exempt(SP, form, "    x");
        assert_eq!(
            exempted,
            form == "inner_margin(",
            "`{form}` 의 면제 여부가 뒤집혔다 — spacing.rs 면제는 등록된 네 접두에만 \
             걸려야 한다"
        );
    }

    // 그리고 실제 판정에서도 잡힌다.
    assert_eq!(
        violating_prefix(SP, "    ui.label(RichText::new(x).size(13.0));", ""),
        Some(".size(")
    );
    assert_eq!(
        violating_prefix(SP, "    Stroke::new(2.0, c)", ""),
        Some("Stroke::new(")
    );
}

/// 반환 타입 시그니처를 넘기는 규칙도 **면제**다(형태가 있는데 안 잡는다). 그 창
/// 안쪽에 진짜 구조체 리터럴을 심으면 잡혀야 한다.
///
/// **의도된 false negative 도 함께 고정한다**: 시그니처 줄은 잡지 않는 것이 옳다.
/// 나중에 누가 이 규칙을 넓히면 그 결정이 여기서 실패로 드러난다.
#[test]
fn the_return_signature_skip_only_covers_signatures() {
    // 시그니처 — 의도적으로 안 잡는다.
    assert_eq!(
        violating_prefix("src/view/x.rs", "fn s(..) -> egui::Stroke {", ""),
        None
    );
    assert_eq!(
        violating_prefix("src/view/x.rs", "fn f(..) -> egui::FontId {", ""),
        None
    );

    // 같은 줄 모양이지만 구조체 리터럴 — 잡힌다.
    assert_eq!(
        violating_prefix(
            "src/view/x.rs",
            "    let s = egui::Stroke { width: 1.0 };",
            ""
        ),
        Some("Stroke {")
    );
    // 시그니처 뒤에 리터럴이 이어 붙은 형태도 잡힌다 — `->` 하나가 줄 전체를 사면하지
    // 않는다.
    assert_eq!(
        violating_prefix(
            "src/view/x.rs",
            "fn s() -> egui::Stroke { egui::Stroke { width: 1.0 } }",
            ""
        ),
        Some("Stroke {")
    );
}

/// 반경 축의 **두 접두가 서로를 대신하지 못한다**는 것을 고정한다.
///
/// 이 축은 셋을 세 번 틀리게 셌다(3 → 7 → 9). 3 은 `.corner_radius(` 뒤에 숫자가
/// 오는 형태만 봐서, 7 은 모수를 스캔 루트로 잡아 `src/gfx/gpu/boot_error.rs` 를 빼서
/// 나온 값이다. **패턴을 고쳐도 모수가 틀리면 다시 적게 센다** — 그래서 접두 하나로
/// 닫혔다고 읽히는 길을 여기서 막는다.
///
/// 접두를 하나로 줄이면 이 테스트가 죽는다. 그것이 이 테스트의 용도다.
#[test]
fn the_corner_radius_axis_needs_both_prefixes() {
    const F: &str = "src/view/x.rs";

    // 감싸인 형태 — `.corner_radius(` 뒤는 `e` 라 그 접두로는 절대 안 잡힌다.
    assert_eq!(
        violating_prefix(
            F,
            "        .corner_radius(egui::CornerRadius::same(12))",
            ""
        ),
        Some("CornerRadius::same(")
    );
    // 벗은 형태 — `CornerRadius::same(` 이 줄에 아예 없다.
    assert_eq!(
        violating_prefix(F, "        .corner_radius(4.0)", ""),
        Some(".corner_radius(")
    );

    // 처방된 세 출구는 전부 통과한다 — 토큰 · 명명 const · 명명 ZERO.
    for ok in [
        "        .corner_radius(th.corner_radius.value())",
        "        .corner_radius(tasty_ui_widgets::tokens::BOOT_CARD_CORNER_RADIUS)",
        "        .corner_radius(egui::CornerRadius::ZERO)",
    ] {
        assert_eq!(
            violating_prefix(F, ok, ""),
            None,
            "처방 형태가 잡혔다: {ok}"
        );
    }
}

/// 모수가 **파일 열거가 아니라 디렉토리**여야 하는 이유를 고정한다.
///
/// `src/gfx/gpu` 는 예전에 `shell_setup.rs` 한 파일로 등재돼 있었다. 그 뒤 같은
/// 디렉토리에 `boot_error.rs` 가 생겼고, **이미 등재된 접두**(`Stroke::new(`)를 쓰는
/// 위반 넷을 조용히 들여왔다 — 가드는 초록이었고 축은 열려 있었다. 파일 열거는 새
/// 파일을 기본 제외로 만든다.
///
/// 실측으로 확인한 형태다: 모수를 파일로 되돌린 채 위반을 심으면
/// `no_inline_visual_token_literals` 는 **통과한다.** 그 조건에서 유일하게 우는 것이
/// 이 테스트다.
#[test]
fn the_gpu_scan_root_is_a_directory_not_a_file() {
    assert!(
        SCAN_ROOTS.contains(&"src/gfx/gpu"),
        "gpu UI 계층의 스캔 루트가 디렉토리가 아니다 — 파일 단위로 되돌리면 그 \
         디렉토리에 새로 생기는 UI 파일이 기본 제외가 된다"
    );
    for roots in [SCAN_ROOTS, COLOR_SCAN_ROOTS, GLYPH_SCAN_ROOTS] {
        assert!(
            !roots.iter().any(|r| r.ends_with(".rs")),
            "스캔 루트에 개별 `.rs` 파일이 있다: {roots:?}"
        );
    }
    // 그리고 그 루트가 실제로 파일을 걷어 온다 — 경로가 틀리면 조용히 0 이 된다.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    gather_rs_files(&root.join("src/gfx/gpu"), &mut files);
    assert!(
        files.len() >= 2,
        "src/gfx/gpu 에서 걷은 .rs 가 {} 개다 — 경로를 확인할 것",
        files.len()
    );
}

/// `rel` 파일의 `line` 에서 `form` 이 면제되는가. 구조 술어가 있으면 **그 줄에서
/// 접두보다 앞에** 마커가 있어야 한다.
fn exempt(rel: &str, form: &str, line: &str) -> bool {
    ALLOWLIST_PREFIXES.iter().any(|(path, prefix, marker)| {
        *path == rel
            && *prefix == form
            && match marker {
                None => true,
                Some(m) => match (line.find(m), line.find(form)) {
                    (Some(mi), Some(fi)) => mi < fi,
                    _ => false,
                },
            }
    })
}

/// `line` 에 금지 prefix + 숫자 인자가 있으면 매칭된 prefix 를 돌려준다.
/// `rel` 파일에 대해 그 접두가 [`ALLOWLIST_PREFIXES`] 에 있으면 건너뛴다.
fn violating_prefix(rel: &str, line: &str, next_line: &str) -> Option<&'static str> {
    for &form in FORBIDDEN_FORMS {
        if exempt(rel, form, line) {
            continue;
        }
        let mut from = 0;
        while let Some(idx) = line[from..].find(form) {
            let start = from + idx;
            // `fn stroke1(..) -> egui::Stroke {` / `fn mono(..) -> egui::FontId {`
            // 같은 **반환 타입**은 구조체 리터럴이 아니다. 형태 앞의 경로 한정자
            // (`egui::`)를 걷어낸 뒤 `->` 로 끝나면 시그니처이므로 넘긴다 —
            // `crates/tasty-egui-theme` 를 스캔에 넣을 때 실제로 이 위양성이 났다
            // (그때는 `egui::` 때문에 `->` 검사가 빗나갔다).
            let head = line[..start]
                .trim_end_matches(|c: char| c.is_alphanumeric() || c == '_' || c == ':');
            if !head.trim_end().ends_with("->") {
                return Some(form);
            }
            from = start + form.len();
        }
    }
    for &prefix in FORBIDDEN_PREFIXES {
        if exempt(rel, prefix, line) {
            continue;
        }
        let mut from = 0;
        while let Some(idx) = line[from..].find(prefix) {
            let after = &line[from + idx + prefix.len()..].trim_start();
            // 접두가 줄 끝에서 열린 채 끝나면(`.size(` 뒤에 아무것도 없음) 인자는 다음
            // 줄에 있다. 한 줄만 보면 `.size(\n    13.0)` 형태로 그냥 빠져나간다 —
            // 실제로 이 회피 변형에 한 번 뚫렸다. `rustfmt` 가 짧은 호출은 한 줄로
            // 되돌리므로 레포에 들어오긴 어렵지만, 가드의 회피 난이도를 포매터
            // 하나에만 의존시키지 않는다.
            let probe = if after.is_empty() {
                next_line.trim_start()
            } else {
                after
            };
            if matches!(probe.chars().next(), Some(c) if c.is_ascii_digit()) {
                return Some(prefix);
            }
            from += idx + prefix.len();
        }
    }
    None
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `line` 에 `th.<primitive>` / `theme.<primitive>` 평면 필드 접근이 있으면 그 표현을 돌려준다.
/// 앞뒤 경계를 검사해 `th.text_primary()`(semantic) 나 `mytheme.blue` 오검출을 배제한다.
fn violating_color(_rel: &str, line: &str, _next: &str) -> Option<String> {
    for receiver in ["th.", "theme."] {
        let mut from = 0;
        while let Some(idx) = line[from..].find(receiver) {
            let start = from + idx;
            // receiver 앞 문자가 word char 면 `xtheme.` 같은 부분매치 → 스킵.
            let before_ok =
                start == 0 || !is_word_char(line[..start].chars().next_back().unwrap_or(' '));
            let after = &line[start + receiver.len()..];
            if before_ok {
                for &field in PRIMITIVE_COLOR_FIELDS {
                    if let Some(rest) = after.strip_prefix(field) {
                        // 필드명 뒤 문자가 word char 면 semantic 접근자(text_primary 등) → 스킵.
                        let next = rest.chars().next();
                        if !matches!(next, Some(c) if is_word_char(c)) {
                            return Some(format!("{receiver}{field}"));
                        }
                    }
                }
            }
            from = start + receiver.len();
        }
    }
    None
}

/// `target` 하위 `.rs` 파일을 모아, 각 라인에 `detect(rel, line)` 을 적용해 위반을
/// 수집한다. 주석 라인(`//`)은 스킵 — 파일 단위 면제는 없다([`ALLOWLIST_PREFIXES`]).
fn collect_violations(
    root: &Path,
    target: &str,
    detect: &dyn Fn(&str, &str, &str) -> Option<String>,
    out: &mut Vec<String>,
) {
    let path = root.join(target);
    let mut files = Vec::new();
    gather_rs_files(&path, &mut files);
    assert!(
        !files.is_empty(),
        "스캔 루트 `{target}` 에서 .rs 파일을 하나도 찾지 못했다 — 경로가 바뀌었거나 \
         읽기에 실패했다. 조용한 미스캔은 위양성보다 나쁘므로 여기서 실패시킨다."
    );
    for file in files {
        let rel = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        let contents = std::fs::read_to_string(&file).expect("소스 파일 read 실패");
        // 판정기가 "다음 줄" 을 볼 수 있어야 한다 — 호출 인자가 줄바꿈으로 넘어간
        // 형태(`.size(` 개행 `13.0)`)를 한 줄만 보고는 잡지 못한다.
        let lines: Vec<&str> = contents.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            // 주석 라인(// 로 시작)은 스킵 — doc/설명의 예시 리터럴 false positive 방지.
            if line.trim_start().starts_with("//") {
                continue;
            }
            let next = lines.get(i + 1).copied().unwrap_or("");
            if let Some(hit) = detect(&rel, line, next) {
                out.push(format!("  {}:{} — `{}`", rel, i + 1, hit));
            }
        }
    }
}

fn gather_rs_files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().map(|e| e == "rs").unwrap_or(false) {
            out.push(path.to_path_buf());
        }
        return;
    }
    let entries = std::fs::read_dir(path).unwrap_or_else(|e| {
        panic!(
            "스캔 대상 디렉토리를 읽을 수 없다: {} — {e}. 조용히 건너뛰면 가드가 \
             아무것도 검사하지 않은 채 통과한다.",
            path.display()
        )
    });
    for entry in entries.flatten() {
        gather_rs_files(&entry.path(), out);
    }
}

/// 링 토큰(`focus_ring_width`)이 **바** 자리에 쓰였는가. 두 토큰은 값이 둘 다 2 라
/// 서로 바꿔 써도 zoom 1 에서는 아무 일도 일어나지 않는다 — 그래서 조용히 섞인다.
/// 실제로 갤러리 9자리가 좌측 accent 바·탭 밑줄을 `focus_ring_width` 로 그리고
/// 있었다.
///
/// 판별은 **구조**로 한다: 바는 크기 벡터가 필요해 `vec2(<굵기>, <길이>)` 형태로
/// 나타나고, 링은 `Stroke::new(<굵기>, <색>)` 이라 크기 벡터를 만들지 않는다. 즉
/// **`vec2(` 안에 `focus_ring_width` 가 있으면 그건 링이 아니라 바**이고, 정본은
/// `tab_indicator_width` 다(값 2 로 동일, 대신 zoom 을 타지 않는다 — 호스트의 바가
/// 원래 그렇다).
///
/// **한계**: 굵기를 먼저 지역 변수에 담는 형태(`let bar_w = ..focus_ring_width..;`)
/// 나 좌표 산술(`pos2(x, bottom - ..)`)은 못 잡는다. 이식 당시 그 두 형태가 각각
/// 1자리씩 있었다.
fn ring_token_used_as_bar(rel: &str, lines: &[&str], out: &mut Vec<String>) {
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let Some(v) = line.find("vec2(") else {
            continue;
        };
        if line[v..].contains("focus_ring_width") {
            out.push(format!(
                "  {}:{} — `vec2(.. focus_ring_width ..)` 는 바다.                  `tab_indicator_width` 를 쓸 것",
                rel,
                i + 1
            ));
        }
    }
}

/// 접두 규칙이 구조적으로 못 보는 두 관용구 — **레포에 실제로 존재하던 형태**만
/// 닫는다. 회피 변이 목록의 나머지(괄호 감싸기·매크로 등)는 레포에 없고 의도적
/// 우회로만 나오므로, 모듈 doc 의 "가드가 막지 못하는 것" 에 한계로 적어 두었다.
#[test]
fn no_literal_margin_fields_or_item_spacing() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();
    for target in SCAN_ROOTS {
        let path = root.join(target);
        let mut files = Vec::new();
        gather_rs_files(&path, &mut files);
        assert!(
            !files.is_empty(),
            "스캔 루트 `{target}` 에서 .rs 파일을 찾지 못했다"
        );
        for file in files {
            let rel = file
                .strip_prefix(root)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            let contents = std::fs::read_to_string(&file).expect("소스 파일 read 실패");
            let lines: Vec<&str> = contents.lines().collect();
            margin_field_violations(&rel, &lines, &mut violations);
            item_spacing_violations(&rel, &lines, &mut violations);
            ring_token_used_as_bar(&rel, &lines, &mut violations);
        }
    }
    assert!(
        violations.is_empty(),
        "`Margin` 필드 / `item_spacing` 에 인라인 숫자 리터럴이 있다 — 토큰이나 명명 \
         const 로 바꿀 것:\n\
         · 4px 그리드 값 → `th.spacing_xs/sm/md/lg/xl.value() as i8`(Margin) 또는 \
         `.value()`(item_spacing)\n\
         · 1~4px 미세 간격 → `tasty_ui_widgets::tokens::STRUCT_GAP_1..4`\n\
         · 그리드 밖 값(9·10·11·14 등) → 사유를 적은 명명 const\n\
         · 0 은 그리드의 원점이라 규칙 안에 있다 — 다만 네 변이 전부 0 이면 \
         `Margin::ZERO`\n\
         · 링 토큰을 바에 쓰지 말 것 — 감싸는 획은 `focus_ring_width`, 한쪽 변에 \
         붙는 띠는 `tab_indicator_width`(둘 다 2 지만 zoom 거동이 다르다)\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_inline_visual_token_literals() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // 형태 규칙(`FORBIDDEN_FORMS`)은 숫자 인자와 무관하게 금지라 `<숫자>` 를 붙이지
    // 않는다 — 붙이면 "숫자만 빼면 통과" 로 잘못 읽힌다.
    let detect = |rel: &str, line: &str, next: &str| {
        violating_prefix(rel, line, next).map(|p| {
            if FORBIDDEN_FORMS.contains(&p) {
                format!(
                    "{p}` — 구조체 리터럴 형태 자체가 금지(생성자 \
                     `Stroke::new(<토큰>, ..)` · `FontId::proportional(<토큰>)` 를 쓴다)`"
                )
            } else {
                format!("{p}<숫자>")
            }
        })
    };
    let mut violations = Vec::new();
    for target in SCAN_ROOTS {
        collect_violations(root, target, &detect, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "인라인 시각 토큰 리터럴이 재유입됨 — 각 축의 대체 수단으로 바꿀 것:\n\
         · 간격/마진 → typed 헬퍼(vspace/hspace/margin_all/margin_sym) + th.spacing_* / \
         STRUCT_GAP_*\n\
         · 폰트 크기 → th.font_size_micro/caption/body/heading/max, 또는 역할을 이름에 \
         담은 component 접근자(th.badge_font_size() · th.tag_font_size() · \
         th.kbd_font_size() 등)\n\
         · 선 굵기 → th.border_width(1) / th.focus_ring_width(2) / th.icon_stroke_width(1.5)\n\
         대응 토큰이 없는 구조값은 명명 const(`const NAME: LogicalPx = LogicalPx(N)`)로 \
         승격한다 — 그건 스코프 밖이다:\n{}",
        violations.join("\n")
    );
}

/// 숫자 리터럴 토큰인가 — `12` · `12.0` · `1.5f32` 등. 부호는 호출부가 뗀다.
fn numeric_literal(tok: &str) -> Option<f32> {
    let t = tok.trim().trim_end_matches("f32").trim_end_matches("f64");
    if t.is_empty() || !t.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    t.parse::<f32>().ok()
}

/// **0 은 4px 그리드의 원점이지 예외가 아니다.** 그리드는 0 · 4 · 8 · 12 … 이고 0 은
/// 그 첫 항이다 — "여백 없음" 은 규칙을 벗어난 값이 아니라 규칙 안의 값이다. 그래서
/// 이 가드의 규칙은 처음부터 0 을 포함한다(면제 목록에 얹은 예외가 아니다). `top: 0` ·
/// `item_spacing = vec2(0.0, 0.0)` 은 egui 기본 간격을 끄는 **관용구**라 레포 전반에
/// 퍼져 있는데, 이걸 위반으로 세면 가드가 값이 아니라 관용구를 막게 된다.
///
/// **가드가 가르지 못하는 것**: `Margin { left: 0, right: 8, .. }` 같은 **부분 0** 이
/// "왼쪽은 붙인다" 는 의도인지 아직 안 채운 미완성인지는 소스에 신호가 없다. 네 변이
/// 전부 0 인 것은 가른다 — 그건 `Margin::ZERO` 라고 쓰면 되기 때문이다.
fn is_zero(v: f32) -> bool {
    v == 0.0
}

/// `egui::Margin { left: 14, .. }` 의 **필드 값** 위반. `Stroke {` 처럼 형태 자체를
/// 막을 수는 없다 — 네 변을 따로 주는 방법이 구조체 리터럴밖에 없기 때문이다
/// (`Margin::same`/`symmetric` 은 대칭 전용). 그래서 형태 대신 값을 본다.
///
/// `.inner_margin(` 접두 규칙은 여기 무력하다: `inner_margin(egui::Margin {` 는
/// 접두 뒤 첫 문자가 `e` 라 숫자 검사를 그대로 빠져나간다. 실제 회피 변이에서
/// 이 형태로 뚫렸고, 레포에 이미 열 자리가 있었다.
fn margin_field_violations(rel: &str, lines: &[&str], out: &mut Vec<String>) {
    let mut depth = 0usize;
    // 여러 줄 블록의 시작 줄과, 그 블록이 지금까지 본 필드 종류. 네 변이 전부 리터럴
    // 0 이면 `Margin::ZERO` 로 쓸 자리다.
    let mut block_start = 0usize;
    let mut zero_fields = 0usize;
    let mut other_fields = 0usize;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        if depth > 0 {
            if let Some((name, rest)) = t.split_once(':')
                && matches!(name.trim(), "left" | "right" | "top" | "bottom")
            {
                match numeric_literal(rest.trim().trim_end_matches(',')) {
                    Some(v) if is_zero(v) => zero_fields += 1,
                    Some(v) => {
                        other_fields += 1;
                        out.push(format!(
                            "  {}:{} — `Margin {{ {}: {v} }}`",
                            rel,
                            i + 1,
                            name.trim()
                        ));
                    }
                    None => other_fields += 1,
                }
            }
            if line.contains('}') {
                if zero_fields == 4 && other_fields == 0 {
                    out.push(format!(
                        "  {}:{} — 네 변이 전부 0 인 `Margin` 리터럴이다. `Margin::ZERO` 를 쓸 것",
                        rel,
                        block_start + 1
                    ));
                }
                depth = 0;
            }
            continue;
        }
        if line.contains("Margin {") && !line.trim_end().ends_with("Margin {") {
            // 한 줄 리터럴(`Margin { left: 14, .. }`) — 같은 줄에서 필드를 훑는다.
            for part in line.split(&['{', ',', '}'][..]) {
                if let Some((name, rest)) = part.split_once(':')
                    && matches!(name.trim(), "left" | "right" | "top" | "bottom")
                    && let Some(v) = numeric_literal(rest)
                    && !is_zero(v)
                {
                    out.push(format!(
                        "  {}:{} — `Margin {{ {}: {v} }}`",
                        rel,
                        i + 1,
                        name.trim()
                    ));
                }
            }
        } else if line.contains("Margin {") {
            depth = 1;
            block_start = i;
            zero_fields = 0;
            other_fields = 0;
        }
    }
}

/// `ui.spacing_mut().item_spacing.y = 8.0` 계열. **이 lane 이 두 자리에서 고친 바로
/// 그 형태**인데 재유입은 막혀 있지 않았다. `.x`/`.y` 개별 대입과 `= vec2(a, b)`
/// 통째 대입을 모두 본다(대입문이 다음 줄로 넘어가는 형태 포함).
fn item_spacing_violations(rel: &str, lines: &[&str], out: &mut Vec<String>) {
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let Some(eq) = line.find("item_spacing").map(|p| p + "item_spacing".len()) else {
            continue;
        };
        let Some(rel_eq) = line[eq..].find('=') else {
            continue;
        };
        // `==` 비교나 `item_spacing.x` 뒤의 다른 문장은 대상이 아니다.
        if line[eq + rel_eq..].starts_with("==") {
            continue;
        }
        // 대입문 전체를 `;` 까지 이어 붙인다 — `= egui::vec2(\n 0.0,\n 0.0);` 형태.
        let mut stmt = line[eq + rel_eq + 1..].to_string();
        let mut j = i;
        while !stmt.contains(';') && j + 1 < lines.len() && j - i < 3 {
            j += 1;
            stmt.push(' ');
            stmt.push_str(lines[j].trim());
        }
        for tok in stmt.split(&['(', ')', ',', ';'][..]) {
            if let Some(v) = numeric_literal(tok)
                && !is_zero(v)
            {
                out.push(format!("  {}:{} — `item_spacing = {v}`", rel, i + 1));
                break;
            }
        }
    }
}

/// 픽토그래픽 글리프 금지 범위(Tier-A) — UI 프로포셔널 폰트에서 tofu 나는 계열만
/// 좁게 잡는다: 이모지·픽토그래프(U+1F000–1FAFF) + 딩뱃(U+2700–27BF). 화살표(↑↓→↵)·
/// 기하도형(▲▼▾)·기술기호(⌘)·경고기호(⚠)는 kbd 힌트·라벨 구분자·콤보 affordance 로
/// 정당하게 쓰이므로 **범위 밖**(연구 §3). CJK·따옴표 등 텍스트도 자동 제외된다.
fn is_forbidden_pictographic(cp: u32) -> bool {
    (0x1F000..=0x1FAFF).contains(&cp) || (0x2700..=0x27BF).contains(&cp)
}

/// `line` 에서 픽토그래픽 글리프를 찾으면 그 표현을 돌려준다. **두 형태 모두** 검사:
/// ① 리터럴 코드포인트(누가 📂 를 그대로 붙여넣음) ② `\u{HEX}` 이스케이프 파싱 후 범위검사.
/// 주석 라인 skip 은 상위 `collect_violations` 가 처리한다.
fn violating_glyph(_rel: &str, line: &str, _next: &str) -> Option<String> {
    // ① 리터럴 char.
    for ch in line.chars() {
        let cp = ch as u32;
        if is_forbidden_pictographic(cp) {
            return Some(format!("U+{cp:04X} `{ch}`"));
        }
    }
    // ② `\u{HEX}` 이스케이프.
    let needle = "\\u{";
    let mut from = 0;
    while let Some(rel) = line[from..].find(needle) {
        let start = from + rel + needle.len();
        let Some(close_rel) = line[start..].find('}') else {
            break;
        };
        let hex = &line[start..start + close_rel];
        if let Ok(cp) = u32::from_str_radix(hex, 16)
            && is_forbidden_pictographic(cp)
        {
            return Some(format!("\\u{{{hex}}}"));
        }
        from = start + close_rel + 1;
    }
    None
}

#[test]
fn no_raw_pictographic_glyph() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();
    for target in GLYPH_SCAN_ROOTS {
        collect_violations(root, target, &violating_glyph, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "host UI 소스에 raw 픽토그래픽 글리프(이모지 U+1F000–1FAFF / 딩뱃 U+2700–27BF)가 \
         재유입됨 — SVG line-icon(`icons::*`)으로 바꿀 것. 리터럴·`\\u{{}}` 양형태 모두 금지:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_primitive_color_field_access_in_host_ui() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();
    for target in COLOR_SCAN_ROOTS {
        collect_violations(root, target, &violating_color, &mut violations);
    }
    assert!(
        violations.is_empty(),
        "host UI 에 primitive 색 필드 직접 접근이 재유입됨 — semantic 접근자\
         (accent_*/surface_*/text_*/border_* 등)로 바꿀 것:\n{}",
        violations.join("\n")
    );
}

/// 면제가 가리키는 경로가 **실재하는가** — 참조 무결성.
///
/// **초록은 "이 면제가 아직 필요하다" 가 아니다**(ADR-0150). 가리키는 것이 실재한다는
/// 것뿐이고, 실재해도 그 면제가 아무것도 안 덮고 있을 수 있다. 두 축을 섞으면 "안 덮으면
/// 지워라" 라는 틀린 처방이 참조 무결성의 옷을 입고 돌아온다.
///
/// 경로가 썩으면 면제는 조용히 아무 일도 안 하게 되는데, 목록에는 "여기는 원래 위반해도
/// 된다" 는 신호가 남는다. 판정과 그 양극성 회귀는 [`tasty_doc_guards::missing_referents`].
#[test]
fn allowlist_prefixes_point_at_paths_that_exist() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let missing = tasty_doc_guards::missing_referents(
        root,
        ALLOWLIST_PREFIXES.iter().map(|(rel, _, _)| *rel),
    );
    assert!(
        missing.is_empty(),
        "면제가 없는 경로를 가리킨다 — 옮겼으면 항목도 옮기고, 사라졌으면 항목을 지워라: {missing:?}"
    );
}

/// 호출부 길이 리터럴을 `(파일, 접두, 값)` 으로 모은다 — **줄 번호를 담지 않는다.**
/// 담으면 위쪽에 한 줄만 들어가도 한시 목록이 통째로 썩어, 목록이 결함과 무관하게
/// 흔들린다.
fn length_setter_literals(root: &Path) -> Vec<(String, &'static str, String)> {
    let mut out = Vec::new();
    let mut scanned = 0usize;
    for target in LENGTH_SETTER_SCAN_ROOTS {
        let path = root.join(target);
        let mut files = Vec::new();
        gather_rs_files(&path, &mut files);
        assert!(
            !files.is_empty(),
            "스캔 루트 `{target}` 에서 .rs 파일을 하나도 찾지 못했다 — 조용한 미스캔은 \
             위양성보다 나쁘다"
        );
        scanned += files.len();
        for file in files {
            let rel = file
                .strip_prefix(root)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            let contents = std::fs::read_to_string(&file).expect("소스 파일 read 실패");
            for line in contents.lines() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for &prefix in LENGTH_SETTER_PREFIXES {
                    let mut from = 0;
                    while let Some(idx) = line[from..].find(prefix) {
                        let start = from + idx;
                        from = start + prefix.len();
                        // `set_max_height(` 안의 `max_height(` 를 두 번 세지 않는다.
                        if line[..start].chars().next_back().is_some_and(is_word_char) {
                            continue;
                        }
                        let tail = &line[from..];
                        let value: String = tail
                            .chars()
                            .take_while(|c| c.is_ascii_digit() || *c == '.')
                            .collect();
                        // 숫자가 아니면 토큰을 넘긴 것이다 — 이 축이 원하는 형태다.
                        if value.is_empty() || !value.starts_with(|c: char| c.is_ascii_digit()) {
                            continue;
                        }
                        out.push((rel.clone(), prefix, value));
                    }
                }
            }
        }
    }
    assert!(
        scanned >= MIN_LENGTH_SETTER_SCANNED_FILES,
        "스캔 파일이 {scanned}개뿐이다(하한 {MIN_LENGTH_SETTER_SCANNED_FILES}) — \
         코퍼스가 비면 위반도 0 이다.\n\
         ★ 판별은 이미 위에 있다 — 이 순회는 [`LENGTH_SETTER_SCAN_ROOTS`] 를 하나씩 돌면서 \
         **루트마다** 빈 것을 따로 잡는다. 그러니 여기까지 왔다는 것은 어느 루트도 비지 \
         않았다는 뜻이고, 남는 갈래는 하나다: 루트들이 다 살아 있는데 합이 줄었다.\n\
         밖에서 세는 법:\n\
             find src/view src/adapters/ui src/gfx/gpu crates/tasty-ui-widgets/src \
                  crates/tasty-egui-theme/src -name '*.rs' | wc -l\n\
         2026-09-06 실측 185. 그 수도 같이 줄었으면 UI 코드가 정말 줄어든 것이다.\n\
         ★ 이 하한을 내려서 통과시키지 마라 — 이 축이 겨냥하는 것은 새로 들어오는 리터럴이라, \
         코퍼스가 좁아진 만큼 정확히 그만큼이 안 보이게 된다.\n\
         루트가 정당하게 옮겨 갔으면 하한이 아니라 [`LENGTH_SETTER_SCAN_ROOTS`] 를 고쳐라."
    );
    out.sort();
    out
}

/// 호출부에 길이 리터럴이 **새로** 들어오지 않는다.
///
/// 이 축의 결함은 토큰이 아니라 배율이다. 본체는 egui `zoom_factor` 를 1.0 으로 고정하고
/// UI 배율을 `Theme::with_colors_and_zoom` 의 `zoomed()` 로만 적용하므로, 호출부에 적힌
/// 숫자는 `ui_scale` 을 안 탄다 — 같은 리터럴이 갤러리에서는 결함이 아니다(ADR-0135).
///
/// **하한(`<= n`)이 아니라 집합 동등으로 본다.** 건수 고정은 빨개지기는 해도 무엇이
/// 늘었는지 말하지 않고, 한 방향(늘어남)만 보면 자리가 고쳐졌을 때 목록이 그대로 남아
/// "이미 부채" 라는 신호가 조용히 살아남는다. 동등은 두 방향을 다 이름으로 뱉는다.
#[test]
fn no_new_length_literal_at_call_sites() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let found = length_setter_literals(root);
    let mut expected: Vec<(String, &str, String)> = LENGTH_SETTER_BASELINE
        .iter()
        .map(|(rel, prefix, value)| ((*rel).to_string(), *prefix, (*value).to_string()))
        .collect();
    expected.sort();

    let show = |v: &[(String, &str, String)]| {
        v.iter()
            .map(|(r, p, val)| format!("  {r} — `{p}{val})`"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let added: Vec<_> = found
        .iter()
        .filter(|x| !expected.contains(x))
        .cloned()
        .collect();
    let gone: Vec<_> = expected
        .iter()
        .filter(|x| !found.contains(x))
        .cloned()
        .collect();

    assert!(
        added.is_empty(),
        "호출부에 길이 리터럴이 새로 들어왔다 — 본체는 egui zoom_factor 를 1.0 으로 \
         고정하므로 이 숫자는 `ui_scale` 을 안 탄다. `Theme` 의 `field_width_*` 등 \
         대응 토큰을 넘겨라(ADR-0135):\n{}",
        show(&added)
    );
    assert!(
        gone.is_empty(),
        "한시 목록이 가리키는 리터럴이 사라졌다 — 고쳤으면 목록에서도 지워라. 남겨 두면 \
         \"여기는 원래 부채\" 라는 신호가 아무것도 안 덮은 채 살아남는다:\n{}",
        show(&gone)
    );
}
