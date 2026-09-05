mod appearance;
mod keybindings;
mod port;
mod port_impl;
mod scripts;
mod types;

pub mod general;
pub mod testing;

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tasty_utils::path::tasty_home;

pub use appearance::{
    ActiveTabIndicator, AppearanceSettings, EffectiveFont, FontOverride, FontSettings, HexColor,
    PluginSettingValue, UI_SCALE_CHOICES,
};
pub use general::{DEFAULT_WHEEL_LINE_SCROLL, GeneralSettings, LinkModifier};
pub use keybindings::KeybindingSettings;
pub use port::SettingsStorage;
pub use port_impl::FileSettingsStorage;
pub use scripts::{
    AUTO_TRIGGER_EVENTS, AutoTrigger, ScriptEntry, ScriptRegistry, hash_bytes, hash_file,
    is_auto_trigger_event,
};
pub use types::{
    AccessibilitySettings, MemorySettings, ModifierHintSettings, NotificationSettings,
    OverlaySettings, PerformanceSettings, RemoteTransferSettings,
};

/// 이 `Settings` 값이 어디서 왔는지 — **원본 파일을 덮어써도 되는지**를 판정한다.
///
/// 파싱 실패를 기본값으로 폴백하는 것 자체는 앱을 계속 쓰게 해 주므로 옳다. 위험한
/// 것은 그 뒤의 저장이다: 폴백한 기본값을 원래 자리에 쓰면 사용자가 쓴 설정이 사라진다.
/// 그래서 로드 결과에 "덮어써도 되는가" 를 실어 [`Settings::save`] 가 판정하게 한다.
///
/// **로드는 파일을 건드리지 않는다.** 보존(백업으로 이동)은 실제로 덮어쓰려는 순간,
/// 즉 [`Settings::save`] 안에서 일어난다. 로드 시점에 옮기면 부팅 중 같은 파일을 여러 번
/// 읽는 프로세스들 사이에서 첫 로드만 사건을 보고 나머지는 "부재" 로 관측하게 되어,
/// 정작 사용자에게 알릴 프로세스가 아무것도 모르는 상태가 된다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SettingsOrigin {
    /// 파일을 정상적으로 읽었거나(부재 포함), 프로그램이 만든 값. 저장해도 잃을 것이 없다.
    #[default]
    Clean,
    /// 원본이 그 자리에 있는데 해석하지 못해 기본값으로 폴백했다. 저장 직전에 원본을
    /// `.bak` 으로 옮기고, 옮기지 못하면 저장을 거부한다.
    Unparsable,
    /// 원본을 **읽지도** 못했다(권한 · IO). 내용을 모르므로 옮기지도 않고, 저장만 막는다.
    ProtectedUnreadable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct Settings {
    pub general: GeneralSettings,
    pub appearance: AppearanceSettings,
    pub notification: NotificationSettings,
    pub keybindings: KeybindingSettings,
    pub performance: PerformanceSettings,
    pub memory: MemorySettings,
    pub accessibility: AccessibilitySettings,
    /// 오버레이류(토스트 등) 표시 설정. `#[serde(default)]`(Settings 전체) 로 기존
    /// config.toml 마이그레이션 안전(누락 시 toast_duration_ms=2000).
    pub overlay: OverlaySettings,
    /// Modifier 키 홀드 안내 오버레이의 표시 토글 + 위치·크기 영속 슬롯.
    /// `#[serde(default)]` 로 기존 config.toml 마이그레이션 안전(누락 시 enabled=true, pos/size=None).
    pub modifier_hint: ModifierHintSettings,
    /// 원격 전송(06 bulk 파일 채널) 수신측 저장 폴더 + 용량 상한(07).
    /// `#[serde(default)]` 로 기존 config.toml 마이그레이션 안전(누락 시 dir="", max_mb=500).
    pub remote_transfer: RemoteTransferSettings,
    /// Plugin-contributed settings page 의 generic 값 저장소.
    /// `plugin_settings[plugin_id][storage_key]` = `PluginSettingValue`.
    /// FontOverride(`appearance.plugin_font_overrides`)와 별개 네임스페이스.
    /// 키 부재 시 host 렌더러가 manifest item 의 default 로 fallback 한다.
    /// `#[serde(default)]` 로 기존 config.toml 마이그레이션 안전(누락 시 빈 맵).
    pub plugin_settings:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, PluginSettingValue>>,
    /// 사용자 등록 Lua 스크립트 목록 (ADR-0031). 단축키 트리거·관리 창·TOFU 게이트의 기반.
    /// `#[serde(default)]` 로 기존 config.toml 마이그레이션 안전(누락 시 빈 목록).
    pub scripts: ScriptRegistry,
    /// 이 값의 출처 — 원본 파일을 덮어써도 되는지. 디스크에 나가지 않는 런타임 상태라
    /// `#[serde(skip)]`. clone 을 타고 전파되므로 어느 복사본으로 저장하든 판정이 같다.
    #[serde(skip)]
    pub origin: SettingsOrigin,
}

