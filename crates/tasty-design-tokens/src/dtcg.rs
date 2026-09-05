//! W3C DTCG 토큰 파서 + alias 해석기 + Rust 코드 생성기.
//!
//! vendor 실물(`dtcg/tasty.tokens.json`)은 strict DTCG 가 아니다 — `$value` 는
//! 보통 문자열이고(`"1.4"`, `"500"`), dimension 에 `"12px"` / `"0"`(무단위) /
//! `"0.04em"`(em) 이 혼재하며, shadow/cubicBezier 는 CSS 문자열이다. 다만
//! `$type: "number"` 토큰 일부는 raw JSON number(`0.4`)로 export 되기도 하므로
//! 파서는 문자열/숫자/불리언 스칼라를 모두 받아 문자열로 정규화한다. 파서는 이
//! 실물 형식을 기준으로 한다.
//!
//! alias 는 `{tier.name}` 문법. 디자인 계약(TOKENS.md)과 달리 실물에는
//! component → primitive 직접 참조 등 tier-skip alias 가 실존하므로, 해석기는
//! 임의 tier 간 참조 + 다단 체인을 허용한다. tier 규율의 강제는 생성물
//! visibility(`generated::primitive` = `pub(crate)`)로만 수행한다.

mod duration_accessor;
use duration_accessor::{emit_duration_accessor, resolve_duration_accessor};

use std::collections::BTreeMap;
use std::fmt;

// ============================================================================
//  모델
// ============================================================================

/// 3-tier 중 어느 계층의 토큰인지.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Primitive,
    Semantic,
    Component,
}

impl Tier {
    pub const ALL: [Tier; 3] = [Tier::Primitive, Tier::Semantic, Tier::Component];

    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Primitive => "primitive",
            Tier::Semantic => "semantic",
            Tier::Component => "component",
        }
    }
}

/// 토큰 하나. `latte` 는 `$extensions["com.tasty.mode"].latte` 값(리터럴 또는 alias).
#[derive(Debug, Clone)]
pub struct Token {
    pub tier: Tier,
    /// tier 내 이름 (kebab-case, 예: `size-12`).
    pub name: String,
    /// `$type` (예: `color` / `dimension` / `duration` / `number` / `fontWeight`).
    pub ty: String,
    /// `$value` — 리터럴 또는 `{tier.name}` alias.
    pub value: String,
    pub latte: Option<String>,
}

impl Token {
    /// alias 참조에서 쓰는 전체 경로 (`primitive.size-12`).
    pub fn path(&self) -> String {
        format!("{}.{}", self.tier.as_str(), self.name)
    }
}

/// `"{tier.name}"` 형태면 내부 경로를 돌려준다.
pub fn alias_target(raw: &str) -> Option<&str> {
    raw.strip_prefix('{').and_then(|s| s.strip_suffix('}'))
}

/// 테마 모드. latte 해석 시 각 hop 에서 latte 오버라이드를 우선한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Mocha,
    Latte,
}

/// 파싱된 토큰 전체. key = `tier.name` 전체 경로 (BTreeMap → 결정적 순서).
#[derive(Debug)]
pub struct TokenSet {
    tokens: BTreeMap<String, Token>,
}

impl TokenSet {
    pub fn get(&self, path: &str) -> Option<&Token> {
        self.tokens.get(path)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Token> {
        self.tokens.values()
    }

    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    pub fn tier_count(&self, tier: Tier) -> usize {
        self.tokens.values().filter(|t| t.tier == tier).count()
    }

    /// alias 체인을 끝까 따라가 터미널 리터럴을 돌려준다.
    /// latte 모드는 각 hop 에서 latte 오버라이드가 있으면 그것을 따른다.
    pub fn resolve(&self, path: &str, mode: ThemeMode) -> Result<String, ResolveError> {
        let mut current = path.to_string();
        // 실물 체인은 최대 3-4단 — 32 는 순환 가드.
        for _ in 0..32 {
            let token = self
                .get(&current)
                .ok_or_else(|| ResolveError::Missing(current.clone()))?;
            let raw = match (mode, &token.latte) {
                (ThemeMode::Latte, Some(latte)) => latte.as_str(),
                _ => token.value.as_str(),
            };
            match alias_target(raw) {
                Some(target) => current = target.to_string(),
                None => return Ok(raw.to_string()),
            }
        }
        Err(ResolveError::Cycle(path.to_string()))
    }
}

// ============================================================================
//  에러
// ============================================================================

#[derive(Debug)]
pub enum ParseError {
    Json(serde_json::Error),
    /// 구조가 기대(3 tier 그룹 / `$type`+`$value` 문자열)와 다름.
    Structure(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Json(e) => write!(f, "invalid JSON: {e}"),
            ParseError::Structure(msg) => write!(f, "unexpected DTCG structure: {msg}"),
        }
    }
}

