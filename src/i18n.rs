//! 본체와 in-process CLI가 공용 번역 저장소를 사용한다.
//! plugin 프로토콜 의존성이 있는 등록 어댑터만 본체에 둔다.

use std::path::Path;

// 이유: 본체에서 쓰지 않는 항목도 공용 번역 API로 다시 노출한다.
#[allow(unused_imports)]
pub use tasty_i18n::{
    FontDecl, LanguageEntry, LoadOutcome, LoadReport, TOAST_MAX_CHARS, available_languages,
    current_language, init, load_report, register_namespace, t, t_args, t_fmt, t_fmt_fit, t_fmt2,
    unregister_namespace,
};

pub struct BinI18nRegistrar;

impl tasty_plugin_protocol::host_port::I18nNamespaceRegistrar for BinI18nRegistrar {
    fn register(&self, namespace: &str, lang_dir: &Path) {
        register_namespace(namespace, lang_dir);
    }
    fn unregister(&self, namespace: &str) {
        unregister_namespace(namespace);
    }
}
