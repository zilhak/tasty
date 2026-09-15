//! Plugin 자체 i18n 헬퍼.
//!
//! Plugin process는 host의 i18n catalog에 접근할 수 없으므로 자기 lang/ 디렉터리에서
//! host와 같은 공용 로더로 설치본 및 사용자 번역을 평면 key→value 맵으로 읽는다.
//!
//! 동작은 host 의 `i18n::Translations` (본 바이너리 `src/i18n.rs`) 와 동일하게 — base = `en.toml`,
//! 활성 locale 파일 → 사용자 override 순서로 overlay. 키 미스 시 키 자체 반환.
//!
//! ```ignore
//! use tasty_plugin_sdk::{PluginEnv, i18n::Translator};
//!
//! let env = PluginEnv::load()?;
//! let tr = Translator::from_plugin_env(&env);
//! let label = tr.t("git_viewer.refresh");
//! ```

use std::collections::HashMap;
use std::path::Path;

use crate::env::PluginEnv;

#[derive(Debug, Default)]
pub struct Translator {
    strings: HashMap<String, String>,
    /// 활성 locale code — 디버그용으로 보관.
    pub locale: String,
}

impl Translator {
    /// `lang_dir`에서 `en.toml`을 base로 로드한 뒤, `locale != "en"`이면
    /// `<locale>.toml`을 덮어쓴다. 파일이 없으면 조용히 무시 (키 자체 반환).
    pub fn load(lang_dir: &Path, locale: &str) -> Self {
        Self::load_with_overrides(lang_dir, locale, "", None)
    }

    /// Load the same catalog as the host, with an explicit parent language root.
    pub fn load_with_overrides(
        lang_dir: &Path,
        locale: &str,
        plugin_id: &str,
        user_lang_dir: Option<&Path>,
    ) -> Self {
        Self {
            strings: tasty_i18n::plugin_catalog::load(lang_dir, locale, plugin_id, user_lang_dir),
            locale: locale.to_string(),
        }
    }

    /// `env.plugin_dir`이 있으면 그 아래 `lang/`을 자동으로 로드한다.
    /// 디렉터리가 없거나 plugin_dir이 미주입이면 빈 카탈로그.
    pub fn from_plugin_env(env: &PluginEnv) -> Self {
        let Some(dir) = env.plugin_dir.as_ref() else {
            return Self {
                strings: HashMap::new(),
                locale: env.locale.clone(),
            };
        };
        // TASTY_HOME belongs to this process, not necessarily the host. Only
        // the explicit parent root may supply user overrides to a plugin.
        let user_lang_dir = std::env::var_os("TASTY_PARENT_HOME")
            .filter(|root| !root.is_empty())
            .map(|root| std::path::PathBuf::from(root).join("lang"));
        Self::load_with_overrides(
            &dir.join("lang"),
            &env.locale,
            &env.plugin_id,
            user_lang_dir.as_deref(),
        )
    }

    /// 키 lookup. 미스 시 키 자체 반환.
    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        self.strings.get(key).map(|s| s.as_str()).unwrap_or(key)
    }

    /// 키 lookup + `{}` 첫 occurrence 치환.
    pub fn t_fmt(&self, key: &str, arg: &str) -> String {
        let template = self.t(key);
        template.replacen("{}", arg, 1)
    }

    /// 키 lookup + `{0}` 토큰 치환 (multi-arg 패턴이 필요할 때).
    pub fn t_replace(&self, key: &str, token: &str, value: &str) -> String {
        self.t(key).replace(token, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn loads_base_en_and_overlays_locale() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("en.toml"),
            "[ns]\nhello = \"Hello\"\nbye = \"Bye\"\n",
        )
        .unwrap();
        fs::write(dir.path().join("ko.toml"), "[ns]\nhello = \"안녕\"\n").unwrap();
        let tr = Translator::load(dir.path(), "ko");
        assert_eq!(tr.t("ns.hello"), "안녕"); // ko가 overlay
        assert_eq!(tr.t("ns.bye"), "Bye"); // ko 미정의 → en fallback
        assert_eq!(tr.t("missing"), "missing"); // 키 미스
    }

    #[test]
    fn fmt_replaces_first_placeholder() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("en.toml"), "[e]\nerr = \"Error: {}\"\n").unwrap();
        let tr = Translator::load(dir.path(), "en");
        assert_eq!(tr.t_fmt("e.err", "boom"), "Error: boom");
    }
}