impl std::error::Error for ParseError {}

impl From<serde_json::Error> for ParseError {
    fn from(e: serde_json::Error) -> Self {
        ParseError::Json(e)
    }
}

#[derive(Debug)]
pub enum ResolveError {
    /// alias 가 존재하지 않는 토큰을 가리킴.
    Missing(String),
    /// alias 순환 (또는 비정상적으로 긴 체인).
    Cycle(String),
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::Missing(path) => write!(f, "alias target not found: {path}"),
            ResolveError::Cycle(path) => write!(f, "alias cycle detected from: {path}"),
        }
    }
}

impl std::error::Error for ResolveError {}

// ============================================================================
//  파서
// ============================================================================

/// DTCG json 텍스트를 파싱한다. 최상위 `primitive`/`semantic`/`component` 3그룹 필수.
pub fn parse(text: &str) -> Result<TokenSet, ParseError> {
    let root: serde_json::Value = serde_json::from_str(text)?;
    let obj = root
        .as_object()
        .ok_or_else(|| ParseError::Structure("root is not an object".into()))?;

    let mut tokens = BTreeMap::new();
    for tier in Tier::ALL {
        let group = obj
            .get(tier.as_str())
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                ParseError::Structure(format!("missing tier group `{}`", tier.as_str()))
            })?;
        collect(tier, "", group, &mut tokens)?;
    }
    Ok(TokenSet { tokens })
}

/// `$value`/latte 오버라이드 스칼라를 문자열로 정규화한다. DTCG는 `$type`에 따라
/// number/boolean 을 raw JSON 스칼라로 export 할 수 있으므로, 문자열 외에도
/// 받아들인다(예: `$type: "number"` 토큰의 `$value: 0.4`).
fn json_scalar_to_string(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn collect(
    tier: Tier,
    prefix: &str,
    group: &serde_json::Map<String, serde_json::Value>,
    out: &mut BTreeMap<String, Token>,
) -> Result<(), ParseError> {
    for (key, val) in group {
        if key.starts_with('$') {
            continue;
        }
        // 실물은 flat 이지만, 중첩 그룹이 생겨도 alias 경로 문법(`.` 구분)과
        // 일치하도록 재귀 수집한다.
        let name = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        let node = val.as_object().ok_or_else(|| {
            ParseError::Structure(format!("{}.{name}: not an object", tier.as_str()))
        })?;
        if node.contains_key("$value") {
            let ty = node
                .get("$type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    ParseError::Structure(format!("{}.{name}: missing $type", tier.as_str()))
                })?
                .to_string();
            let value = node
                .get("$value")
                .and_then(json_scalar_to_string)
                .ok_or_else(|| {
                    ParseError::Structure(format!(
                        "{}.{name}: $value is not a string/number/bool",
                        tier.as_str()
                    ))
                })?;
            let latte = node
                .get("$extensions")
                .and_then(|e| e.get("com.tasty.mode"))
                .and_then(|m| m.get("latte"))
                .and_then(json_scalar_to_string);
            let path = format!("{}.{name}", tier.as_str());
            let token = Token {
                tier,
                name,
                ty,
                value,
                latte,
            };
            if out.insert(path.clone(), token).is_some() {
                return Err(ParseError::Structure(format!("duplicate token: {path}")));
            }
        } else {
            collect(tier, &name, node, out)?;
        }
    }
    Ok(())
}

// ============================================================================
//  코드 생성
// ============================================================================

