//! Theme의 배율 정책을 적용하는 component 치수·시간 접근자와 색 접근자를 생성한다.
//! 배율을 적용하지 않은 상수는 `super`에서 생성한다.

use super::{
    ThemeMode, Tier, Token, TokenSet, alias_target,
    duration_accessor::{emit_duration_accessor, resolve_duration_accessor},
};

/// 토큰 경로와 Theme/SIZING 필드의 대응표. 치수 검사도 이 표를 사용한다.
/// 같은 토큰에 필드가 여럿이면 첫 항목을 선택하므로 배율이 적용되는 위젯 필드를
/// 배율에서 제외되는 창 UI 필드보다 앞에 둔다.
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
    ("semantic.font-size-term-sm", "font_size_term_sm"),
    ("semantic.font-size-term", "font_size_term"),
    ("semantic.font-size-term-lg", "font_size_term_lg"),
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
    // 위의 배율 적용 필드가 먼저 선택된다. 아래 필드는 치수 검사에 필요하다.
    ("semantic.control-height-tab", "tab_bar_height"),
    ("semantic.font-size-body", "tab_bar_label_font_size"),
    ("semantic.font-size-caption", "tab_bar_arrow_font_size"),
];

/// semantic 색 토큰과 Theme 필드·메서드의 대응표. `self.<expr>`로 생성한다.
/// 대응하는 접근자가 없는 토큰은 로그를 남기고 제외한다.
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

/// 생성할 semantic 색 접근자: 토큰 경로, 메서드명, 반환할 Theme 필드.
/// 토큰과 필드가 항상 일대일로 대응하지 않으므로 명시적으로 연결한다.
/// 예를 들어 text-placeholder는 overlay0와 기본값이 같지만 별도 placeholder 필드를 쓴다.
/// color_drift 시험은 값, semantic_accessors_map_to_primitives 시험은 필드 연결을 확인한다.
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
    // 원격 연결과 일반 안내는 기본 색이 같아도 용도를 구분한다.
    ("semantic.accent-remote", "accent_remote", "sky"),
    ("semantic.accent-success", "accent_success", "green"),
    ("semantic.accent-warning", "accent_warning", "yellow"),
    // 주의 환기(peach)와 경고(yellow)를 구분한다.
    ("semantic.accent-attention", "accent_attention", "peach"),
    // 점유 표시는 soft=green, hard=peach. 성공·주의 환기와 색이 같아도 별도 역할이다.
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
    // 장식과 주의 환기는 색이 같아도 역할을 구분한다.
    ("semantic.accent-decorative", "accent_decorative", "peach"),
    // 상태 표시 (status-*)
    // 입력 placeholder의 사용자 설정이 상태 표시 색에 영향을 주지 않게 한다.
    ("semantic.status-idle", "status_idle", "overlay0"),
    // 약하게 표시할 창 아이콘 색. 입력 placeholder 설정이나 비활성 색과 구분한다.
    ("semantic.glyph-dim", "glyph_dim", "overlay0"),
    // 보더 (border-*)
    ("semantic.border-default", "border_default", "surface0"),
    ("semantic.border-strong", "border_strong", "surface1"),
    ("semantic.border-focus", "border_focus", "blue"),
    // 프레임 선과 선택된 배경은 색이 같아도 역할을 구분한다.
    ("semantic.border-frame", "border_frame", "surface2"),
];

/// 단순 필드 반환이 아닌 색 접근자는 theme.rs에 직접 구현한다.
/// 터미널 전용 색이나 사용하지 않는 토큰처럼 접근자 자체가 없는 색은 이 목록에서 제외한다.
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

/// theme.rs에 직접 구현한 색 접근자. 같은 이름으로 생성하면 중복 정의가 된다.
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

/// theme.rs에 직접 구현한 치수 접근자. 같은 이름으로 생성하지 않는다.
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
                .map_err(|_| format!("{own_path}: 최종 값 파싱 실패 ({terminal}) — 생성 스킵"))
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
            // 대상 접근자도 생성 가능한지 확인해야 존재하지 않는 메서드를 호출하지 않는다.
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

/// component 접근자는 Theme가 있는 type-appearance에 쓴다.
/// design-tokens가 type-appearance에 런타임 의존성을 갖지 않도록 한다.
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
                // 반환 타입이 달라도 같은 이름의 메서드를 중복 정의할 수 없다.
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
            // 테마와 배율에 무관한 number/fontWeight는 상수로만 생성한다.
            _ => {}
        }
    }

    let header = "//! Generated from `dtcg/tasty.tokens.json` — DO NOT EDIT.\n\
                  //! 재생성: `cargo run -p tasty-design-tokens --bin generate`.\n\
                  //!\n\
                  //! Component 치수·색·시간을 Theme를 통해 읽는다.\n\
                  //! 치수는 배율을 적용한 필드를 쓰거나 ui_zoom을 곱한다.\n\
                  //! 색은 연결된 접근자로 읽고, 시간은 배율 없이 Millis로 반환한다.\n\n\
                  use crate::color::HexColor;\n\
                  use crate::motion::Millis;\n\
                  use tasty_type_geometry::length::LogicalPx;\n\n\
                  impl crate::theme::Theme {";
    let mut file = header.to_string();
    file.push_str(&body);
    file.push_str("}\n");
    (file, skips)
}

/// semantic 색 접근자 하나의 `impl Theme` 메서드 텍스트.
fn emit_semantic_color_accessor(token: &Token, fn_name: &str, field: &str) -> String {
    format!(
        "\n    /// `{}` → `{}`\n    #[inline]\n    pub fn {fn_name}(&self) -> HexColor {{\n        self.{field}\n    }}\n",
        token.path(),
        token.value,
    )
}

/// semantic 색 접근자를 Theme가 있는 type-appearance에 생성한다.
pub(super) fn generate_semantic_color_accessors(set: &TokenSet) -> (String, Vec<String>) {
    let mut skips = Vec::new();
    let mut body = String::new();

    for (path, fn_name, field) in SEMANTIC_COLOR_ACCESSOR_GEN {
        match set.get(path) {
            // 대응표에만 남은 토큰은 로그로 알린다.
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
                  //! 대응 `Theme` 필드를 반환한다.\n\
                  //! is_light 분기(text-on-accent)·도출 overlay·합성색(scrim)·OS/brand\n\
                  //! 리터럴 등 비단순 접근자는 theme.rs 에 수기로 남는다.\n\n\
                  use crate::color::HexColor;\n\n\
                  impl crate::theme::Theme {";
    let mut file = header.to_string();
    file.push_str(&body);
    file.push_str("}\n");
    (file, skips)
}
