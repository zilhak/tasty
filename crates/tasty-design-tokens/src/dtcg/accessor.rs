//! DTCG 토큰 → `&Theme` 경유 접근자 생성 (component 치수·색, semantic 색).
//!
//! `generated::component` 의 raw const 는 zoom 을 모른다 (테마 불변 스케일).
//! 위젯이 그 const 를 직접 읽으면 `Theme::with_colors_and_zoom()` 의 zoom
//! resolve/제외 정책을 우회한다 — 그래서 component 토큰은 `&Theme` 경유
//! 접근자로만 노출한다 (`crates/tasty-type-appearance/src/generated_component.rs`).
//!
//! 여기서 만드는 텍스트는 `super::generate` 가 `Generated` 로 모은다. 파싱·분류·
//! const emit 은 `super` 에 남는다 — 이 모듈은 접근자 표와 그 emit 만 든다.

use super::{
    ThemeMode, Tier, Token, TokenSet, alias_target,
    duration_accessor::{emit_duration_accessor, resolve_duration_accessor},
};

/// semantic/component 치수 토큰의 전체 경로 ↔ `Theme`/`SIZING` 필드명.
/// `component.<name>` 키는 SIZING 이 그 component 토큰 전용 필드를 직접 보유하는
/// 경우(사이드바, titlebar OS 어포던스, 토스트, status-dot, spinner, tab
/// indicator), `semantic.<name>` 키는 위젯이 semantic 치수를 공유 소비하는
/// 일반 경로.
///
/// **순서 중요**: 같은 토큰 경로를 여러 `SIZING` 필드가 가리킬 때(tab-bar 류
/// zoom-제외 필드가 위젯과 같은 semantic 값을 재사용, 예: `control-height-tab`
/// ↔ `item_height_tab`/`tab_bar_height` 모두 대응) lookup 은 **먼저 나오는
/// 항목이 승자** — zoom 적용 위젯 필드를 zoom-제외 host-chrome 전용 필드보다
/// 앞에 둔다.
///
/// `tests/sizing_parity.rs` 의 가드는 이 표를 데이터로 순회한다 — 표 밖에서
/// 대응 pair 를 따로 하드코딩하지 않는다.
pub const SEMANTIC_DIM_TO_THEME_FIELD: &[(&str, &str)] = &[
    // spacing (4px 그리드 5단)
    ("semantic.space-xs", "spacing_xs"),
    ("semantic.space-sm", "spacing_sm"),
    ("semantic.space-md", "spacing_md"),
    ("semantic.space-lg", "spacing_lg"),
    ("semantic.space-xl", "spacing_xl"),
    // 보더/라운드
    ("semantic.border-width", "border_width"),
    ("semantic.focus-ring-width", "focus_ring_width"),
    ("semantic.radius", "corner_radius"),
    ("semantic.radius-sm", "corner_radius_sm"),
    // 컨트롤 높이 / 탭
    ("semantic.control-height-tree", "item_height_tree"),
    ("semantic.control-height", "item_height_interactive"),
    ("semantic.control-height-tab", "item_height_tab"),
    ("semantic.tab-width", "tab_width"),
    // 타이포
    ("semantic.font-size-micro", "font_size_micro"),
    ("semantic.font-size-caption", "font_size_caption"),
    ("semantic.font-size-body", "font_size_body"),
    ("semantic.font-size-heading", "font_size_heading"),
    ("semantic.font-size-max", "font_size_max"),
    (
        "semantic.font-size-brand-display",
        "font_size_brand_display",
    ),
    ("semantic.font-size-prose-h1", "font_size_prose_h1"),
    // `semantic.font-size-prose-h2` 는 은퇴·제거됨 — egui_commonmark 이 헤딩을 보간해
    // per-H2 픽셀을 받지 못한다(vendor json 에서도 제거됨).
    ("semantic.font-size-term-sm", "font_size_term_sm"),
    ("semantic.font-size-term", "font_size_term"),
    ("semantic.font-size-term-lg", "font_size_term_lg"),
    // `semantic.line-height-prose` 는 은퇴·제거됨 — markdown body leading 을
    // egui_commonmark 이 소유해 override 미노출(vendor json 에서도 제거됨).
    // 아이콘 글리프
    ("semantic.icon-size-xs", "icon_glyph_size_xs"),
    ("semantic.icon-size-sm", "icon_glyph_size_sm"),
    ("semantic.icon-size-md", "icon_glyph_size_md"),
    // 가독 폭 / form-control 폭
    ("semantic.measure-sm", "measure_sm"),
    ("semantic.measure-md", "measure_md"),
    ("semantic.measure-lg", "measure_lg"),
    ("semantic.measure-xl", "measure_xl"),
    ("semantic.field-width-xs", "field_width_xs"),
    ("semantic.field-width-color", "field_width_color"),
    ("semantic.field-width-md", "field_width_md"),
    ("semantic.field-width-lg", "field_width_lg"),
    // 세부 치수 (semantic)
    ("semantic.status-bar-height", "status_bar_height"),
    ("semantic.titlebar-height", "titlebar_height"),
    ("semantic.overlay-top-offset", "overlay_top_offset"),
    // component 전용 필드 — 사이드바 (host UI zoom 영향 받음)
    ("component.sidebar-logo-size", "sidebar_logo_size"),
    (
        "component.sidebar-logo-collapsed-size",
        "sidebar_logo_collapsed_size",
    ),
    (
        "component.sidebar-wordmark-font-size",
        "sidebar_wordmark_font_size",
    ),
    (
        "component.sidebar-section-heading-font-size",
        "sidebar_section_heading_font_size",
    ),
    (
        "component.sidebar-button-label-font-size",
        "sidebar_button_label_font_size",
    ),
    (
        "component.sidebar-collapsed-slot-width",
        "sidebar_collapsed_slot_width",
    ),
    (
        "component.sidebar-collapsed-icon-height",
        "sidebar_collapsed_icon_height",
    ),
    (
        "component.sidebar-collapsed-workspace-height",
        "sidebar_collapsed_workspace_height",
    ),
    // component 전용 필드 — titlebar / toast / status-dot / spinner / tab (zoom 제외 또는 고정)
    ("component.titlebar-traffic-size", "traffic_size"),
    ("component.titlebar-caption-width", "caption_width"),
    (
        "component.titlebar-window-button-size",
        "window_button_size",
    ),
    ("component.toast-max-width", "toast_max_width"),
    ("component.toast-accent-width", "toast_accent_width"),
    ("component.status-dot-size", "status_dot_size"),
    ("component.spinner-size", "spinner_size"),
    ("component.tab-indicator-width", "tab_indicator_width"),
    // zoom-제외 host-chrome 전용 필드 — 위 item_height_tab/font_size_body/
    // font_size_caption 이 같은 토큰 경로를 먼저 흡수하므로 lookup 에서는 도달하지
    // 않는다. sizing_parity 가드 완전성을 위해서만 유지.
    ("semantic.control-height-tab", "tab_bar_height"),
    ("semantic.font-size-body", "tab_bar_label_font_size"),
    ("semantic.font-size-caption", "tab_bar_arrow_font_size"),
];