impl Settings {
    /// 전역 `Theme` 을 만들 때 실어야 하는 설정 값들을 한 덩이로 낸다.
    ///
    /// **이 함수가 이 값들이 채워지는 유일한 자리다.** 종전에는 install 호출부마다
    /// 값을 하나씩 인자로 넘겼고, 그 형태가 실제로 두 번 사고를 냈다 — `ui_zoom` 을
    /// 빠뜨린 install 이 전역 Theme 을 배율 1.0 으로 되돌렸고, `reduced_motion` 은
    /// 위젯 인자로만 존재해 넘기는 자리가 레포 전체에 하나도 없었다(설정을 켜도
    /// 스피너가 계속 돌았다). 값을 늘릴 때 호출부를 안 건드려도 되게 하려고 묶는다.
    /// 결정과 대안은 `docs/adr/0174-theme-carries-reduced-motion.md`.
    pub fn theme_runtime(&self) -> tasty_themes::ThemeRuntime {
        tasty_themes::ThemeRuntime {
            ui_zoom: self.appearance.ui_scale_factor(),
            reduced_motion: self.accessibility.reduced_motion,
        }
    }

    /// Plugin settings 슬롯 조회. 부재 시 `None` — 호출자가 manifest default 로 fallback.
    pub fn plugin_setting(
        &self,
        plugin_id: &str,
        storage_key: &str,
    ) -> Option<&PluginSettingValue> {
        self.plugin_settings
            .get(plugin_id)
            .and_then(|m| m.get(storage_key))
    }

    /// Plugin settings 슬롯 write. 영속(save)은 기존 Settings 흐름이 담당.
    pub fn set_plugin_setting(
        &mut self,
        plugin_id: &str,
        storage_key: &str,
        value: PluginSettingValue,
    ) {
        self.plugin_settings
            .entry(plugin_id.to_string())
            .or_default()
            .insert(storage_key.to_string(), value);
    }
}

// ---- Settings file operations ----

impl Settings {
    /// Returns the config file path: ~/.tasty/config.toml
    pub fn config_path() -> Option<PathBuf> {
        tasty_home().map(|dir| dir.join("config.toml"))
    }

    /// Ensure the config directory exists.
    pub fn ensure_config_dir() -> Result<()> {
        if let Some(path) = Self::config_path()
            && let Some(parent) = path.parent()
        {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    /// Load settings from the config file. Falls back to defaults if not found or invalid.
    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            tracing::info!("no config path available, using defaults");
            return Self::default();
        };
        Self::load_from_path(&path)
    }

