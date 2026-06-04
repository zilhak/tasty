//! H — plugin 자동 reload 단위 테스트.
//!
//! 검증 대상:
//! - `capture_plugin_baseline` — entry binary mtime + manifest version 캡처
//! - `check_for_updates` — baseline 대비 변경 감지 (binary mtime / manifest version)
//! - flag 가 off 면 변경 감지 안 함
//!
//! `auto_reload_one` 의 happy path 는 PluginProcess::spawn 이 필요해 별도
//! 통합 테스트로 이전 (본 모듈은 process spawn 없이 검증 가능한 부분만).

use std::sync::Arc;
use std::time::SystemTime;

use tasty_plugin_manifest::{Manifest, PluginPackage};
use tasty_terminal::waker_factory::NoopWakerFactory;

use crate::manager::PluginManager;

const FAKE_MANIFEST: &str = r#"
manifest_version = 1
id = "com.example.autoreload_test"
name = "Auto Reload Test"
version = "0.1.0"
api_version = "1.0"

[entry]
type = "process"
command = "fake-bin"
args = []
"#;

fn parse_manifest() -> Manifest {
    toml::from_str(FAKE_MANIFEST).expect("fake manifest should parse")
}

fn mgr() -> PluginManager {
    PluginManager::new(Arc::new(NoopWakerFactory))
}

/// 임시 디렉터리 안에 entry binary 파일 (`fake-bin`) 을 만들고 PluginPackage 생성.
/// 반환된 TempDir 은 호출부에서 drop 까지 보관해야 path 가 유지된다.
fn make_pkg_with_binary(version: &str) -> (tempfile::TempDir, PluginPackage) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bin = tmp.path().join("fake-bin");
    std::fs::write(&bin, b"#!/bin/sh\nexit 0\n").expect("write fake binary");
    let mut manifest = parse_manifest();
    manifest.version = version.to_string();
    let pkg = PluginPackage {
        dir: tmp.path().to_path_buf(),
        manifest,
    };
    (tmp, pkg)
}

#[test]
fn fields_default_to_disabled() {
    let m = mgr();
    assert!(!m.auto_reload_enabled, "default off");
    assert!(m.plugin_binary_mtimes.is_empty());
    assert!(m.plugin_manifest_versions.is_empty());
}

#[test]
fn capture_baseline_records_mtime_and_version() {
    let (_tmp, pkg) = make_pkg_with_binary("0.1.0");
    let pid = pkg.manifest.id.clone();
    let mut m = mgr();
    m.packages.push(pkg);

    m.capture_plugin_baseline(&pid);

    assert!(m.plugin_binary_mtimes.contains_key(&pid));
    assert_eq!(
        m.plugin_manifest_versions.get(&pid).map(String::as_str),
        Some("0.1.0")
    );
}

#[test]
fn capture_baseline_missing_binary_keeps_version_only() {
    // entry binary 파일이 없는 PluginPackage. metadata 실패 → mtime skip,
    // version 은 그래도 기록되어야 한다.
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut manifest = parse_manifest();
    manifest.version = "0.2.0".to_string();
    let pkg = PluginPackage {
        dir: tmp.path().to_path_buf(),
        manifest,
    };
    let pid = pkg.manifest.id.clone();

    let mut m = mgr();
    m.packages.push(pkg);
    m.capture_plugin_baseline(&pid);

    assert!(!m.plugin_binary_mtimes.contains_key(&pid));
    assert_eq!(
        m.plugin_manifest_versions.get(&pid).map(String::as_str),
        Some("0.2.0")
    );
}

#[test]
fn capture_baseline_clears_stale_mtime_on_missing_binary() {
    // 한 번 캡처 후 binary 가 사라진 상태에서 재캡처 — stale mtime 이 남으면
    // 후속 check_for_updates 가 binary 없음 ≠ 변경 판정. 제거되어야 한다.
    let (tmp, pkg) = make_pkg_with_binary("0.1.0");
    let pid = pkg.manifest.id.clone();
    let mut m = mgr();
    m.packages.push(pkg);
    m.capture_plugin_baseline(&pid);
    assert!(m.plugin_binary_mtimes.contains_key(&pid));

    std::fs::remove_file(tmp.path().join("fake-bin")).expect("remove fake-bin");
    m.capture_plugin_baseline(&pid);
    assert!(!m.plugin_binary_mtimes.contains_key(&pid));
}

#[test]
fn capture_baseline_unknown_plugin_is_noop() {
    let mut m = mgr();
    m.capture_plugin_baseline("com.example.ghost");
    assert!(m.plugin_binary_mtimes.is_empty());
    assert!(m.plugin_manifest_versions.is_empty());
}

// 본 모듈은 H.c 의 check_for_updates / H.d 의 auto_reload_one 가 추가되면 그
// 테스트도 함께 확장한다. unused import 방지를 위해 SystemTime 만 export.
#[allow(dead_code)]
fn _unused() -> SystemTime {
    SystemTime::now()
}