/// semantic 색 토큰의 전체 경로 ↔ `theme.rs` 수기 접근자 표현식. 필드는 괄호 없이
/// (`separator`), 메서드는 `()` 포함(`accent_primary()`) — `self.<expr>` 로 그대로
/// 이어붙인다.
///
/// component 색 토큰의 alias 체인이 semantic 홉에서 이 표에 없는 경로를 만나면
/// 생성기는 skip + 로그한다. 값을 임의로 새 접근자에 매핑하지 않는다 — 대응
/// 접근자가 실제로 존재하는데 이 표에만 빠져 있으면 여기 한 줄을 추가한다.
pub const SEMANTIC_COLOR_TO_THEME_ACCESSOR: &[(&str, &str)] = &[
    ("semantic.accent-agent", "accent_agent()"),
    ("semantic.accent-attached", "border_attached()"),
    ("semantic.accent-attention", "accent_attention()"),
    ("semantic.accent-danger", "accent_danger()"),
    ("semantic.accent-decorative", "accent_decorative()"),
    ("semantic.accent-info", "accent_info()"),
    ("semantic.accent-macos-close", "accent_macos_close()"),
    ("semantic.accent-macos-min", "accent_macos_min()"),
    ("semantic.accent-macos-zoom", "accent_macos_zoom()"),
    ("semantic.accent-occupied-hard", "accent_occupied_hard()"),
    ("semantic.accent-occupied-soft", "accent_occupied_soft()"),
    ("semantic.accent-primary", "accent_primary()"),
    ("semantic.accent-remote", "accent_remote()"),
    ("semantic.accent-success", "accent_success()"),
    ("semantic.accent-warning", "accent_warning()"),
    ("semantic.accent-window-close", "accent_window_close()"),
    ("semantic.bg-app", "bg_app()"),
    ("semantic.bg-panel", "bg_panel()"),
    ("semantic.bg-sidebar", "bg_sidebar()"),
    ("semantic.border-default", "border_default()"),
    ("semantic.border-focus", "border_focus()"),
    ("semantic.border-frame", "border_frame()"),
    ("semantic.border-strong", "border_strong()"),
    ("semantic.glyph-dim", "glyph_dim()"),
    ("semantic.overlay-active", "overlay_active()"),
    ("semantic.overlay-hover", "overlay_hover()"),
    ("semantic.separator", "separator"),
    ("semantic.status-idle", "status_idle()"),
    ("semantic.surface-active", "surface_active()"),
    ("semantic.surface-hover", "surface_hover()"),
    ("semantic.surface-raised", "surface_raised()"),
    ("semantic.text-disabled", "text_disabled()"),
    ("semantic.text-muted", "text_muted()"),
    ("semantic.text-on-accent", "text_on_accent()"),
    ("semantic.text-on-window-close", "text_on_window_close()"),
    ("semantic.text-placeholder", "text_placeholder()"),
    ("semantic.text-primary", "text_primary()"),
    ("semantic.text-secondary", "text_secondary()"),
];