    /// 확정된 config 경로에서 읽어 파싱한다.
    ///
    /// **부재와 읽기 실패를 구분한다.** 둘을 같은 기본값으로 뭉개면 권한 오류·IO 오류가
    /// "설정을 만든 적 없음" 과 같아지고, 이후 저장이 멀쩡한 사용자 파일을 기본값으로
    /// 덮어쓴다. 읽지 못한 경우에는 파일을 건드리지 않고 저장만 막는다 — 내용을 확인하지
    /// 못한 파일을 옮기면 일시적 오류에도 사용자 파일이 자리를 뜨기 때문이다.
    fn load_from_path(path: &std::path::Path) -> Self {
        let contents = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!("no settings file at {}, using defaults", path.display());
                return Self::default();
            }
            Err(e) => {
                tracing::error!(
                    "failed to read settings file {}: {e} — using defaults and refusing to \
                     overwrite it (fix permissions or move the file to start fresh)",
                    path.display()
                );
                return Self {
                    origin: SettingsOrigin::ProtectedUnreadable,
                    ..Self::default()
                };
            }
        };
        Self::parse_or_default(&contents, path)
    }

    /// 파싱 성공/실패를 각각 로그하고, 실패 시 기본값으로 폴백한다.
    ///
    /// **파일은 건드리지 않는다.** 원본 보존은 실제로 덮어쓰려는 순간([`Settings::save`])에
    /// 한다. 레벨이 `error!` 인 이유는 `docs/dev-guide/error-handling.md` 의 표가 "설정
    /// 저장 실패" 를 그 레벨의 예로 들기 때문이다 — 파싱 실패는 사용자의 전체 설정이
    /// 무효가 되는 같은 무게의 사건이다.
    fn parse_or_default(contents: &str, path: &std::path::Path) -> Self {
        match Self::parse_with_migration(contents) {
            Ok(settings) => {
                tracing::info!("loaded settings from {}", path.display());
                settings
            }
            Err(e) => {
                tracing::error!(
                    "failed to parse settings file {}: {e} — using defaults; the file is left \
                     as it is and will be moved aside to a .bak before anything overwrites it",
                    path.display()
                );
                Self {
                    origin: SettingsOrigin::Unparsable,
                    ..Self::default()
                }
            }
        }
    }

    /// TOML 파싱 + keybinding 충돌 제거 + 레거시 font override 마이그레이션. keybinding
    /// 충돌 판정에 "원본 파일에 실제로 있던 키" 목록이 필요해, TOML 을 `toml::Value` 로
    /// 먼저 훑어 `[keybindings]` 테이블의 키 이름을 뽑아둔다.
    fn parse_with_migration(contents: &str) -> std::result::Result<Self, toml::de::Error> {
        let existing_kb_keys: HashSet<String> = toml::from_str::<toml::Value>(contents)
            .ok()
            .and_then(|v| v.get("keybindings").and_then(|kb| kb.as_table().cloned()))
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default();

        let mut settings = toml::from_str::<Settings>(contents)?;
        settings.appearance.migrate_legacy_font_overrides();
        settings
            .keybindings
            .remove_conflicts_from_defaults(&existing_kb_keys);
        Ok(settings)
    }

    /// 그 경로의 파일이 **지금도** 해석되지 않는가. 보존이 실제로 일어났는지 되묻는 데
    /// 쓴다 — 원본을 `.bak` 으로 옮겼으면 그 자리는 새로 쓴 정상 파일이라 `false` 이고,
    /// 옮기지 못했으면(백업 자리 소진 등) 원본이 그대로라 `true` 다. 파일이 없으면
    /// 해석할 것이 없으므로 `false`.
    pub fn file_is_unparsable(path: &std::path::Path) -> bool {
        match fs::read_to_string(path) {
            Ok(contents) => Self::parse_with_migration(&contents).is_err(),
            Err(_) => false,
        }
    }

    /// Save settings to the config file.
    ///
    /// 원본을 읽지 못했고 보존도 못 한 상태([`SettingsOrigin::ProtectedUnreadable`])면
    /// **거부한다.** 그 상태의 `self` 는 기본값이므로, 쓰면 디스크에 남아 있는 사용자
    /// 설정을 기본값으로 대체하게 된다.
    pub fn save(&self) -> Result<()> {
        Self::ensure_config_dir()?;
        let Some(path) = Self::config_path() else {
            anyhow::bail!("could not determine config path");
        };
        self.save_to_path(&path)
    }

    /// 확정된 경로에 쓴다. 덮어쓰기 전에 [`Self::protect_existing_file`] 로 기존 파일을
    /// 지킨다.
    fn save_to_path(&self, path: &std::path::Path) -> Result<()> {
        self.protect_existing_file(path)?;
        let contents = toml::to_string_pretty(self)?;
        fs::write(path, contents)?;
        tracing::info!("saved settings to {}", path.display());
        Ok(())
    }

    /// 덮어쓰기 직전에 디스크의 기존 파일을 지킨다.
    ///
    /// 이 프로세스가 로드할 때 해석하지 못했던 파일이 **아직 그 자리에 있으면** `.bak` 으로
    /// 옮긴 뒤 진행한다. 읽지도 못했던 경우에는 옮길 수 없으므로 저장 자체를 거부한다 —
    /// 지금 `self` 는 기본값이라, 쓰는 순간 사용자의 설정이 기본값으로 대체된다.
    ///
    /// 저장 시점에 다시 확인하는 이유: `save` 는 `&self` 라 한 번 보존했다는 사실을
    /// 남길 곳이 없다. 파일을 다시 읽어 판정하면 두 번째 저장이 방금 쓴 정상 파일을
    /// 백업으로 옮기는 일이 없다.
    fn protect_existing_file(&self, path: &std::path::Path) -> Result<()> {
        match self.origin {
            SettingsOrigin::Clean => return Ok(()),
            SettingsOrigin::ProtectedUnreadable => anyhow::bail!(
                "refusing to overwrite the settings file {}: it could not be read (see the \
                 earlier error); fix permissions or move it aside first",
                path.display()
            ),
            SettingsOrigin::Unparsable => {}
        }
        let still_unparsable = match fs::read_to_string(path) {
            Ok(contents) => Self::parse_with_migration(&contents).is_err(),
            // 이미 사라졌다면 지킬 것이 없다.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => anyhow::bail!(
                "refusing to overwrite the settings file {}: it could not be re-read before \
                 being replaced ({e})",
                path.display()
            ),
        };
        if !still_unparsable {
            return Ok(());
        }
        match tasty_utils::path::preserve_corrupt_file(path) {
            Ok(Some(backup)) => {
                tracing::error!(
                    "the settings file {} could not be parsed at startup; it was moved to {} \
                     before being replaced",
                    path.display(),
                    backup.display()
                );
                Ok(())
            }
            // 다른 곳에서 이미 옮겼다 — 원본은 그 백업에 있고 이 자리는 비었다.
            Ok(None) => Ok(()),
            Err(e) => anyhow::bail!(
                "refusing to overwrite the settings file {}: the unparsable original could not \
                 be preserved ({e}); move it aside first",
                path.display()
            ),
        }
    }

    /// Validate enum-like string fields and replace invalid values with safe defaults.
    /// Returns a report so the caller can decide whether to save() and whether to
    /// surface a popup (theme only). Non-theme fields are fixed silently with a warn log.
    ///
    /// `general.language` is intentionally NOT normalized — users may install a
    /// language pack at `~/.tasty/lang/{code}/pack.toml` (a single `{code}.toml` only
    /// overrides a built-in language), so any code is potentially valid. Shape and
    /// discovery rules: `docs/features/language-packs/index.md`.
    pub fn normalize(&mut self) -> NormalizeReport {
        let mut report = NormalizeReport::default();

        // appearance.theme — legacy id 매핑.
        // 실제 valid 검증/fallback 은 부팅 흐름의 `tasty_themes::apply_theme()` 가 담당.
        // (settings 가 어떤 id 가 valid 인지 직접 알 필요 없음.)
        match self.appearance.theme.as_str() {
            "catppuccin-mocha" => {
                self.appearance.theme = "mocha".to_string();
                tracing::info!("migrated legacy theme id catppuccin-mocha → mocha");
                report.changed = true;
            }
            "catppuccin-latte" => {
                self.appearance.theme = "latte".to_string();
                tracing::info!("migrated legacy theme id catppuccin-latte → latte");
                report.changed = true;
            }
            _ => {}
        }

        // appearance.ui_scale
        // 목록을 여기 다시 쓰지 않는다 — 배율 집합의 모수는 `UI_SCALE_CHOICES` 하나이고
        // 그 집합은 `the_supported_ui_scale_set_is_pinned` 가 못박는다.
        normalize_choice(
            &mut self.appearance.ui_scale,
            UI_SCALE_CHOICES,
            "medium",
            "ui_scale",
            &mut report.changed,
        );

        // appearance.default_font.font_scale_mode
        normalize_choice(
            &mut self.appearance.default_font.font_scale_mode,
            &["auto", "fixed"],
            "auto",
            "default_font.font_scale_mode",
            &mut report.changed,
        );

        // appearance.{terminal,markdown,explorer}_font.font_scale_mode (Option<String>)
        // markdown_font / explorer_font are legacy fields kept for one-shot
        // migration; validating them here means a normalize() call made *before*
        // migration still catches bad values.
        for (label, opt) in [
            (
                "terminal_font.font_scale_mode",
                &mut self.appearance.terminal_font.font_scale_mode,
            ),
            (
                "markdown_font.font_scale_mode",
                &mut self.appearance.markdown_font.font_scale_mode,
            ),
            (
                "explorer_font.font_scale_mode",
                &mut self.appearance.explorer_font.font_scale_mode,
            ),
        ] {
            normalize_optional_font_scale_mode(opt, label, &mut report.changed);
        }

        // plugin_font_overrides.<kind>.font_scale_mode (Option<String>)
        for (kind, ov) in self.appearance.plugin_font_overrides.iter_mut() {
            let label = format!("plugin_font_overrides.{kind}.font_scale_mode");
            normalize_optional_font_scale_mode(
                &mut ov.font_scale_mode,
                &label,
                &mut report.changed,
            );
        }

        // general.shell_mode — Windows 전용 필드. 기존 settings.toml 의
        // `shell_mode = "custom"` 같은 값은 비-Windows 에서 serde(default) 가
        // unknown field 로 무시하고, Windows 에서는 여기서 default 로 fallback.
        #[cfg(windows)]
        normalize_choice(
            &mut self.general.shell_mode,
            &["default", "tasty"],
            "default",
            "shell_mode",
            &mut report.changed,
        );

        // general.close_behavior
        normalize_choice(
            &mut self.general.close_behavior,
            &["ask", "minimize", "quit"],
            "ask",
            "close_behavior",
            &mut report.changed,
        );

        // general.link_click_modifier
        normalize_choice(
            &mut self.general.link_click_modifier,
            &["ctrl", "alt", "none"],
            "ctrl",
            "link_click_modifier",
            &mut report.changed,
        );

        // general.explorer_view_mode
        normalize_choice(
            &mut self.general.explorer_view_mode,
            &["grid", "list", "detail"],
            "detail",
            "explorer_view_mode",
            &mut report.changed,
        );

        // general.alt_display_style / option_display_style / shift_display_style
        normalize_choice(
            &mut self.general.alt_display_style,
            &["alt", "cmd", "symbol"],
            "alt",
            "alt_display_style",
            &mut report.changed,
        );
        normalize_choice(
            &mut self.general.option_display_style,
            &["option", "symbol"],
            "option",
            "option_display_style",
            &mut report.changed,
        );
        normalize_choice(
            &mut self.general.shift_display_style,
            &["shift", "symbol"],
            "shift",
            "shift_display_style",
            &mut report.changed,
        );

        report
    }
}