/// 생성 결과. `files` 는 `src/generated/` 밑에 쓸 (파일명, 내용) — 결정적 순서.
/// `type_appearance_files` 는 `crates/tasty-type-appearance/src/` 밑에 쓸 (파일명,
/// 내용) — semantic 색·component 접근자는 `&Theme` 경유가 강제라 type-appearance
/// 안에 산출해야 한다(런타임 의존 방향 보존, 04/05 생성기 확장 설계 참조).
/// `skips` 는 생성에서 제외한 토큰의 사유 로그.
#[derive(Debug)]
pub struct Generated {
    pub files: Vec<(&'static str, String)>,
    pub type_appearance_files: Vec<(&'static str, String)>,
    pub skips: Vec<String>,
}

/// 토큰 하나의 Rust 표현. 값은 mocha(`$value`) 터미널 리터럴 기준 —
/// 치수·타이포·모션은 테마 불변이라 latte 분기가 없다.
#[derive(Debug, Clone, Copy, PartialEq)]
enum RustKind {
    /// dimension (px 또는 무단위) → `LogicalPx`.
    Px(f32),
    /// duration → `f32` (ms).
    Ms(f32),
    /// number (line-height / scale / opacity …) → `f32`.
    Num(f32),
    /// fontWeight → `u16`.
    Weight(u16),
}

impl RustKind {
    fn type_name(self) -> &'static str {
        match self {
            RustKind::Px(_) => "LogicalPx",
            RustKind::Ms(_) | RustKind::Num(_) => "f32",
            RustKind::Weight(_) => "u16",
        }
    }

    fn literal(self) -> String {
        match self {
            RustKind::Px(v) => format!("LogicalPx({v:?})"),
            RustKind::Ms(v) | RustKind::Num(v) => format!("{v:?}"),
            RustKind::Weight(v) => format!("{v}"),
        }
    }
}

/// 생성 제외 사유. `Color` 는 요약 한 줄로, 나머지는 토큰별 로그.
#[derive(Debug)]
enum Skip {
    /// 색 — 런타임 테마 시스템이 SSoT (시리즈 04/05).
    Color,
    /// 시리즈 01 범위 밖 `$type` (fontFamily / shadow / cubicBezier).
    Type(String),
    /// em 단위 dimension (letter-spacing 계열) — `LogicalPx` 로 표현 불가.
    EmUnit(String),
    /// 터미널 리터럴을 숫자로 파싱 실패.
    Unparsable(String),
}

/// component 토큰 이름 → 하위 모듈 그룹. 최장 일치 우선
/// (예: `switch-overlay-*` 는 `switch` 가 아니라 `switch_overlay`).
/// 목록에 없으면 첫 세그먼트로 fallback.
const COMPONENT_GROUPS: &[&str] = &[
    "icon-button",
    "help-hint",
    "status-dot",
    "switch-overlay",
    "tree-row",
    "plugins-list",
    "button",
    "badge",
    "tag",
    "kbd",
    "input",
    "select",
    "checkbox",
    "switch",
    "tab",
    "menu",
    "toast",
    "tooltip",
    "banner",
    "spinner",
    "table",
    "settings",
    "swatch",
    "remote",
    "preset",
    "titlebar",
    "sidebar",
];

/// component 토큰 이름을 (모듈명 snake_case, 모듈 내 나머지 kebab) 으로 분해.
fn component_split(name: &str) -> (String, String) {
    let mut best: Option<&str> = None;
    for p in COMPONENT_GROUPS {
        if name.len() > p.len()
            && name.starts_with(p)
            && name.as_bytes()[p.len()] == b'-'
            && best.is_none_or(|b| p.len() > b.len())
        {
            best = Some(p);
        }
    }
    match best {
        Some(p) => (p.replace('-', "_"), name[p.len() + 1..].to_string()),
        None => match name.split_once('-') {
            Some((head, rest)) => (head.replace('-', "_"), rest.to_string()),
            None => (name.replace('-', "_"), name.to_string()),
        },
    }
}

/// kebab-case → SCREAMING_SNAKE_CASE 상수명.
fn const_name(kebab: &str) -> String {
    kebab.replace(['-', '.'], "_").to_uppercase()
}

/// 토큰 분류: 생성 대상이면 Rust 표현, 아니면 제외 사유.
fn classify(set: &TokenSet, token: &Token) -> Result<RustKind, Skip> {
    let terminal = match set.resolve(&token.path(), ThemeMode::Mocha) {
        Ok(v) => v,
        Err(e) => return Err(Skip::Unparsable(e.to_string())),
    };
    match token.ty.as_str() {
        "color" => Err(Skip::Color),
        "dimension" => {
            if terminal.ends_with("em") {
                return Err(Skip::EmUnit(terminal));
            }
            let stripped = terminal.strip_suffix("px").unwrap_or(&terminal);
            stripped
                .trim()
                .parse::<f32>()
                .map(RustKind::Px)
                .map_err(|_| Skip::Unparsable(terminal.clone()))
        }
        "duration" => {
            let stripped = terminal.strip_suffix("ms").unwrap_or(&terminal);
            stripped
                .trim()
                .parse::<f32>()
                .map(RustKind::Ms)
                .map_err(|_| Skip::Unparsable(terminal.clone()))
        }
        "number" => terminal
            .trim()
            .parse::<f32>()
            .map(RustKind::Num)
            .map_err(|_| Skip::Unparsable(terminal.clone())),
        "fontWeight" => terminal
            .trim()
            .parse::<u16>()
            .map(RustKind::Weight)
            .map_err(|_| Skip::Unparsable(terminal.clone())),
        other => Err(Skip::Type(other.to_string())),
    }
}

/// 참조 표현식: `from` 컨텍스트에서 `target` 토큰의 생성 상수를 가리키는 경로.
fn const_expr(from: Tier, target: &Token) -> String {
    match target.tier {
        Tier::Primitive => {
            let n = const_name(&target.name);
            match from {
                Tier::Primitive => n,
                Tier::Semantic => format!("super::primitive::{n}"),
                Tier::Component => format!("crate::generated::primitive::{n}"),
            }
        }
        Tier::Semantic => {
            let n = const_name(&target.name);
            match from {
                Tier::Semantic => n,
                _ => format!("crate::generated::semantic::{n}"),
            }
        }
        Tier::Component => {
            let (module, rest) = component_split(&target.name);
            let n = const_name(&rest);
            match from {
                Tier::Component => format!("super::{module}::{n}"),
                _ => format!("crate::generated::component::{module}::{n}"),
            }
        }
    }
}

/// 한 토큰의 `pub const` 라인(들)을 만든다. doc 주석에 토큰 경로·체인·터미널 값 명기.
fn emit_const(
    set: &TokenSet,
    kinds: &BTreeMap<String, Result<RustKind, Skip>>,
    token: &Token,
    kind: RustKind,
    vis: &str,
    name: &str,
    indent: &str,
) -> String {
    let terminal = set
        .resolve(&token.path(), ThemeMode::Mocha)
        .expect("classify 통과 토큰은 resolve 가능");
    // sentinel (radius-full 계열): 9999px 는 "완전 원형" 상한값이지 실측 치수가 아니다.
    let sentinel = if terminal == "9999px" {
        " (sentinel — 완전 원형용 상한값)"
    } else {
        ""
    };

    let (doc, expr) = match alias_target(&token.value) {
        Some(target_path) => {
            let doc = format!(
                "`{}` → `{}` = {terminal}{sentinel}",
                token.path(),
                token.value
            );
            // 대상이 같은 kind 로 생성됐을 때만 상수 참조 — 아니면 리터럴 fallback.
            let target_generated = set
                .get(target_path)
                .filter(|t| matches!(kinds.get(&t.path()), Some(Ok(k)) if *k == kind));
            let expr = match target_generated {
                Some(t) => const_expr(token.tier, t),
                None => kind.literal(),
            };
            (doc, expr)
        }
        None => (
            format!("`{}` = {terminal}{sentinel}", token.path()),
            kind.literal(),
        ),
    };
    let unit = match kind {
        RustKind::Ms(_) => " (ms)",
        _ => "",
    };
    let ty = kind.type_name();
    // rustfmt(max_width 100) 와 같은 줄바꿈을 생성기가 직접 낸다 — 생성물은 커밋되고
    // `cargo fmt --check` 게이트를 지나가는데, freshness 테스트가 "생성기 출력 == 커밋된
    // 텍스트" 를 요구하므로 사후 rustfmt 로 고칠 수 없다(고치면 다음 생성에서 다시 어긋난다).
    let one_line = format!("{indent}{vis} const {name}: {ty} = {expr};");
    let decl = if one_line.chars().count() > 100 {
        format!("{indent}{vis} const {name}: {ty} =\n{indent}    {expr};")
    } else {
        one_line
    };
    format!("{indent}/// {doc}{unit}\n{decl}\n")
}

// ============================================================================
//  Component 접근자 (치수 + 색) — 04
// ============================================================================
//
// `generated::component` 의 raw const 는 zoom 을 모른다 (테마 불변 스케일).
// 위젯이 그 const 를 직접 읽으면 `Theme::with_colors_and_zoom()` 의 zoom
// resolve/제외 정책을 우회한다 — 그래서 component 토큰은 `&Theme` 경유
// 접근자로만 노출한다 (`crates/tasty-type-appearance/src/generated_component.rs`).

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
    ("semantic.border-strong", "border_strong()"),
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
    // 상태 표시 (status-*)
    // status-idle: idle/inactive 인디케이터 톤. 값상 text-placeholder 와 같은
    // neutral-600 이지만 필드는 `overlay0` 로 종착한다 — `placeholder` 는 텍스트
    // 입력 전용 필드라 사용자가 독립적으로 덮어쓸 수 있고, 인디케이터 도트가 그
    // 오버라이드를 따라가는 것은 의도가 아니다.
    ("semantic.status-idle", "status_idle", "overlay0"),
    // 보더 (border-*)
    ("semantic.border-default", "border_default", "surface0"),
    ("semantic.border-strong", "border_strong", "surface1"),
    ("semantic.border-focus", "border_focus", "blue"),
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
fn accessor_fn_name(name: &str) -> String {
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
fn generate_component_accessors(set: &TokenSet) -> (String, Vec<String>) {
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
            // number/fontWeight component 토큰 — 04 범위 밖. 테마 불변이고 무단위라
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
// `&Theme` 의 primitive 필드를 그대로 반환 — component 색 접근자(04)가 이 semantic
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
fn generate_semantic_color_accessors(set: &TokenSet) -> (String, Vec<String>) {
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

/// 파싱된 토큰셋에서 `src/generated/` 파일 4개 + `tasty-type-appearance` 접근자
/// 파일 2개(semantic 색 + component)를 생성한다. 출력은 입력에만 의존하는 결정적 텍스트
/// (타임스탬프 없음 — freshness 테스트 전제).
pub fn generate(set: &TokenSet) -> Generated {
    let mut skips: Vec<String> = Vec::new();
    let mut color_count = 0usize;

    // 1) 전 토큰 분류 (참조 emit 시 대상 kind 대조에 필요).
    let mut kinds: BTreeMap<String, Result<RustKind, Skip>> = BTreeMap::new();
    for token in set.iter() {
        kinds.insert(token.path(), classify(set, token));
    }
    for (path, kind) in &kinds {
        match kind {
            Ok(_) => {}
            Err(Skip::Color) => color_count += 1,
            Err(Skip::Type(ty)) => {
                skips.push(format!("{path}: $type {ty} — 시리즈 01 생성 보류"));
            }
            Err(Skip::EmUnit(v)) => {
                skips.push(format!(
                    "{path}: em 단위 ({v}) — LogicalPx 표현 불가, 생성 스킵"
                ));
            }
            Err(Skip::Unparsable(v)) => {
                skips.push(format!("{path}: 터미널 값 파싱 실패 ({v}) — 생성 스킵"));
            }
        }
    }
    if color_count > 0 {
        skips.push(format!(
            "color 토큰 {color_count}개 — 런타임 테마 시스템이 SSoT, 생성하지 않음 (시리즈 04/05)"
        ));
    }

    let header = |tier_doc: &str| {
        format!(
            "//! Generated from `dtcg/tasty.tokens.json` — DO NOT EDIT.\n\
             //! 재생성: `cargo run -p tasty-design-tokens --bin generate`.\n\
             //!\n\
             {tier_doc}\n"
        )
    };

    // 2) primitive.rs — pub(crate).
    let mut primitive = header(
        "//! Tier 1 — primitive 치수 스케일. **`pub(crate)`**: \"UI 는 primitive 를 직접\n\
         //! 읽지 않는다\"(3-tier 계약)를 visibility 로 컴파일 타임 강제한다.\n\
         //! 외부 crate 는 `semantic` / `component` 를 경유할 것.",
    );
    primitive.push_str("#![allow(dead_code)] // 스케일 전체를 보존한다 — 미참조 엔트리 포함.\n\n");
    primitive.push_str("use tasty_type_geometry::length::LogicalPx;\n");
    for token in set.iter().filter(|t| t.tier == Tier::Primitive) {
        if let Some(Ok(kind)) = kinds.get(&token.path()) {
            primitive.push('\n');
            primitive.push_str(&emit_const(
                set,
                &kinds,
                token,
                *kind,
                "pub(crate)",
                &const_name(&token.name),
                "",
            ));
        }
    }

    // 3) semantic.rs — pub, primitive 참조.
    let mut semantic = header(
        "//! Tier 2 — semantic 치수/타이포/모션 (테마 불변). primitive 참조로 정의된다.\n\
         //! 색 semantic 은 생성하지 않는다 — 런타임 테마 시스템(`tasty-themes`)이 SSoT.\n\
         //!\n\
         //! **zoom 주의**: 이 const 들은 `SIZING` 초기값·정합 테스트용이다. 런타임\n\
         //! 소비는 반드시 `&Theme` 필드/접근자 경유 (`with_colors_and_zoom` 의 zoom\n\
         //! resolve 를 우회하지 말 것).",
    );
    semantic.push('\n');
    semantic.push_str("use tasty_type_geometry::length::LogicalPx;\n");
    for token in set.iter().filter(|t| t.tier == Tier::Semantic) {
        if let Some(Ok(kind)) = kinds.get(&token.path()) {
            semantic.push('\n');
            semantic.push_str(&emit_const(
                set,
                &kinds,
                token,
                *kind,
                "pub",
                &const_name(&token.name),
                "",
            ));
        }
    }

    // 4) component.rs — pub, 컴포넌트별 하위 모듈.
    let mut by_module: BTreeMap<String, Vec<(&Token, RustKind, String)>> = BTreeMap::new();
    for token in set.iter().filter(|t| t.tier == Tier::Component) {
        if let Some(Ok(kind)) = kinds.get(&token.path()) {
            let (module, rest) = component_split(&token.name);
            by_module
                .entry(module)
                .or_default()
                .push((token, *kind, const_name(&rest)));
        }
    }
    let mut component = header(
        "//! Tier 3 — component 치수 (테마 불변), 컴포넌트별 하위 모듈. semantic (일부는\n\
         //! primitive 직접 — 디자인 실물의 tier-skip alias) 참조로 정의된다.\n\
         //! 색 component 접근자는 시리즈 04 에서 결정.\n\
         //!\n\
         //! **zoom 주의**: 런타임 소비는 반드시 `&Theme` 경유 — `semantic.rs` 참조.",
    );
    for (module, entries) in &by_module {
        component.push('\n');
        component.push_str(&format!("pub mod {module} {{\n"));
        let uses_px = entries.iter().any(|(_, k, _)| matches!(k, RustKind::Px(_)));
        if uses_px {
            component.push_str("    use tasty_type_geometry::length::LogicalPx;\n");
        }
        for (token, kind, name) in entries {
            component.push('\n');
            component.push_str(&emit_const(set, &kinds, token, *kind, "pub", name, "    "));
        }
        component.push_str("}\n");
    }

    // 5) mod.rs.
    let module_root =
        header("//! 생성 모듈 루트. `primitive` 는 `pub(crate)` — tier 규율의 컴파일 타임 강제.")
            + "\npub(crate) mod primitive;\n\npub mod semantic;\n\npub mod component;\n";

    // 6) tasty-type-appearance/src/semantic_color_generated.rs — semantic 색 접근자 (05-A).
    let (semantic_color_accessors, semantic_color_skips) = generate_semantic_color_accessors(set);
    skips.extend(semantic_color_skips);

    // 7) tasty-type-appearance/src/generated_component.rs — component 접근자.
    let (component_accessors, accessor_skips) = generate_component_accessors(set);
    skips.extend(accessor_skips);

    Generated {
        files: vec![
            ("mod.rs", module_root),
            ("primitive.rs", primitive),
            ("semantic.rs", semantic),
            ("component.rs", component),
        ],
        type_appearance_files: vec![
            ("semantic_color_generated.rs", semantic_color_accessors),
            ("generated_component.rs", component_accessors),
        ],
        skips,
    }
}
