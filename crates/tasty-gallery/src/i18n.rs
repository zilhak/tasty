//! 갤러리의 번역 조회. 본체와 같은 `lang/en.toml` 문자열을 읽어 사본을 두지 않는다.
//! 갤러리는 설정을 읽지 않으므로 처음 조회할 때 영어 번역표로 한 번 초기화한다.

fn ensure_init() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let report = tasty_i18n::init("en");
        if report.fell_back() {
            tracing::warn!("gallery i18n init fell back: {report:?}");
        }
    });
}

/// `key`의 영어 문자열.
pub fn t(key: &'static str) -> &'static str {
    ensure_init();
    tasty_i18n::t(key)
}

/// `key`의 영어 문자열에서 첫 `{}` 를 `arg` 로 바꾼다.
pub fn t_fmt(key: &'static str, arg: &str) -> String {
    ensure_init();
    tasty_i18n::t_fmt(key, arg)
}

/// 수 `n` 에 맞는 영어 문자열(`<key>` 또는 단수 변형 `<key>_one`)을 `args` 로 채운다.
pub fn t_count(key: &'static str, n: u64, args: &[&str]) -> String {
    ensure_init();
    tasty_i18n::t_count(key, n, args)
}

/// 수 `n` 에 맞는 영어 템플릿(`<key>` 또는 `<key>_one`)을 채우지 않고 돌려준다.
pub fn t_count_template(key: &'static str, n: u64) -> &'static str {
    ensure_init();
    tasty_i18n::t_count_template(key, n)
}

/// `key`의 영어 문자열에서 앞의 두 `{}` 를 차례로 바꾼다.
pub fn t_fmt2(key: &'static str, arg1: &str, arg2: &str) -> String {
    ensure_init();
    tasty_i18n::t_fmt2(key, arg1, arg2)
}

/// `key`의 영어 문자열에서 `{}` 를 `args` 로 차례로 바꾼다.
pub fn t_args(key: &'static str, args: &[&str]) -> String {
    ensure_init();
    tasty_i18n::t_args(key, args)
}

/// `key`의 `code`(ko·ja) 문자열. 한 화면에 여러 언어의 예제를 나란히 보일 때 쓴다.
/// 본체와 같은 `lang/{code}.toml`을 읽으며 키가 없으면 영어로 돌아간다.
pub fn t_lang(code: &str, key: &'static str) -> &'static str {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static KO: OnceLock<HashMap<String, String>> = OnceLock::new();
    static JA: OnceLock<HashMap<String, String>> = OnceLock::new();
    let (cell, text) = match code {
        "ko" => (&KO, include_str!("../../../lang/ko.toml")),
        "ja" => (&JA, include_str!("../../../lang/ja.toml")),
        _ => return t(key),
    };
    let table = cell.get_or_init(|| {
        let mut map = HashMap::new();
        match toml::from_str::<toml::Value>(text) {
            Ok(value) => tasty_i18n::flatten_catalog_toml("", &value, &mut map),
            Err(e) => tracing::warn!("gallery lang/{code}.toml did not parse: {e}"),
        }
        map
    });
    match table.get(key) {
        Some(value) => value.as_str(),
        None => t(key),
    }
}