/// 생성 대상 semantic 색 접근자 — (DTCG semantic 토큰 경로, `impl Theme` 메서드명,
/// 반환 `Theme` primitive 필드). 각 항목은 theme.rs 의 기존 수기 접근자와 **diff 0**
/// (동일 필드를 반환). 필드 바인딩을 표로 고정하는 이유는 DTCG primitive → `Theme`
/// 필드 대응이 1:1 이 아니기 때문 — 예: `text-placeholder` 는 `{primitive.color-neutral-600}`
/// (값상 `overlay0` 과 동일) 이지만 별도 `placeholder` 필드로 종착한다. 값 일치는
/// `tests/color_drift.rs`, 필드 일치는 theme.rs `semantic_accessors_map_to_primitives`
/// 가 이중으로 가드한다.
///
/// 메서드명이 토큰명 snake_case 와 다른 경우가 있다 — `accent-attached` → `border_attached`
/// (attached workspace outline 은 accent 가 아니라 border role). 그래서 fn 이름을 표에 명시한다.
pub const SEMANTIC_COLOR_ACCESSOR_GEN: &[(&str, &str, &str)] = &[
    // 배경 (bg-*)
    ("semantic.bg-app", "bg_app", "crust"),
    ("semantic.bg-sidebar", "bg_sidebar", "mantle"),
    ("semantic.bg-panel", "bg_panel", "base"),
    // 표면 (surface-*)
    ("semantic.surface-raised", "surface_raised", "surface0"),
    ("semantic.surface-hover", "surface_hover", "surface1"),
    ("semantic.surface-active", "surface_active", "surface2"),
    // 텍스트 (text-*)
    ("semantic.text-primary", "text_primary", "text"),
    ("semantic.text-secondary", "text_secondary", "subtext1"),
    ("semantic.text-muted", "text_muted", "subtext0"),
    ("semantic.text-disabled", "text_disabled", "overlay1"),
    (
        "semantic.text-placeholder",
        "text_placeholder",
        "placeholder",
    ),
    // accent (의미색)
    ("semantic.accent-primary", "accent_primary", "blue"),
    ("semantic.accent-info", "accent_info", "sky"),
    // accent-remote: mirror/원격 origin 전용 role. accent-info 와 같은 sky 지만 의미 분리
    // (accent-info 는 git-viewer/chip/banner/explorer/preset 다수 실사용처 — 용도 격리).
    ("semantic.accent-remote", "accent_remote", "sky"),
    ("semantic.accent-success", "accent_success", "green"),
    ("semantic.accent-warning", "accent_warning", "yellow"),
    // accent-attention: plugin/occupancy "needs-attention" notice role. accent-warning
    // (yellow) 과 별도 — peach 로 분리해 경고(yellow)와 주의환기(peach)를 구분한다.
    ("semantic.accent-attention", "accent_attention", "peach"),
    // accent-occupied-soft/hard: surface 점유(occupancy) 테두리 role (ADR-0040).
    // soft=green(협조 신호, write 제한 없음), hard=peach(readonly + force-detach).
    // accent-success(green)·accent-
    // attention(peach) 와 primitive 는 공유하나 의미가 겹치지 않도록 독립 role 로
    // 분리 — 점유 의미가 축 독립 진화 가능.
    (
        "semantic.accent-occupied-soft",
        "accent_occupied_soft",
        "green",
    ),
    (
        "semantic.accent-occupied-hard",
        "accent_occupied_hard",
        "peach",
    ),
    ("semantic.accent-danger", "accent_danger", "red"),
    ("semantic.accent-agent", "accent_agent", "mauve"),
    ("semantic.accent-attached", "border_attached", "lavender"),
    // accent-decorative: 장식 accent role (Plugins 창 헤더 glyph). accent-attention
    // (peach) 과 primitive 는 같지만 "주의 환기" 가 아니라 "장식" 이라 role 을 가른다.
    ("semantic.accent-decorative", "accent_decorative", "peach"),
    // 상태 표시 (status-*)
    // status-idle: idle/inactive 인디케이터 톤. 값상 text-placeholder 와 같은
    // neutral-600 이지만 필드는 `overlay0` 로 종착한다 — `placeholder` 는 텍스트
    // 입력 전용 필드라 사용자가 독립적으로 덮어쓸 수 있고, 인디케이터 도트가 그
    // 오버라이드를 따라가는 것은 의도가 아니다.
    ("semantic.status-idle", "status_idle", "overlay0"),
    // glyph-dim: 물러나야 하는 chrome glyph 톤. 값상 text-placeholder 와 같은
    // neutral-600 이지만 status-idle 과 같은 이유로 `overlay0` 로 종착한다 —
    // `placeholder` 는 텍스트 입력 전용 필드라 사용자 오버라이드를 chrome glyph 가
    // 따라가는 것은 의도가 아니다. disabled 용이 아니다(그쪽은 text-disabled).
    ("semantic.glyph-dim", "glyph_dim", "overlay0"),
    // 보더 (border-*)
    ("semantic.border-default", "border_default", "surface0"),
    ("semantic.border-strong", "border_strong", "surface1"),
    ("semantic.border-focus", "border_focus", "blue"),
    // border-frame: surface2 값의 border role (pane divider · GPU 비활성 surface
    // 보더 · popup 프레임 보더). surface-active 와 primitive 는 같지만 "선택된 표면"
    // 이 아니라 "틀의 선" 이라 role 을 가른다.
    ("semantic.border-frame", "border_frame", "surface2"),
];

