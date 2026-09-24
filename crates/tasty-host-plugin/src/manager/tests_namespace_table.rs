//! 설치 목록의 변경으로 namespace 소유자가 갱신되는지 확인한다. disable은 소유자를 지우지 않는다.
//! IPC 메서드 조회는 tasty-ipc의 method_meta_tests.rs가 별도로 검사한다.

use tasty_plugin_manifest::{Manifest, PluginPackage};

use super::PluginManager;

const FAKE_MANIFEST: &str = r#"
manifest_version = 1
id = "com.example.namespace_table_test"
name = "Namespace Table Test"
version = "0.0.1"
api_version = "1.0"

[entry]
type = "process"
command = "echo"
args = []

[[contributes.ipc_namespace]]
prefix = "nstest"
"#;

fn empty_waker() -> tasty_terminal::waker_factory::SharedWakerFactory {
    std::sync::Arc::new(tasty_terminal::waker_factory::NoopWakerFactory)
}

fn fake_package() -> PluginPackage {
    let manifest: Manifest = toml::from_str(FAKE_MANIFEST).expect("fake manifest should parse");
    PluginPackage {
        dir: std::path::PathBuf::from("/nonexistent/namespace_table_test"),
        manifest,
    }
}

/// 디스크를 읽지 않고 설치 목록으로 namespace 소유자를 계산한다.
fn manager_with(packages: Vec<PluginPackage>) -> PluginManager {
    let mut mgr = PluginManager::new(empty_waker());
    mgr.set_packages_for_tests(packages);
    mgr
}

#[test]
fn nothing_is_owned_before_anything_is_installed() {
    let mgr = manager_with(Vec::new());
    assert!(
        !mgr.owns_namespace("nstest.invoke"),
        "설치된 플러그인이 없으면 namespace 소유자도 없어야 한다"
    );
}

#[test]
fn a_manifest_prefix_becomes_owned() {
    let mgr = manager_with(vec![fake_package()]);
    assert!(
        mgr.owns_namespace("nstest.invoke"),
        "매니페스트가 선언한 prefix 아래의 이름은 그 plugin 의 것이다"
    );
    assert!(
        mgr.namespace_belongs_to_other("nstest.invoke", "com.example.other"),
        "다른 plugin의 namespace는 전달 대상이어야 한다"
    );
    assert!(
        !mgr.namespace_belongs_to_other("nstest.invoke", "com.example.namespace_table_test"),
        "자기 namespace 를 자기가 부르는 것은 forward 가 아니라 trampoline 이다"
    );
}

/// 설치 목록에서 제거하면 namespace 소유자도 사라진다.
#[test]
fn removing_the_package_takes_the_ownership_back() {
    let mut mgr = manager_with(vec![fake_package()]);
    assert!(mgr.owns_namespace("nstest.invoke"));
    mgr.set_packages_for_tests(Vec::new());
    assert!(
        !mgr.owns_namespace("nstest.invoke"),
        "설치가 사라졌는데 이름이 예약된 채로 남으면, 그 이름의 host 구현에 닿을 방법이 없다"
    );
}

#[test]
fn install_remove_reinstall_is_consistent() {
    let mut mgr = manager_with(vec![fake_package()]);
    assert!(mgr.owns_namespace("nstest.invoke"));
    mgr.set_packages_for_tests(Vec::new());
    assert!(!mgr.owns_namespace("nstest.invoke"));
    mgr.set_packages_for_tests(vec![fake_package()]);
    assert!(
        mgr.owns_namespace("nstest.invoke"),
        "다시 설치하면 namespace 소유자도 다시 등록되어야 한다"
    );
}

/// 설치 목록과 표가 다르면 debug 검사가 실패해야 한다.
#[test]
fn a_fresh_table_passes_the_assertion() {
    let mgr = manager_with(vec![fake_package()]);
    mgr.debug_assert_namespaces_fresh();
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "namespace 소유 표가 낡았다")]
fn a_source_write_after_the_derivation_is_caught() {
    let mut mgr = manager_with(vec![fake_package()]);
    // 소유자 표를 갱신하지 않고 설치 목록만 바꾼다.
    mgr.overwrite_packages_without_deriving_for_tests(Vec::new());
    mgr.debug_assert_namespaces_fresh();
}
