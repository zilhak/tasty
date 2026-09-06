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

/// 재귀 toml table → 평면 dot-key 평탄화.
///
/// **host 의 규칙을 그대로 부른다 — 여기에 사본을 두지 않는다.** 예전에는 같은 재귀가
/// 이 파일에 복사돼 있었고 주석이 "host 의 `Translations::parse_toml_into` 와 동일" 이라고
/// 주장했는데, 둘을 같게 잡아 주는 것이 그 주석뿐이었다. 그 상태에서 무엇이 안 잡히는지
/// 실측했다(2026-09-06): 사본에 정수 leaf 도 키로 세게 하는 변이를 넣어도 이 크레이트와
/// 소비 plugin 의 시험 **443 건이 전부 초록**이었고, 반대로 host 규칙을 통째로 망가뜨려도
/// 이 크레이트의 시험 39 건이 전부 초록이었다. **양방향으로 아무 결합이 없었다.**
///
/// 갈리면 무엇이 깨지는가: plugin 은 자기 `lang/` 을 이 함수로 펴서 `t()` 를 풀고,
/// 카탈로그 정합 가드(`tests/i18n_key_parity.rs`)는 **host 규칙으로** 같은 파일을 편다.
/// 두 규칙이 갈리면 가드가 검사한 키 집합과 plugin 이 실제로 푸는 키 집합이 달라진다 —
/// 그 어긋남은 **가드가 초록인 채로** 생긴다.
///
/// `tasty-i18n` 을 의존에 더해도 새 전이 의존은 없다. 이 크레이트는 그것이 쓰는
/// `toml` · `tracing` · `tasty-utils` 를 이미 전부 갖고 있다.
fn parse_toml_into(map: &mut HashMap<String, String>, source: &str) {
    if let Ok(value) = source.parse::<toml::Value>() {
        tasty_i18n::flatten_catalog_toml("", &value, map);
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
