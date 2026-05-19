//! 부팅 시 1회 settings 로드 + i18n 초기화.

pub(crate) fn init() {
    let lang_settings = crate::settings::Settings::load();
    crate::i18n::init(&lang_settings.general.language);
}