/// semantic 색 토큰 중 **생성하지 않고 theme.rs 에 수기로 남기는** 접근자 + 사유.
/// (단순 primitive 필드 alias 가 아니라 분기·도출·합성·리터럴이라 codegen 불가.)
/// 나머지 semantic 색(ansi-*·surface-terminal/markdown-*·selection/vi/search-*·
/// brand-melon-rind/seed)은 semantic **접근자 자체가 없다** — 터미널 표면 색
/// subsystem 또는 미사용 토큰이라 여기 열거하지 않는다.
const SEMANTIC_COLOR_HAND_WRITTEN: &[(&str, &str)] = &[
    (
        "semantic.text-on-accent",
        "is_light role-remap (Mocha=crust / Latte=white) — 단순 alias 아님",
    ),
    (
        "semantic.overlay-hover",
        "derive_overlays 도출값 (hover_overlay, primitive 아님)",
    ),
    (
        "semantic.overlay-active",
        "derive_overlays 도출값 (active_overlay, primitive 아님)",
    ),
    (
        "semantic.scrim-bg",
        "합성색 from_rgba(0,0,0,SCRIM_ALPHA) — primitive 필드 아님",
    ),
    (
        "semantic.accent-window-close",
        "OS 리터럴 const (Windows close hover, 테마 불변)",
    ),
    ("semantic.text-on-window-close", "리터럴 const (white 고정)"),
    ("semantic.accent-macos-close", "OS 리터럴 const"),
    ("semantic.accent-macos-min", "OS 리터럴 const"),
    ("semantic.accent-macos-zoom", "OS 리터럴 const"),
    (
        "semantic.brand-melon-flesh",
        "브랜드 리터럴 const (테마 불변)",
    ),
];

