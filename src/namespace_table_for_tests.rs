//! 시험 바이너리에서 함께 쓰는 플러그인 namespace 표. 프로세스당 한 번만 설치할 수 있다.
//! 각 시험은 서로 다른 플러그인 ID와 prefix를 사용하고 끝나면 자기 ID의 등록을 지운다.

use std::sync::{Arc, OnceLock, RwLock};

use tasty_ipc::ipc_namespace::IpcNamespaceRegistry;

pub(crate) fn installed_test_table() -> &'static Arc<RwLock<IpcNamespaceRegistry>> {
    static TABLE: OnceLock<Arc<RwLock<IpcNamespaceRegistry>>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let table = Arc::new(RwLock::new(IpcNamespaceRegistry::new()));
        assert!(
            tasty_ipc::method_meta::install_namespace_table(Arc::clone(&table)),
            "다른 곳에서 namespace 표를 먼저 설치해 이 시험의 표가 사용되지 않는다"
        );
        table
    })
}
