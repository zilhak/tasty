//! Plugin 자체 i18n 헬퍼.
//!
//! Plugin process는 host의 i18n catalog에 접근할 수 없으므로 자기 lang/ 디렉터리에서
//! 직접 toml을 읽어 평면 key→value 맵을 만든다.
//!
//! 동작은 host 의 `i18n::Translations` (본 바이너리 `src/i18n.rs`) 와 동일하게 — base = `en.toml`,
//! 활성 locale 파일이 있으면 그 위에 overlay. 키 미스 시 키 자체 반환.
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
        let mut strings: HashMap<String, String> = HashMap::new();
        let en_path = lang_dir.join("en.toml");
        if let Ok(s) = std::fs::read_to_string(&en_path) {
            parse_toml_into(&mut strings, &s);
        }
        if locale != "en" {
            let p = lang_dir.join(format!("{locale}.toml"));
            if let Ok(s) = std::fs::read_to_string(&p) {
                parse_toml_into(&mut strings, &s);
            }
        }
        Self {
            strings,
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
        Self::load(&dir.join("lang"), &env.locale)
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

/// 재귀 toml table → 평면 dot-key 평탄화. host의 `Translations::parse_toml_into`와 동일.
fn parse_toml_into(map: &mut HashMap<String, String>, source: &str) {
    if let Ok(value) = source.parse::<toml::Value>()
        && let Some(table) = value.as_table()
    {
        flatten(map, "", table);
    }
}

fn flatten(map: &mut HashMap<String, String>, prefix: &str, table: &toml::value::Table) {
    for (k, v) in table {
        let key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            toml::Value::Table(inner) => flatten(map, &key, inner),
            toml::Value::String(s) => {
                map.insert(key, s.clone());
            }
            _ => {}
        }
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
