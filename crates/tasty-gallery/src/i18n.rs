//! 갤러리의 번역 조회. 본체와 같은 `lang/en.toml` 문자열을 읽어 사본을 두지 않는다.
//! 갤러리는 설정을 읽지 않으므로 처음 조회할 때 영어 번역표로 한 번 초기화한다.

/// `key`의 영어 문자열.
pub fn t(key: &'static str) -> &'static str {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let report = tasty_i18n::init("en");
        if report.fell_back() {
            tracing::warn!("gallery i18n init fell back: {report:?}");
        }
    });
    tasty_i18n::t(key)
}
