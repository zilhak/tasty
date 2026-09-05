//! 유도된 확장 집합이 **원본과 어긋나면 디버그 단정이 잡는가.**
//!
//! 텍스트 가드는 "유도를 안 불렀다" 는 잡지만 "불렀는데 원본이 그 뒤에 또 바뀌었다"
//! 는 못 잡는다 — 흐름 판정이라 줄 단위 스캔의 범위 밖이다. 그 부류를 실행 시점으로
//! 옮긴 것이 `debug_assert_extensions_fresh` 이고, 여기서 **실제로 터지는지**를 못
//! 박는다(안 터지는 단정은 아무것도 안 지킨다).

use std::sync::Arc;

use tasty_plugin_manifest::{Manifest, PluginPackage};
use tasty_terminal::waker_factory::NoopWakerFactory;

use crate::manager::PluginManager;

const TARGET: &str = "com.example.freshness_target";
const EXT: &str = "com.example.freshness_ext";

fn manifest_toml(id: &str, extends: bool) -> String {
    let head = format!(
        r#"
manifest_version = 1
id = "{id}"
name = "Freshness Fixture"
version = "0.1.0"
api_version = "1.0"

[entry]
type = "process"
command = "fake-bin"
args = []
"#
    );
    if extends {
        format!(
            r#"{head}
[extends]
plugin_id = "{TARGET}"
version_req = "*"
api_version = "1.0"
"#
        )
    } else {
        head
    }
}

fn pkg(id: &str, extends: bool) -> PluginPackage {
    let manifest: Manifest =
        toml::from_str(&manifest_toml(id, extends)).expect("fixture manifest should parse");
    PluginPackage {
        dir: std::path::PathBuf::from("/nonexistent/freshness-fixture"),
        manifest,
    }
}

/// target + 그것을 확장하는 plugin 을 얹고, 확장 권한을 준 뒤 유도까지 마친 매니저.
fn manager_with_active_extension() -> PluginManager {
    let mut m = PluginManager::new(Arc::new(NoopWakerFactory));
    m.packages.push(pkg(TARGET, false));
    m.packages.push(pkg(EXT, true));
    m.config.set_granted(EXT, vec![format!("ext:{TARGET}")]);
    m.recompute_extensions();
    m
}

/// 유도가 원본보다 앞서 있으면 단정이 통과한다 — 대조군.
#[test]
fn a_freshly_recomputed_set_passes() {
    let m = manager_with_active_extension();
    m.debug_assert_extensions_fresh();
}

/// 유도 **뒤에** 원본(config)이 바뀌면 단정이 터진다.
///
/// 이것이 `plugin_install` 에서 실제로 났던 순서다 — `recompute_extensions()` 를
/// `config.set_granted()` 앞에서 불렀다. 거기서는 뒤따르는 `enable` 이 한 번 더
/// 계산해 가려져 있었고, 그 `enable` 은 `is_disabled` 분기에서 건너뛴다.
#[test]
#[should_panic(expected = "확장 집합이 낡았다")]
fn a_source_write_after_the_derivation_is_caught() {
    let mut m = manager_with_active_extension();
    // 유도를 다시 안 부르고 원본만 바꾼다.
    m.config.disable(EXT);
    m.debug_assert_extensions_fresh();
}

/// 판정이 **정말 값을 본다** — 위 단정이 그냥 항상 터지는 것이 아니다.
///
/// 원본을 바꿨다가 유도를 다시 부르면 통과해야 한다. 이것이 없으면 위 테스트는
/// "단정이 늘 터진다" 로도 초록이다.
#[test]
fn recomputing_after_the_write_makes_it_fresh_again() {
    let mut m = manager_with_active_extension();
    m.config.disable(EXT);
    m.recompute_extensions();
    m.debug_assert_extensions_fresh();
}
