//! W3C DTCG 토큰 파서 + alias 해석기 + Rust 코드 생성기.
//!
//! vendor 실물(`dtcg/tasty.tokens.json`)은 strict DTCG 가 아니다 — 모든 `$value`
//! 가 문자열이고(`"1.4"`, `"500"`), dimension 에 `"12px"` / `"0"`(무단위) /
//! `"0.04em"`(em) 이 혼재하며, shadow/cubicBezier 는 CSS 문자열이다. 파서는 이
//! 실물 형식을 기준으로 한다.
//!
//! alias 는 `{tier.name}` 문법. 디자인 계약(TOKENS.md)과 달리 실물에는
//! component → primitive 직접 참조 등 tier-skip alias 가 실존하므로, 해석기는
//! 임의 tier 간 참조 + 다단 체인을 허용한다. tier 규율의 강제는 생성물
//! visibility(`generated::primitive` = `pub(crate)`)로만 수행한다.

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
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    ParseError::Structure(format!(
                        "{}.{name}: $value is not a string",
                        tier.as_str()
                    ))
                })?
                .to_string();
            let latte = node
                .get("$extensions")
                .and_then(|e| e.get("com.tasty.mode"))
                .and_then(|m| m.get("latte"))
                .and_then(|v| v.as_str())
                .map(String::from);
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
/// `skips` 는 생성에서 제외한 토큰의 사유 로그.
#[derive(Debug)]
pub struct Generated {
    pub files: Vec<(&'static str, String)>,
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
    format!(
        "{indent}/// {doc}{unit}\n{indent}{vis} const {name}: {ty} = {expr};\n",
        ty = kind.type_name()
    )
}

/// 파싱된 토큰셋에서 `src/generated/` 파일 4개를 생성한다.
/// 출력은 입력에만 의존하는 결정적 텍스트 (타임스탬프 없음 — freshness 테스트 전제).
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

    Generated {
        files: vec![
            ("mod.rs", module_root),
            ("primitive.rs", primitive),
            ("semantic.rs", semantic),
            ("component.rs", component),
        ],
        skips,
    }
}
