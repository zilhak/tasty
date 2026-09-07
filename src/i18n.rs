//! Internationalization (i18n) facade for the main binary.
//!
//! The translation store itself now lives in the shared [`tasty_i18n`] crate so
//! that the `tasty-cli` crate can reach the *same* global table (CLI runs
//! in-process inside the `tasty` binary). This module re-exports the public
//! lookup API unchanged — every `crate::i18n::t(...)` call site keeps working —
//! and keeps the bin-specific plugin-registrar adapter, which depends on
//! `tasty_plugin_protocol` and therefore cannot live in the leaf i18n crate.

use std::path::Path;

// 일부 항목(`current_language`, `t_args`)은 본 바이너리에 호출처가 없고 CLI/plugin
// wiring 등 다른 크레이트·미래 호출처를 위해 facade 에 보존한다. 본 바이너리는
// 다운스트림이 없어 미사용 `pub use` 가 unused_imports 로 잡히므로 명시적 allow.
#[allow(unused_imports)]
pub use tasty_i18n::{
    FontDecl, LanguageEntry, LoadOutcome, LoadReport, TOAST_MAX_CHARS, available_languages,
    current_language, init, load_report, register_namespace, t, t_args, t_fmt, t_fmt_fit, t_fmt2,
    unregister_namespace,
};

/// Plugin manager 가 의존하는 trait 의 본 바이너리 impl. boot wiring 에서
/// `Arc::new(BinI18nRegistrar)` 로 주입.
pub struct BinI18nRegistrar;

impl tasty_plugin_protocol::host_port::I18nNamespaceRegistrar for BinI18nRegistrar {
    fn register(&self, namespace: &str, lang_dir: &Path) {
        register_namespace(namespace, lang_dir);
    }
    fn unregister(&self, namespace: &str) {
        unregister_namespace(namespace);
    }
}