/// component 색 접근자 이름이 `theme.rs` 의 기존 수기 접근자와 충돌하는 목록.
/// `banner-*`/`titlebar-*` 색은 이미 semantic 접근자 조합으로 손으로 작성돼 있다
/// (예: `banner_bg` → `surface_raised()`) — 동일 이름으로 재생성하면 `impl Theme`
/// 중복 정의로 컴파일이 깨진다. 새 충돌이 생기면 `cargo build` 가 "duplicate
/// definitions" 로 즉시 드러나며, 그때 이 표에 추가한다.
const EXISTING_THEME_ACCESSOR_NAMES: &[&str] = &[
    "banner_bg",
    "banner_border",
    "banner_fg",
    "banner_icon_fg",
    "banner_countdown_fg",
    "titlebar_bg",
    "titlebar_bg_inactive",
    "titlebar_border",
    "titlebar_fg",
    "titlebar_fg_inactive",
    "preset_leaf_label_fg",
    "preset_leaf_value_fg",
    "modhint_bg",
    "modhint_border",
    "modhint_role_bg",
    "modhint_role_fg",
    "modhint_empty_fg",
];

/// [`EXISTING_THEME_ACCESSOR_NAMES`]의 치수(dimension) 버전 — component 치수
/// 접근자 생성 시에도 동일한 이름 충돌이 발생할 수 있다(예: modifier-hint 크기
/// 토큰은 theme.rs 에 수기 접근자가 먼저 생겼고, 이후 vendor json 에 대응 component
/// 토큰이 추가됨).
const EXISTING_THEME_DIM_ACCESSOR_NAMES: &[&str] = &[
    "modhint_width",
    "modhint_height",
    "modhint_min_width",
    "modhint_min_height",
    "modhint_section_gap",
];

/// component 토큰 kebab 이름 → snake_case 접근자 함수명 (전체 이름 그대로 —
/// `component_split` 의 모듈 분해와 달리 flat `impl Theme` 메서드라 그룹 접두어를
/// 남겨 둔다. 예: `switch-overlay-bg` → `switch_overlay_bg`).
pub(super) fn accessor_fn_name(name: &str) -> String {
    name.replace('-', "_")
}

/// 치수 component 접근자의 본문 형태.
enum DimAccessor<'a> {
    /// alias 체인이 표에 있는 `Theme` 필드에 닿음 — 필드를 그대로 반환.
    Field(&'a str),
    /// alias 체인이 다른 component 접근자에 닿음(component→component) — 그 접근자를 호출.
    Chain(String),
    /// alias 체인이 primitive 로 직접 닿거나 표에 없는 semantic 을 거침 — `ui_zoom` 을
    /// 곱해 직접 계산 (raw const 를 그대로 소비하는 zoom 우회를 막는다).
    RawZoom(f32),
}

