//! 호스트 i18n 시스템에 plugin namespace 의 lang 디렉터리를 등록하기 위한 trait.
//!
//! plugin manager 는 plugin discovery/enable 시 manifest 의 `lang_dir` 을 받아
//! 호스트 i18n 에 등록해야 한다. manager 가 본 바이너리 `crate::i18n::*` 함수에
//! 직접 결합하지 않도록 좁은 trait 으로 노출한다.

use std::path::Path;

pub trait I18nNamespaceRegistrar: Send + Sync {
    fn register(&self, namespace: &str, lang_dir: &Path);
    fn unregister(&self, namespace: &str);
}
