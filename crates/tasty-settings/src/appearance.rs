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

/// plugin_settings[plugin_id][storage_key]에 저장할 TOML 스칼라 값.
/// Bool·Text·Number는 매니페스트의 Toggle·Select·Number 항목에 대응한다.
/// 폰트의 plugin_font_overrides와는 별개다.
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
    /// 터미널 폰트의 합자 표시 여부. 기본값은 켜짐이다.
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
    /// Surface kind별 폰트 설정. 지정하지 않은 필드는 default_font를 따른다.
    #[serde(default)]
    pub plugin_font_overrides: HashMap<String, FontOverride>,
    /// 호환용 markdown 설정. 로드 때 plugin_font_overrides로 옮기고 다시 저장하지 않는다.
    /// 옛 설정을 잃지 않도록 이 읽기를 제거하기 전에 배포 호환성을 확인해야 한다.
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

    /// 설정과 프리뷰가 공유하는 UI 배율. 지원값을 바꾸면 tasty-type-appearance의
    /// SUPPORTED_ZOOMS 시험 사본도 맞춘다. the_supported_ui_scale_set_is_pinned가 변경을 확인한다.
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

    /// 호환용 폰트 필드를 plugin_font_overrides로 옮긴다. 이미 맵에 있는 값이 우선한다.
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

/// 설정에서 선택할 UI 배율 이름. 정규화는 나머지를 medium으로 바꾼다.
pub const UI_SCALE_CHOICES: &[&str] = &["small", "medium", "large"];

#[cfg(test)]
mod tests {
    use super::*;

    /// 하위 크레이트의 배율 시험 사본과 맞춰 지원 배율을 확인한다.
    /// 목록 밖 이름은 기본 배율로 처리하는지도 확인한다.
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
             · docs/design/systems/theme.md#토큰에-없는-값과-배율 의 굵기 축 서술 — 새 배율에서 `border_width`(1) 가 \
             `(1 * z).round() != 1` 이 되면 그 축에도 대가가 생긴다"
        );

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
        assert_eq!(parsed.markdown_font.font_family.as_deref(), Some("Iosevka"));
        parsed.migrate_legacy_font_overrides();
        assert!(parsed.markdown_font.is_empty());
        let md_ov = parsed.plugin_font_overrides.get("markdown").unwrap();
        assert_eq!(md_ov.font_family.as_deref(), Some("Iosevka"));
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