/// Result of [`Settings::normalize`].
#[derive(Debug, Default)]
pub struct NormalizeReport {
    /// Original (invalid) theme name if it was replaced. Caller may surface a popup.
    pub invalid_theme_name: Option<String>,
    /// True if any field was changed by normalize. Caller should `save()` to persist.
    pub changed: bool,
}

fn normalize_choice(
    field: &mut String,
    allowed: &[&str],
    fallback: &str,
    label: &str,
    changed: &mut bool,
) {
    if !allowed.contains(&field.as_str()) {
        let invalid = std::mem::take(field);
        *field = fallback.to_string();
        tracing::warn!("invalid {label} \"{invalid}\" → {fallback}");
        *changed = true;
    }
}

/// `font_scale_mode` 류 `Option<String>` 필드가 `Some(x)` 인데 x 가 허용 값
/// (`auto`/`fixed`) 이 아니면 unset(`None`) 한다. `None` 은 그대로 유효(미지정).
fn normalize_optional_font_scale_mode(opt: &mut Option<String>, label: &str, changed: &mut bool) {
    let Some(mode) = opt.as_deref() else {
        return;
    };
    if matches!(mode, "auto" | "fixed") {
        return;
    }
    let invalid = opt.take().unwrap_or_default();
    tracing::warn!("invalid {label} \"{invalid}\" → unset");
    *changed = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_valid() {
        let settings = Settings::default();
        assert!(!settings.general.shell.is_empty());
        assert!(settings.appearance.default_font.font_size > 0.0);
        assert!(settings.appearance.sidebar_width > tasty_type_geometry::length::LogicalPx(0.0));
    }

    #[test]
    fn settings_serialization_roundtrip() {
        let settings = Settings::default();
        let toml_str = toml::to_string_pretty(&settings).unwrap();
        let parsed: Settings = toml::from_str(&toml_str).unwrap();
        assert_eq!(
            parsed.appearance.default_font.font_size,
            settings.appearance.default_font.font_size
        );
        assert_eq!(parsed.general.shell, settings.general.shell);
        assert_eq!(
            parsed.notification.coalesce_ms,
            settings.notification.coalesce_ms
        );
    }

    #[test]
    fn settings_partial_toml_uses_defaults() {
        let partial = r#"
[appearance]
ui_scale = "large"
"#;
        let parsed: Settings = toml::from_str(partial).unwrap();
        assert_eq!(parsed.appearance.ui_scale, "large");
        // Other fields fall back to defaults.
        assert_eq!(parsed.appearance.default_font.font_size, 14.0);
        assert!(parsed.notification.enabled);
        assert!(!parsed.general.shell.is_empty());
    }

    /// 해석하지 못한 파일은 **로드가 건드리지 않는다.** 부팅 중 같은 파일을 여러 번 읽고,
    /// 런처와 GUI 는 서로 다른 프로세스다 — 첫 로드가 파일을 옮겨버리면 정작 사용자에게
    /// 알릴 프로세스는 "파일 없음" 만 보게 된다.
    #[test]
    fn unparsable_settings_file_is_left_in_place_by_load() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "this is not = valid toml [[[").unwrap();

        for _ in 0..2 {
            let loaded = Settings::load_from_path(&path);
            assert_eq!(
                loaded.origin,
                SettingsOrigin::Unparsable,
                "몇 번을 읽어도 같은 사실을 봐야 한다"
            );
        }
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "this is not = valid toml [[[",
            "로드는 원본을 옮기지 않는다"
        );
        assert!(!tmp.path().join("config.toml.bak").exists());
    }

    /// 보존은 실제로 덮어쓰는 순간에 일어난다. 저장 뒤 원본은 `.bak` 에, 새 값은 원래
    /// 자리에 있어야 한다.
    #[test]
    fn saving_over_an_unparsable_file_preserves_it_first() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "this is not = valid toml [[[").unwrap();

        let loaded = Settings::load_from_path(&path);
        loaded.save_to_path(&path).unwrap();

        let backup = tmp.path().join("config.toml.bak");
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            "this is not = valid toml [[[",
            "원본이 백업에 그대로 남아야 한다"
        );
        assert!(
            Settings::load_from_path(&path).origin == SettingsOrigin::Clean,
            "새로 쓴 파일은 정상이어야 한다"
        );
        // 두 번째 저장은 방금 쓴 정상 파일을 백업으로 옮기지 않는다.
        loaded.save_to_path(&path).unwrap();
        assert!(!tmp.path().join("config.toml.bak.2").exists());
    }

    /// 파일이 없는 것은 정상이다 — 첫 실행. 저장을 막을 이유가 없다.
    #[test]
    fn missing_settings_file_is_clean() {
        let tmp = tempfile::tempdir().unwrap();
        let loaded = Settings::load_from_path(&tmp.path().join("config.toml"));
        assert_eq!(loaded.origin, SettingsOrigin::Clean);
    }

    /// 읽기 자체가 실패하면 파일을 **건드리지 않고** 저장만 막는다. 일시적 권한 오류에
    /// 사용자 설정이 자리를 뜨면 안 되고, 그 위에 기본값을 쓰면 더더욱 안 된다.
    #[cfg(unix)]
    #[test]
    fn unreadable_settings_file_blocks_save_and_is_left_in_place() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "[general]\nshell = \"/bin/zsh\"\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

        let loaded = Settings::load_from_path(&path);
        assert_eq!(loaded.origin, SettingsOrigin::ProtectedUnreadable);
        assert!(
            loaded.save_to_path(&path).is_err(),
            "읽지 못한 파일 위에 기본값을 쓰면 안 된다"
        );

        // 위 단정만으로는 부족하다 — mode 000 파일에는 `fs::write` 자체가 실패하므로
        // 가드를 통째로 들어내도 그대로 통과한다(OS 권한을 검사하는 셈이다). 거부가
        // **origin 판정에서** 나온다는 것을 보이려면 OS 가 막지 않는 자리에 써 봐야 한다.
        let writable = tmp.path().join("elsewhere.toml");
        assert!(
            loaded.save_to_path(&writable).is_err(),
            "쓸 수 있는 자리라도 거부해야 한다 — 거부의 근거는 파일 권한이 아니라 \
             '원본을 읽지 못했다' 는 판정이다"
        );
        assert!(!writable.exists(), "거부했으면 파일을 만들지도 않는다");
        // 대조군: 같은 자리라도 origin 이 Clean 이면 정상적으로 쓰인다 — 위 두 단정이
        // "save_to_path 는 늘 실패한다" 로 통과하는 것을 막는다.
        Settings::default()
            .save_to_path(&writable)
            .expect("Clean 인 설정은 쓸 수 있어야 한다");
        assert!(writable.exists());

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[general]\nshell = \"/bin/zsh\"\n",
            "원본이 그대로 있어야 한다"
        );
        assert!(
            !tmp.path().join("config.toml.bak").exists(),
            "백업도 만들지 않는다"
        );
    }

    /// 정상 로드는 저장을 막지 않는다(회귀 가드 — 가드가 과하게 잠그면 설정 저장이 죽는다).
    #[test]
    fn valid_settings_file_stays_clean() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "[general]\nshell = \"/bin/zsh\"\n").unwrap();
        let loaded = Settings::load_from_path(&path);
        assert_eq!(loaded.origin, SettingsOrigin::Clean);
        assert_eq!(loaded.general.shell, "/bin/zsh");
    }

    #[test]
    fn settings_empty_toml_uses_all_defaults() {
        let parsed: Settings = toml::from_str("").unwrap();
        let defaults = Settings::default();
        assert_eq!(
            parsed.appearance.default_font.font_size,
            defaults.appearance.default_font.font_size
        );
        assert_eq!(
            parsed.notification.coalesce_ms,
            defaults.notification.coalesce_ms
        );
    }

    #[test]
    fn settings_font_family_default() {
        let settings = Settings::default();
        assert_eq!(settings.appearance.default_font.font_family, "");
    }

    #[test]
    fn settings_theme_default() {
        let settings = Settings::default();
        assert_eq!(settings.appearance.theme, "mocha");
    }

    #[test]
    fn settings_background_opacity_default() {
        let settings = Settings::default();
        assert_eq!(settings.appearance.background_opacity, 1.0);
    }

    #[test]
    fn settings_custom_appearance_roundtrip() {
        let mut settings = Settings::default();
        settings.appearance.default_font.font_family = "Fira Code".to_string();
        settings.appearance.default_font.font_size = 18.0;
        settings.appearance.terminal_font.font_size = Some(20.0);
        settings.appearance.theme = "light".to_string();
        settings.appearance.background_opacity = 0.8;
        let toml_str = toml::to_string_pretty(&settings).unwrap();
        let parsed: Settings = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.appearance.default_font.font_family, "Fira Code");
        assert_eq!(parsed.appearance.default_font.font_size, 18.0);
        assert_eq!(parsed.appearance.terminal_font.font_size, Some(20.0));
        assert_eq!(parsed.appearance.theme, "light");
        assert_eq!(parsed.appearance.background_opacity, 0.8);
    }

    #[test]
    fn settings_keybindings_default() {
        let settings = Settings::default();
        assert_eq!(
            settings.keybindings.get_field("new_workspace"),
            Some("alt+n")
        );
        assert_eq!(settings.keybindings.get_field("new_tab"), Some("alt+t"));
    }

    #[test]
    fn normalize_valid_settings_is_noop() {
        let mut settings = Settings::default();
        let report = settings.normalize();
        assert!(!report.changed);
        assert!(report.invalid_theme_name.is_none());
    }

    #[test]
    fn normalize_migrates_legacy_theme_ids() {
        for (legacy, expected) in [("catppuccin-mocha", "mocha"), ("catppuccin-latte", "latte")] {
            let mut settings = Settings::default();
            settings.appearance.theme = legacy.to_string();
            let report = settings.normalize();
            assert!(report.changed, "{legacy} should be migrated");
            assert_eq!(settings.appearance.theme, expected);
        }
    }

    #[test]
    fn normalize_does_not_touch_unknown_theme_ids() {
        // valid 검증은 부팅 흐름의 apply_theme 에서 처리. settings 는 단순 매핑만 한다.
        let mut settings = Settings::default();
        settings.appearance.theme = "my-custom-theme".to_string();
        let report = settings.normalize();
        // 마이그레이션 대상이 아니라 변경 없음 (다른 enum 필드들도 valid 였다고 가정).
        assert!(!report.changed);
        assert_eq!(settings.appearance.theme, "my-custom-theme");
    }

    #[test]
    fn normalize_current_ids_pass_through() {
        for theme in ["mocha", "latte"] {
            let mut settings = Settings::default();
            settings.appearance.theme = theme.to_string();
            let report = settings.normalize();
            assert!(!report.changed, "theme {theme} should be accepted as-is");
            assert_eq!(settings.appearance.theme, theme);
        }
    }

    #[test]
    fn normalize_silent_enum_fields_get_fixed() {
        let mut settings = Settings::default();
        settings.appearance.ui_scale = "huge".to_string();
        settings.appearance.default_font.font_scale_mode = "bogus".to_string();
        settings.appearance.terminal_font.font_scale_mode = Some("nope".to_string());
        #[cfg(windows)]
        {
            settings.general.shell_mode = "weird".to_string();
        }
        settings.general.close_behavior = "explode".to_string();
        settings.general.link_click_modifier = "meta".to_string();
        settings.general.alt_display_style = "bogus".to_string();
        settings.general.option_display_style = "bogus".to_string();
        settings.general.shift_display_style = "bogus".to_string();

        let report = settings.normalize();
        assert!(report.changed);
        assert!(report.invalid_theme_name.is_none());
        assert_eq!(settings.appearance.ui_scale, "medium");
        assert_eq!(settings.appearance.default_font.font_scale_mode, "auto");
        assert!(settings.appearance.terminal_font.font_scale_mode.is_none());
        #[cfg(windows)]
        assert_eq!(settings.general.shell_mode, "default");
        assert_eq!(settings.general.close_behavior, "ask");
        assert_eq!(settings.general.link_click_modifier, "ctrl");
        assert_eq!(settings.general.alt_display_style, "alt");
        assert_eq!(settings.general.option_display_style, "option");
        assert_eq!(settings.general.shift_display_style, "shift");
    }

    /// 기존 settings.toml 에 `shell_mode = "custom"` 이 남아 있을 때 normalize 가
    /// panic 없이 default 로 fallback 하는지 검증 (Windows 전용 필드).
    #[cfg(windows)]
    #[test]
    fn custom_mode_no_longer_exists() {
        let mut settings = Settings::default();
        settings.general.shell_mode = "custom".to_string();
        let report = settings.normalize();
        assert!(report.changed);
        assert_eq!(settings.general.shell_mode, "default");
    }

    #[test]
    fn normalize_does_not_touch_language() {
        let mut settings = Settings::default();
        settings.general.language = "fr".to_string();
        let report = settings.normalize();
        assert!(!report.changed);
        assert_eq!(settings.general.language, "fr");
    }

    #[test]
    fn default_targeted_pty_polling_is_on() {
        assert!(PerformanceSettings::default().targeted_pty_polling);
    }

    #[test]
    fn performance_missing_key_uses_new_default() {
        let parsed: Settings = toml::from_str("[performance]").unwrap();
        assert!(parsed.performance.targeted_pty_polling);
        let parsed: Settings = toml::from_str("").unwrap();
        assert!(parsed.performance.targeted_pty_polling);
    }

    #[test]
    fn performance_explicit_false_preserved() {
        let parsed: Settings =
            toml::from_str("[performance]\ntargeted_pty_polling = false").unwrap();
        assert!(!parsed.performance.targeted_pty_polling);
    }

    #[test]
    fn modifier_hint_default_enabled() {
        let settings = Settings::default();
        assert!(settings.modifier_hint.enabled);
        assert!(settings.modifier_hint.pos.is_none());
        assert!(settings.modifier_hint.size.is_none());
    }

    #[test]
    fn modifier_hint_missing_key_uses_defaults() {
        // 신규 키가 없는 구버전 config.toml 마이그레이션: enabled=true, pos/size=None.
        let parsed: Settings = toml::from_str("").unwrap();
        assert!(parsed.modifier_hint.enabled);
        assert!(parsed.modifier_hint.pos.is_none());
        assert!(parsed.modifier_hint.size.is_none());
        // 섹션은 있으나 키가 비어도 동일.
        let parsed: Settings = toml::from_str("[modifier_hint]").unwrap();
        assert!(parsed.modifier_hint.enabled);
    }

    #[test]
    fn remote_transfer_missing_key_uses_defaults() {
        // 신규 키가 없는 구버전 config.toml 마이그레이션: dir="", max_mb=500.
        let parsed: Settings = toml::from_str("").unwrap();
        assert_eq!(parsed.remote_transfer.dir, "");
        assert_eq!(parsed.remote_transfer.max_mb, 500);
        // 섹션은 있으나 키가 비어도 동일.
        let parsed: Settings = toml::from_str("[remote_transfer]").unwrap();
        assert_eq!(parsed.remote_transfer.dir, "");
        assert_eq!(parsed.remote_transfer.max_mb, 500);
    }

    #[test]
    fn remote_transfer_roundtrip() {
        let mut settings = Settings::default();
        settings.remote_transfer.dir = "/tmp/xfer".to_string();
        settings.remote_transfer.max_mb = 42;
        let toml_str = toml::to_string_pretty(&settings).unwrap();
        let parsed: Settings = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.remote_transfer.dir, "/tmp/xfer");
        assert_eq!(parsed.remote_transfer.max_mb, 42);
    }

    #[test]
    fn modifier_hint_enabled_false_preserved() {
        let parsed: Settings = toml::from_str("[modifier_hint]\nenabled = false").unwrap();
        assert!(!parsed.modifier_hint.enabled);
    }

    /// `theme_runtime()` 이 이 값들이 채워지는 유일한 자리다 — 여기서 빠지면 전역
    /// Theme 을 설치하는 4 경로 전부가 조용히 기본값을 쓴다.
    #[test]
    fn theme_runtime_carries_every_settings_backed_value() {
        let mut settings = Settings::default();
        settings.accessibility.reduced_motion = true;
        let rt = settings.theme_runtime();
        assert!(rt.reduced_motion);
        assert_eq!(rt.ui_zoom, settings.appearance.ui_scale_factor());

        let off = Settings::default().theme_runtime();
        assert!(!off.reduced_motion);
    }

    #[test]
    fn modifier_hint_geometry_roundtrip() {
        use tasty_type_geometry::length::LogicalPx;
        let mut settings = Settings::default();
        settings.modifier_hint.enabled = false;
        settings.modifier_hint.pos = Some((LogicalPx(120.0), LogicalPx(340.0)));
        settings.modifier_hint.size = Some((LogicalPx(280.0), LogicalPx(160.0)));
        let toml_str = toml::to_string_pretty(&settings).unwrap();
        let parsed: Settings = toml::from_str(&toml_str).unwrap();
        assert!(!parsed.modifier_hint.enabled);
        assert_eq!(
            parsed.modifier_hint.pos,
            Some((LogicalPx(120.0), LogicalPx(340.0)))
        );
        assert_eq!(
            parsed.modifier_hint.size,
            Some((LogicalPx(280.0), LogicalPx(160.0)))
        );
    }
}
