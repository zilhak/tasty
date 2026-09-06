//! namespace 소유 표가 **설치된 매니페스트에서 유도되는가**.
//!
//! 이 파일은 `tests_namespace_mirror.rs` 를 대체한다. 옛 파일이 묻던 것은 "host 의 표와
//! `tasty-ipc` 의 미러가 동조하는가" 였는데, 표가 하나가 되면서 **비교할 사본이 없어졌다.**
//! 다만 그때 함께 묻고 있던 것 — 매니페스트가 소유를 만들고, 설치가 사라지면 소유도
//! 사라진다 — 는 그대로 남는다. 그래서 삭제가 아니라 이 자리로 옮겼다.
//!
//! 옛 이름 하나는 **틀린 이름이었다**: `disable_path_unregisters_prefix`. ADR-0173 이후
//! disable 은 소유를 건드리지 않는다(꺼진 plugin 도 자기 이름의 주인이고, 라우터가
//! `-32002` 로 따로 답한다). 소유를 잃는 것은 **제거**다. 아래 이름이 그것을 반영한다.
//!
//! 해소 쪽 물음("`method_meta` 가 그 prefix 를 푸는가")은 `tasty-ipc` 의
//! `method_meta_tests.rs` 에 있다 — 표를 설치할 수 있는 자리가 거기다.

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

/// 설치 목록을 직접 놓고 유도만 돌린다 — 디스크 스캔(`refresh_packages`)은 이 물음의
/// 재료가 아니다. 재료는 "무엇이 설치돼 있는가" 하나다.
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
        "설치가 없으면 주인도 없다"
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
        "주인이 다른 plugin 이면 forward 대상이다"
    );
    assert!(
        !mgr.namespace_belongs_to_other("nstest.invoke", "com.example.namespace_table_test"),
        "자기 namespace 를 자기가 부르는 것은 forward 가 아니라 trampoline 이다"
    );
}

/// **제거**가 소유를 거둔다 — disable 이 아니다(ADR-0173).
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
        "다시 설치하면 소유도 돌아온다 — 유도는 상태가 아니라 함수다"
    );
}

/// 유도 신선도 단정이 **낡음을 실제로 잡는가**. 통과하는 쪽만 있으면 늘 참인 단정도
/// 초록이라, 반대 방향을 함께 둔다.
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
    // 유도를 거치지 않고 원본만 바꾼다 — 이것이 잡아야 할 형태다.
    mgr.overwrite_packages_without_deriving_for_tests(Vec::new());
    mgr.debug_assert_namespaces_fresh();
}
