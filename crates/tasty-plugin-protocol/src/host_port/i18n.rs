//! 플러그인 언어팩을 호스트에 등록하는 trait.
//! 플러그인 관리자가 본체 i18n 구현에 직접 의존하지 않도록 분리한다.

use std::path::Path;

pub trait I18nNamespaceRegistrar: Send + Sync {
    fn register(&self, namespace: &str, lang_dir: &Path);
    fn unregister(&self, namespace: &str);
}