/// 치수 component 토큰 하나의 접근자 형태를 결정. 실패하면 스킵 사유 문자열.
fn resolve_dim_accessor<'a>(set: &TokenSet, token: &'a Token) -> Result<DimAccessor<'a>, String> {
    let own_path = token.path();
    if let Some((_, field)) = SEMANTIC_DIM_TO_THEME_FIELD
        .iter()
        .find(|(p, _)| *p == own_path)
    {
        return Ok(DimAccessor::Field(field));
    }
    let target_path = match alias_target(&token.value) {
        Some(t) => t,
        None => return Err(format!("{own_path}: 리터럴(alias 아님) — 생성 스킵")),
    };
    if let Some((_, field)) = SEMANTIC_DIM_TO_THEME_FIELD
        .iter()
        .find(|(p, _)| *p == target_path)
    {
        return Ok(DimAccessor::Field(field));
    }
    match set.get(target_path) {
        Some(target) if target.tier == Tier::Component => {
            Ok(DimAccessor::Chain(accessor_fn_name(&target.name)))
        }
        Some(_) => {
            let terminal = set
                .resolve(&own_path, ThemeMode::Mocha)
                .map_err(|e| format!("{own_path}: {e} — 생성 스킵"))?;
            let stripped = terminal.strip_suffix("px").unwrap_or(&terminal);
            stripped
                .trim()
                .parse::<f32>()
                .map(DimAccessor::RawZoom)
                .map_err(|_| format!("{own_path}: 터미널 값 파싱 실패 ({terminal}) — 생성 스킵"))
        }
        None => Err(format!(
            "{own_path}: alias 대상 없음 ({target_path}) — 생성 스킵"
        )),
    }
}

