use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tasty_themes::{ThemeApplyContext, mocha_fallback_colors};
use tasty_type_appearance::theme::{PartialColors, ThemeColors};
use tasty_type_geometry::length::LogicalPx;

pub use tasty_type_appearance::color::HexColor;

/// `AppearanceSettings::ligatures` serde 기본값 — 디자인은 ligatures on 이 기본.
fn default_ligatures() -> bool {
    true
}

/// Plugin-contributed settings page 의 generic 값. host 가 `Settings::plugin_settings`
/// 의 `[plugin_id][storage_key]` 슬롯에 저장한다 (FontOverride 의 전역
/// `plugin_font_overrides` 슬롯과 **별개 네임스페이스**). manifest 의
/// `SettingsItemDecl::{Toggle,Select,Number}` 가 각각 `Bool`/`Text`/`Number` 로 매핑된다.
///
/// `#[serde(untagged)]` — TOML 스칼라(`true` / `100.0` / `"follow"`)로 그대로 저장돼
/// 손편집/디버깅이 자연스럽다. host 는 항상 타입을 맞춰 write 하므로 round-trip 안정적.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PluginSettingValue {
    Bool(bool),
    Number(f64),
    Text(String),
}

/// Active tab indicator style for the app-chrome tab bar (Appearance › Tasty).
///
/// Mutually exclusive — the renderer draws exactly one marker per active tab.
/// Default `Underline` keeps the accent line that has always marked the active
/// tab; serde `default` makes older settings files migrate safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ActiveTabIndicator {
    /// Accent underline; tab background matches inactive tabs.
    #[default]
    Underline,
    /// Filled tab background; no underline.
    Fill,
    /// Small accent dot marker; tab background matches inactive tabs.
    Dot,
}

/// Default font settings applied when a surface override field is `None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FontSettings {
    pub font_family: String,
    pub font_size: f32,
    pub custom_font_path: String,
    pub line_height: f32,
    /// "auto" | "fixed"
    pub font_scale_mode: String,
}

impl Default for FontSettings {
    fn default() -> Self {
        Self {
            font_family: String::new(),
            font_size: 14.0,
            custom_font_path: String::new(),
            line_height: 1.0,
            font_scale_mode: "auto".to_string(),
        }
    }
}

/// Per-surface font override. `None` for any field falls back to `FontSettings`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FontOverride {
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub custom_font_path: Option<String>,
    pub line_height: Option<f32>,
    pub font_scale_mode: Option<String>,
}

/// Resolved font values after applying a `FontOverride` to a `FontSettings`.
#[derive(Debug, Clone)]
pub struct EffectiveFont {
    pub font_family: String,
    pub font_size: f32,
    pub custom_font_path: String,
    pub line_height: f32,
    pub font_scale_mode: String,
}

impl EffectiveFont {
    /// Compute font size considering scale_factor and font_scale_mode.
    pub fn effective_font_size(&self, scale_factor: f32) -> f32 {
        match self.font_scale_mode.as_str() {
            "auto" => self.font_size * scale_factor,
            _ => self.font_size,
        }
    }
}

impl FontOverride {
    /// `true` when every field is `None` — i.e. nothing actually overrides the default.
    pub(crate) fn is_empty(&self) -> bool {
        self.font_family.is_none()
            && self.font_size.is_none()
            && self.custom_font_path.is_none()
            && self.line_height.is_none()
            && self.font_scale_mode.is_none()
    }
}