/// 색 component 접근자의 본문 형태.
enum ColorAccessor {
    /// alias 체인이 semantic 색에 닿고, 표에 대응 `theme.rs` 접근자가 있음.
    SemanticExpr(&'static str),
    /// alias 체인이 다른 component 색 접근자에 닿음(component→component).
    Chain(String),
}

/// 색 component 토큰 하나의 접근자 형태를 결정. 실패하면 스킵 사유 문자열.
fn resolve_color_accessor(set: &TokenSet, token: &Token) -> Result<ColorAccessor, String> {
    let own_path = token.path();
    let target_path = match alias_target(&token.value) {
        Some(t) => t,
        None => return Err(format!("{own_path}: 리터럴/합성 색상값 — 생성 스킵")),
    };
    match set.get(target_path) {
        Some(target) if target.tier == Tier::Semantic => SEMANTIC_COLOR_TO_THEME_ACCESSOR
            .iter()
            .find(|(p, _)| *p == target_path)
            .map(|(_, expr)| ColorAccessor::SemanticExpr(expr))
            .ok_or_else(|| {
                format!("{own_path}: `{target_path}` 대응 theme.rs 접근자 없음 — 생성 스킵")
            }),
        Some(target) if target.tier == Tier::Component => {
            let fn_name = accessor_fn_name(&target.name);
            // chain 대상이 EXISTING_THEME_ACCESSOR_NAMES 충돌로 스킵되거나 자기 자신의
            // alias 해석에 실패하면, 대상 접근자가 실제로 생성되지 않아 이 체인 호출이
            // dangling self-call 이 된다 — tier 만 보고 낙관적으로 Chain 을 반환하지
            // 않도록 재귀 검증한다.
            if EXISTING_THEME_ACCESSOR_NAMES.contains(&fn_name.as_str()) {
                Ok(ColorAccessor::Chain(fn_name))
            } else {
                resolve_color_accessor(set, target)
                    .map(|_| ColorAccessor::Chain(fn_name))
                    .map_err(|e| format!("{own_path}: chain 대상이 스킵됨 → {e}"))
            }
        }
        Some(_) => Err(format!(
            "{own_path}: primitive 직접 alias — 색 접근자 규칙 밖, 생성 스킵"
        )),
        None => Err(format!(
            "{own_path}: alias 대상 없음 ({target_path}) — 생성 스킵"
        )),
    }
}

/// 치수 접근자 하나의 `impl Theme` 메서드 텍스트.
fn emit_dim_accessor(set: &TokenSet, token: &Token, acc: &DimAccessor) -> String {
    let terminal = set
        .resolve(&token.path(), ThemeMode::Mocha)
        .expect("resolve_dim_accessor 통과 토큰은 resolve 가능");
    let sentinel = if terminal == "9999px" {
        " (sentinel — 완전 원형용 상한값)"
    } else {
        ""
    };
    let fn_name = accessor_fn_name(&token.name);
    let body = match acc {
        DimAccessor::Field(field) => format!("self.{field}"),
        DimAccessor::Chain(target_fn) => format!("self.{target_fn}()"),
        DimAccessor::RawZoom(v) => format!("LogicalPx(({v:?} * self.ui_zoom).round())"),
    };
    format!(
        "\n    /// `{}` → `{}` = {terminal}{sentinel}\n    #[inline]\n    pub fn {fn_name}(&self) -> LogicalPx {{\n        {body}\n    }}\n",
        token.path(),
        token.value,
    )
}

/// 색 접근자 하나의 `impl Theme` 메서드 텍스트.
fn emit_color_accessor(token: &Token, acc: &ColorAccessor) -> String {
    let fn_name = accessor_fn_name(&token.name);
    let body = match acc {
        ColorAccessor::SemanticExpr(expr) => format!("self.{expr}"),
        ColorAccessor::Chain(target_fn) => format!("self.{target_fn}()"),
    };
    format!(
        "\n    /// `{}` → `{}`\n    #[inline]\n    pub fn {fn_name}(&self) -> HexColor {{\n        {body}\n    }}\n",
        token.path(),
        token.value,
    )
}

/// component 치수+색 접근자 파일(`generated_component.rs`) 본문을 만든다.
/// `crates/tasty-type-appearance/src/` 에 산출 — `&Theme` 경유 강제 원칙 때문에
/// (`tasty-design-tokens` → `tasty-type-appearance` 런타임 의존은 금지이므로
/// 생성기가 산출물을 상대 크레이트 안에 직접 쓴다).
pub(super) fn generate_component_accessors(set: &TokenSet) -> (String, Vec<String>) {
    let mut skips = Vec::new();
    let mut body = String::new();

    for token in set.iter().filter(|t| t.tier == Tier::Component) {
        match token.ty.as_str() {
            "dimension" => {
                let fn_name = accessor_fn_name(&token.name);
                if EXISTING_THEME_DIM_ACCESSOR_NAMES.contains(&fn_name.as_str()) {
                    skips.push(format!(
                        "{}: theme.rs 기존 수기 접근자 `{fn_name}` 과 이름 충돌 — 생성 스킵",
                        token.path()
                    ));
                    continue;
                }
                match resolve_dim_accessor(set, token) {
                    Ok(acc) => body.push_str(&emit_dim_accessor(set, token, &acc)),
                    Err(reason) => skips.push(reason),
                }
            }
            "color" => {
                let fn_name = accessor_fn_name(&token.name);
                if EXISTING_THEME_ACCESSOR_NAMES.contains(&fn_name.as_str()) {
                    skips.push(format!(
                        "{}: theme.rs 기존 수기 접근자 `{fn_name}` 과 이름 충돌 — 생성 스킵",
                        token.path()
                    ));
                    continue;
                }
                match resolve_color_accessor(set, token) {
                    Ok(acc) => body.push_str(&emit_color_accessor(token, &acc)),
                    Err(reason) => skips.push(reason),
                }
            }
            "duration" => {
                let fn_name = accessor_fn_name(&token.name);
                // 이름이 겹치면 `impl Theme` 이 중복 메서드로 컴파일이 깨진다 — 반환
                // 타입이 달라도 마찬가지라 두 표를 **함께** 본다.
                if EXISTING_THEME_DIM_ACCESSOR_NAMES.contains(&fn_name.as_str())
                    || EXISTING_THEME_ACCESSOR_NAMES.contains(&fn_name.as_str())
                {
                    skips.push(format!(
                        "{}: theme.rs 기존 수기 접근자 `{fn_name}` 과 이름 충돌 — 생성 스킵",
                        token.path()
                    ));
                    continue;
                }
                match resolve_duration_accessor(set, token) {
                    Ok(acc) => body.push_str(&emit_duration_accessor(set, token, &acc)),
                    Err(reason) => skips.push(reason),
                }
            }
            // number/fontWeight component 토큰 — component 접근자 생성 범위 밖. 테마 불변이고 무단위라
            // `generated::component` 의 raw const 로 이미 충분.
            _ => {}
        }
    }

    let header = "//! Generated from `dtcg/tasty.tokens.json` — DO NOT EDIT.\n\
                  //! 재생성: `cargo run -p tasty-design-tokens --bin generate`.\n\
                  //!\n\
                  //! Tier 3 (component) 치수·색·시간 접근자. `generated::component` 의\n\
                  //! raw const 와 달리 **`&Theme` 경유** — 치수는 zoom-resolve 된 필드를\n\
                  //! 반환하거나(semantic 종착) `ui_zoom` 을 직접 곱하고(primitive 직접\n\
                  //! 종착), 색은 semantic 접근자 체인 또는 component→component 접근자\n\
                  //! 상호 호출로 이어붙인다. 시간은 `Millis` 로 나가며 **zoom 을 곱하지\n\
                  //! 않는다** — 배율은 길이 축이다.\n\n\
                  use crate::color::HexColor;\n\
                  use crate::motion::Millis;\n\
                  use tasty_type_geometry::length::LogicalPx;\n\n\
                  impl crate::theme::Theme {";
    let mut file = header.to_string();
    file.push_str(&body);
    file.push_str("}\n");
    (file, skips)
}

// ============================================================================
//  Semantic 색 접근자 (단순 primitive 필드 alias) — 05-A
// ============================================================================
//
// theme.rs 가 수기로 들고 있던 semantic 색 접근자(`bg_app`/`accent_primary`/
// `border_default` 등)를 DTCG semantic 색 토큰에서 생성으로 전환한다. 각 접근자는
// `&Theme` 의 primitive 필드를 그대로 반환 — component 색 접근자가 이 semantic
// 접근자를 `self.accent_primary()` 처럼 호출하므로 inherent method 이름을 유지한다.
// 분기(is_light)·도출(overlay)·합성(scrim)·리터럴(OS/brand) 접근자는 생성 불가라
// theme.rs 에 수기로 남는다 (`SEMANTIC_COLOR_HAND_WRITTEN` 참조).

/// semantic 색 접근자 하나의 `impl Theme` 메서드 텍스트.
fn emit_semantic_color_accessor(token: &Token, fn_name: &str, field: &str) -> String {
    format!(
        "\n    /// `{}` → `{}`\n    #[inline]\n    pub fn {fn_name}(&self) -> HexColor {{\n        self.{field}\n    }}\n",
        token.path(),
        token.value,
    )
}

/// semantic 색 접근자 파일(`semantic_color_generated.rs`) 본문을 만든다.
/// component 접근자와 같은 이유로 `crates/tasty-type-appearance/src/` 에 산출
/// (`&Theme` 경유 강제 + 런타임 의존 방향 보존).
pub(super) fn generate_semantic_color_accessors(set: &TokenSet) -> (String, Vec<String>) {
    let mut skips = Vec::new();
    let mut body = String::new();

    for (path, fn_name, field) in SEMANTIC_COLOR_ACCESSOR_GEN {
        match set.get(path) {
            // 표에 든 토큰이 사라지면(디자인 rename 등) 드리프트 신호로 skip 로그.
            None => skips.push(format!(
                "{path}: SEMANTIC_COLOR_ACCESSOR_GEN 표에 있으나 vendor json 에 없음 — 생성 스킵"
            )),
            Some(token) if token.ty != "color" => skips.push(format!(
                "{path}: $type {} (color 아님) — 생성 스킵",
                token.ty
            )),
            Some(token) => body.push_str(&emit_semantic_color_accessor(token, fn_name, field)),
        }
    }

    for (path, reason) in SEMANTIC_COLOR_HAND_WRITTEN {
        skips.push(format!("{path}: {reason} — theme.rs 수기 유지"));
    }

    let header = "//! Generated from `dtcg/tasty.tokens.json` — DO NOT EDIT.\n\
                  //! 재생성: `cargo run -p tasty-design-tokens --bin generate`.\n\
                  //!\n\
                  //! Tier 2 (semantic) 색 접근자. 각 메서드는 DTCG semantic 색 토큰의\n\
                  //! primitive 종착을 `Theme` 필드로 그대로 반환하는 단순 alias 다.\n\
                  //! is_light 분기(text-on-accent)·도출 overlay·합성색(scrim)·OS/brand\n\
                  //! 리터럴 등 비단순 접근자는 theme.rs 에 수기로 남는다.\n\n\
                  use crate::color::HexColor;\n\n\
                  impl crate::theme::Theme {";
    let mut file = header.to_string();
    file.push_str(&body);
    file.push_str("}\n");
    (file, skips)
}