impl FontSettings {
    /// Apply per-field override. `None` fields fall back to defaults.
    pub fn apply_override(&self, ov: &FontOverride) -> EffectiveFont {
        EffectiveFont {
            font_family: ov
                .font_family
                .clone()
                .unwrap_or_else(|| self.font_family.clone()),
            font_size: ov.font_size.unwrap_or(self.font_size),
            custom_font_path: ov
                .custom_font_path
                .clone()
                .unwrap_or_else(|| self.custom_font_path.clone()),
            line_height: ov.line_height.unwrap_or(self.line_height),
            font_scale_mode: ov
                .font_scale_mode
                .clone()
                .unwrap_or_else(|| self.font_scale_mode.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceSettings {
    /// 현재 선택된 테마 id (파일명 stem = `~/.tasty/themes/<id>.toml`).
    pub theme: String,
    /// 누적된 테마 기본값. 테마 변경 시 새 테마의 partial 이 이 위에 덮어쓰여진다.
    /// 첫 실행 시 mocha 풀 세트로 시드.
    pub theme_base: ThemeColors,
    /// 사용자가 픽커로 직접 손댄 흔적. 테마 변경 시 클리어.
    pub theme_overrides: PartialColors,
    /// 현재 라이트/다크 플래그. 테마 파일이 명시하면 그 값으로 갱신.
    pub theme_is_light: bool,
    pub background_opacity: f32,
    /// 폰트 합자(ligature) 사용 여부. 디자인 settings_window.jsx:225 Ligatures
    /// Switch (기본 on). 번들 D2Coding ligature 폰트의 합자 표시를 제어한다.
    #[serde(default = "default_ligatures")]
    pub ligatures: bool,
    pub sidebar_width: LogicalPx,
    /// UI scale: "small", "medium", or "large". Affects all egui UI elements.
    pub ui_scale: String,
    /// Active tab indicator style (Underline / Fill / Dot) for the tab bar.
    #[serde(default)]
    pub active_tab_indicator: ActiveTabIndicator,
    /// 탭 바 base 너비 (logical px). 모니터 scale 은 egui 가 자동 반영 (= auto).
    pub tab_width: f32,
    /// 탭 라벨 base 폰트 크기 (logical px). 모니터 scale 은 egui 가 자동 반영.
    pub tab_font_size: f32,
    /// Default font settings applied when a surface override is unset.
    pub default_font: FontSettings,
    /// Terminal surface font override (per-field). Host-rendered terminal core uses this.
    pub terminal_font: FontOverride,
    /// Per-plugin (surface-kind) font overrides. Key = surface kind string
    /// (e.g. `"markdown"`, `"explorer"`). Single source of truth for surface
    /// font overrides — the host stays kind-agnostic and never names a specific
    /// kind in the *live* font path.
    #[serde(default)]
    pub plugin_font_overrides: HashMap<String, FontOverride>,
    /// Legacy markdown override. Read on load and migrated into
    /// `plugin_font_overrides["markdown"]`; never written back.
    ///
    /// The `migrate_legacy_font_overrides` reader is transitional back-compat:
    /// the migration has **not** shipped in a tagged release yet, so users of the
    /// last release (v0.3.1) still keep their markdown/explorer font override in
    /// these top-level `[markdown_font]`/`[explorer_font]` sections. We keep these
    /// fields as a one-shot config-migration reader until the migration reaches a
    /// release; the next cycle after that ships can remove them (removal trigger).
    /// The live font logic itself is fully generic over `plugin_font_overrides`.
    #[serde(default, skip_serializing)]
    pub markdown_font: FontOverride,
    /// Legacy explorer override. Read on load and migrated into
    /// `plugin_font_overrides["explorer"]`; never written back. Same transitional
    /// back-compat rationale as [`Self::markdown_font`].
    #[serde(default, skip_serializing)]
    pub explorer_font: FontOverride,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: "mocha".to_string(),
            theme_base: mocha_fallback_colors(),
            theme_overrides: PartialColors::default(),
            theme_is_light: false,
            background_opacity: 1.0,
            ligatures: default_ligatures(),
            sidebar_width: LogicalPx(180.0),
            ui_scale: "medium".to_string(),
            active_tab_indicator: ActiveTabIndicator::default(),
            tab_width: 150.0,
            tab_font_size: 11.0,
            default_font: FontSettings::default(),
            terminal_font: FontOverride::default(),
            plugin_font_overrides: HashMap::new(),
            markdown_font: FontOverride::default(),
            explorer_font: FontOverride::default(),
        }
    }
}

impl ThemeApplyContext for AppearanceSettings {
    fn theme_id(&self) -> &str {
        &self.theme
    }
    fn set_theme_id(&mut self, id: &str) {
        self.theme = id.to_string();
    }
    fn theme_base(&self) -> &ThemeColors {
        &self.theme_base
    }
    fn theme_base_mut(&mut self) -> &mut ThemeColors {
        &mut self.theme_base
    }
    fn theme_overrides(&self) -> &PartialColors {
        &self.theme_overrides
    }
    fn theme_overrides_mut(&mut self) -> &mut PartialColors {
        &mut self.theme_overrides
    }
    fn theme_is_light(&self) -> bool {
        self.theme_is_light
    }
    fn set_theme_is_light(&mut self, v: bool) {
        self.theme_is_light = v;
    }
}

impl AppearanceSettings {
    /// Get the UI scale factor based on the ui_scale setting.
    pub fn ui_scale_factor(&self) -> f32 {
        Self::ui_scale_factor_for(&self.ui_scale)
    }

    /// UI scale 배율의 단일 출처. 인스턴스의 `ui_scale_factor` 와 Display 설정의
    /// "Aa" 프리뷰가 공유한다 (배율 숫자가 한 곳에만 존재하도록).
    ///
    /// **배율 집합이 바뀌면 다른 크레이트의 사본도 같이 고쳐야 한다** —
    /// `tasty-type-appearance` 의 `zoom_cost_differs_by_axis` 는 이 셋을 하드코딩한
    /// 사본(`SUPPORTED_ZOOMS`)으로 돈다. 그쪽이 여기를 읽을 수 없어서(의존 방향이
    /// 반대다) 사본을 지울 수 없고, 대신 **이쪽에 핀을 둔다**:
    /// `the_supported_ui_scale_set_is_pinned`. 그 핀이 이 함수의 배율 집합이
    /// 움직이는 것을 잡는다.
    pub fn ui_scale_factor_for(scale: &str) -> f32 {
        match scale {
            "small" => 0.85,
            "large" => 1.2,
            _ => 1.0, // medium
        }
    }

    /// Get the sidebar width adjusted for UI scale.
    pub fn scaled_sidebar_width(&self) -> LogicalPx {
        match self.ui_scale.as_str() {
            "small" => LogicalPx(150.0),
            "large" => LogicalPx(220.0),
            _ => LogicalPx(180.0),
        }
    }

    /// Effective font for terminal surface.
    pub fn effective_terminal_font(&self) -> EffectiveFont {
        self.default_font.apply_override(&self.terminal_font)
    }

    /// Effective font for an arbitrary plugin surface kind (e.g. `"markdown"`,
    /// `"explorer"`). Falls back to `default_font` if no override is registered
    /// for the given kind.
    pub fn effective_font_for_kind(&self, kind: &str) -> EffectiveFont {
        match self.plugin_font_overrides.get(kind) {
            Some(ov) => self.default_font.apply_override(ov),
            None => self.default_font.apply_override(&FontOverride::default()),
        }
    }

    /// Migrate legacy `markdown_font` / `explorer_font` fields into
    /// `plugin_font_overrides`. Called by [`crate::Settings::load`] right after
    /// deserialization. Idempotent: an existing entry in `plugin_font_overrides`
    /// always wins over the legacy field.
    ///
    /// Transitional back-compat: the migration has not shipped in a tagged
    /// release yet, so the last release (v0.3.1) writes overrides into the
    /// top-level `[markdown_font]`/`[explorer_font]` sections. Removing this
    /// reader before the migration reaches a release would silently drop those
    /// users' font overrides on their next save.
    pub fn migrate_legacy_font_overrides(&mut self) {
        let legacy = [
            ("markdown", std::mem::take(&mut self.markdown_font)),
            ("explorer", std::mem::take(&mut self.explorer_font)),
        ];
        for (key, ov) in legacy {
            if ov.is_empty() {
                continue;
            }
            if self.plugin_font_overrides.contains_key(key) {
                continue;
            }
            self.plugin_font_overrides.insert(key.to_string(), ov);
        }
    }
}

/// `ui_scale` 로 **도달 가능한** 값의 전부. 설정 로드 시 정규화가 이 목록 밖의 값을
/// `"medium"` 으로 접으므로, 여기 없는 이름은 배율로 실현되지 않는다.
///
/// 목록과 [`AppearanceSettings::ui_scale_factor_for`] 의 match 는 **따로 움직일 수
/// 있다** — 그래서 둘을 대조하는 핀이 있다(`the_supported_ui_scale_set_is_pinned`).
pub const UI_SCALE_CHOICES: &[&str] = &["small", "medium", "large"];

#[cfg(test)]
mod tests {
    use super::*;

    /// **지원 배율 집합을 못박는다.** 이 셋이 움직이면 `border_width`(1) 가 배율
    /// 가변이 되는지 여부가 바뀌고, 그 위에 선 ADR-0126 의 "굵기 축엔 대가가 없다"
    /// 가 조건부가 된다.
    ///
    /// 이 핀이 필요한 이유는 **소비자가 여기를 읽을 수 없기 때문**이다.
    /// `tasty-type-appearance::theme` 의 `zoom_cost_differs_by_axis` 는
    /// `SUPPORTED_ZOOMS = [0.85, 1.0, 1.2]` 라는 하드코딩 사본으로 돈다 — 의존
    /// 방향이 반대라(이 크레이트가 그쪽에 의존한다) 사본을 없앨 수 없다. 그쪽 사본은
    /// **배율 집합이 바뀌어도 안 운다.** 우는 것은 이 핀이다.
    ///
    /// **이 핀이 못 잡는 것**: `ui_scale_factor_for` 의 match 에 `UI_SCALE_CHOICES`
    /// 에 없는 이름으로 팔을 더하는 경우. 다만 그런 이름은 정규화가 `"medium"` 으로
    /// 접어서 설정으로 도달할 수 없다 — 둘째 단언이 그 접힘을 확인한다.
    #[test]
    fn the_supported_ui_scale_set_is_pinned() {
        let mut factors: Vec<f32> = UI_SCALE_CHOICES
            .iter()
            .map(|s| AppearanceSettings::ui_scale_factor_for(s))
            .collect();
        factors.sort_by(|a, b| a.partial_cmp(b).expect("배율에 NaN 이 없다"));
        assert_eq!(
            factors,
            vec![0.85, 1.0, 1.2],
            "지원 배율 집합이 바뀌었다. 같이 고칠 자리가 둘이다:\n\
             · `crates/tasty-type-appearance/src/theme.rs` 의 `SUPPORTED_ZOOMS` 사본\n\
             · ADR-0126 의 굵기 축 서술 — 새 배율에서 `border_width`(1) 가 \
             `(1 * z).round() != 1` 이 되면 그 축에도 대가가 생긴다"
        );

        // 목록 밖 이름이 배율을 새로 만들지 못한다는 것 — 집합이 닫혀 있다는 쪽 근거.
        for unknown in ["huge", "tiny", ""] {
            assert_eq!(
                AppearanceSettings::ui_scale_factor_for(unknown),
                1.0,
                "목록 밖 `{unknown}` 이 medium 이 아닌 배율을 냈다 — 집합이 안 닫혔다"
            );
        }
    }

    #[test]
    fn plugin_setting_value_untagged_round_trip() {
        // `#[serde(untagged)]` 가 TOML 스칼라를 Bool/Number/Text 로 정확히 분류·복원하는지.
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct W {
            v: PluginSettingValue,
        }
        for v in [
            PluginSettingValue::Bool(true),
            PluginSettingValue::Number(100.0),
            PluginSettingValue::Text("follow".to_string()),
        ] {
            let w = W { v: v.clone() };
            let dumped = toml::to_string(&w).unwrap();
            let back: W = toml::from_str(&dumped).unwrap();
            assert_eq!(back.v, v, "round-trip changed value (dumped: {dumped:?})");
        }
        // 명시적 TOML 스칼라 → 기대 variant (분류 순서 확인).
        assert_eq!(
            toml::from_str::<W>("v = true").unwrap().v,
            PluginSettingValue::Bool(true)
        );
        assert_eq!(
            toml::from_str::<W>("v = 100.0").unwrap().v,
            PluginSettingValue::Number(100.0)
        );
        assert_eq!(
            toml::from_str::<W>(r#"v = "follow""#).unwrap().v,
            PluginSettingValue::Text("follow".to_string())
        );
    }

    #[test]
    fn apply_override_falls_back_to_defaults() {
        let defaults = FontSettings {
            font_family: "Default".to_string(),
            font_size: 14.0,
            custom_font_path: "/path/default.ttf".to_string(),
            line_height: 1.0,
            font_scale_mode: "fixed".to_string(),
        };
        let ov = FontOverride::default();
        let eff = defaults.apply_override(&ov);
        assert_eq!(eff.font_family, "Default");
        assert_eq!(eff.font_size, 14.0);
        assert_eq!(eff.custom_font_path, "/path/default.ttf");
        assert_eq!(eff.line_height, 1.0);
        assert_eq!(eff.font_scale_mode, "fixed");
    }

    #[test]
    fn apply_override_uses_overrides_when_set() {
        let defaults = FontSettings::default();
        let ov = FontOverride {
            font_family: Some("Override".to_string()),
            font_size: Some(20.0),
            custom_font_path: None,
            line_height: Some(1.5),
            font_scale_mode: Some("auto".to_string()),
        };
        let eff = defaults.apply_override(&ov);
        assert_eq!(eff.font_family, "Override");
        assert_eq!(eff.font_size, 20.0);
        assert_eq!(eff.custom_font_path, "");
        assert_eq!(eff.line_height, 1.5);
        assert_eq!(eff.font_scale_mode, "auto");
    }

    #[test]
    fn effective_font_size_modes() {
        let mut eff = EffectiveFont {
            font_family: String::new(),
            font_size: 16.0,
            custom_font_path: String::new(),
            line_height: 1.0,
            font_scale_mode: "fixed".to_string(),
        };
        assert_eq!(eff.effective_font_size(2.0), 16.0);
        eff.font_scale_mode = "auto".to_string();
        assert_eq!(eff.effective_font_size(2.0), 32.0);
    }

    #[test]
    fn new_structured_form_deserializes() {
        let toml_str = r#"
[default_font]
font_family = "Cascadia"
font_size = 15.0

[terminal_font]
font_size = 18.0

[markdown_font]
font_family = "Iosevka"
"#;
        let mut parsed: AppearanceSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.default_font.font_family, "Cascadia");
        assert_eq!(parsed.default_font.font_size, 15.0);
        assert_eq!(parsed.terminal_font.font_size, Some(18.0));
        // Legacy field deserializes pre-migration.
        assert_eq!(parsed.markdown_font.font_family.as_deref(), Some("Iosevka"));
        parsed.migrate_legacy_font_overrides();
        // After migration: markdown override lives only in plugin_font_overrides.
        assert!(parsed.markdown_font.is_empty());
        let md_ov = parsed.plugin_font_overrides.get("markdown").unwrap();
        assert_eq!(md_ov.font_family.as_deref(), Some("Iosevka"));
        // Effective values: terminal_font overrides only size, markdown only family.
        let eff_term = parsed.effective_terminal_font();
        assert_eq!(eff_term.font_family, "Cascadia");
        assert_eq!(eff_term.font_size, 18.0);
        let eff_md = parsed.effective_font_for_kind("markdown");
        assert_eq!(eff_md.font_family, "Iosevka");
        assert_eq!(eff_md.font_size, 15.0);
    }

    #[test]
    fn empty_toml_uses_defaults() {
        let parsed: AppearanceSettings = toml::from_str("").unwrap();
        assert_eq!(parsed.default_font.font_size, 14.0);
        assert!(parsed.terminal_font.font_size.is_none());
    }

    #[test]
    fn active_tab_indicator_defaults_to_underline() {
        assert_eq!(ActiveTabIndicator::default(), ActiveTabIndicator::Underline);
        let parsed: AppearanceSettings = toml::from_str("").unwrap();
        assert_eq!(parsed.active_tab_indicator, ActiveTabIndicator::Underline);
    }

    #[test]
    fn active_tab_indicator_round_trips() {
        let mut s = AppearanceSettings::default();
        s.active_tab_indicator = ActiveTabIndicator::Dot;
        let dumped = toml::to_string(&s).unwrap();
        assert!(dumped.contains("active_tab_indicator = \"dot\""));
        let reparsed: AppearanceSettings = toml::from_str(&dumped).unwrap();
        assert_eq!(reparsed.active_tab_indicator, ActiveTabIndicator::Dot);
    }

    #[test]
    fn legacy_markdown_font_migrates_to_plugin_overrides() {
        let toml_str = r#"
[default_font]
font_family = "Base"

[markdown_font]
font_family = "Iosevka"
font_size = 17.0
"#;
        let mut parsed: AppearanceSettings = toml::from_str(toml_str).unwrap();
        parsed.migrate_legacy_font_overrides();
        let ov = parsed
            .plugin_font_overrides
            .get("markdown")
            .expect("markdown migrated");
        assert_eq!(ov.font_family.as_deref(), Some("Iosevka"));
        assert_eq!(ov.font_size, Some(17.0));
        assert!(parsed.markdown_font.is_empty());
    }

    #[test]
    fn legacy_explorer_font_migrates_to_plugin_overrides() {
        let toml_str = r#"
[explorer_font]
font_family = "Mono"
line_height = 1.4
"#;
        let mut parsed: AppearanceSettings = toml::from_str(toml_str).unwrap();
        parsed.migrate_legacy_font_overrides();
        let ov = parsed
            .plugin_font_overrides
            .get("explorer")
            .expect("explorer migrated");
        assert_eq!(ov.font_family.as_deref(), Some("Mono"));
        assert_eq!(ov.line_height, Some(1.4));
        assert!(parsed.explorer_font.is_empty());
    }

    #[test]
    fn new_plugin_font_overrides_round_trip() {
        let toml_str = r#"
[default_font]
font_family = "Base"

[plugin_font_overrides.markdown]
font_family = "Iosevka"
font_size = 16.0
font_scale_mode = "fixed"
"#;
        let mut parsed: AppearanceSettings = toml::from_str(toml_str).unwrap();
        parsed.migrate_legacy_font_overrides();
        let dumped = toml::to_string(&parsed).unwrap();
        let mut reparsed: AppearanceSettings = toml::from_str(&dumped).unwrap();
        reparsed.migrate_legacy_font_overrides();
        let ov = reparsed
            .plugin_font_overrides
            .get("markdown")
            .expect("markdown survives round-trip");
        assert_eq!(ov.font_family.as_deref(), Some("Iosevka"));
        assert_eq!(ov.font_size, Some(16.0));
        assert_eq!(ov.font_scale_mode.as_deref(), Some("fixed"));
        // Legacy fields never re-emit.
        assert!(!dumped.contains("[markdown_font]"));
        assert!(!dumped.contains("[explorer_font]"));
    }

    #[test]
    fn effective_font_for_kind_falls_back_to_default() {
        let mut s = AppearanceSettings::default();
        s.default_font.font_family = "Default".to_string();
        s.default_font.font_size = 13.0;
        let eff = s.effective_font_for_kind("nonexistent");
        assert_eq!(eff.font_family, "Default");
        assert_eq!(eff.font_size, 13.0);
    }

    #[test]
    fn migration_does_not_overwrite_existing_plugin_override() {
        let toml_str = r#"
[markdown_font]
font_family = "Legacy"

[plugin_font_overrides.markdown]
font_family = "New"
"#;
        let mut parsed: AppearanceSettings = toml::from_str(toml_str).unwrap();
        parsed.migrate_legacy_font_overrides();
        let ov = parsed.plugin_font_overrides.get("markdown").unwrap();
        assert_eq!(
            ov.font_family.as_deref(),
            Some("New"),
            "explicit plugin_font_overrides wins over legacy field"
        );
        assert!(parsed.markdown_font.is_empty());
    }

    #[test]
    fn explicit_plugin_override_deserializes_per_kind() {
        // 여러 kind override 가 서로 독립적으로 역직렬화되는지 (generic per-kind).
        let toml_str = r#"
[plugin_font_overrides.markdown]
font_family = "Md"

[plugin_font_overrides.explorer]
font_size = 22.0
"#;
        let parsed: AppearanceSettings = toml::from_str(toml_str).unwrap();
        let eff_md = parsed.effective_font_for_kind("markdown");
        assert_eq!(eff_md.font_family, "Md");
        let eff_ex = parsed.effective_font_for_kind("explorer");
        assert_eq!(eff_ex.font_size, 22.0);
    }
}
