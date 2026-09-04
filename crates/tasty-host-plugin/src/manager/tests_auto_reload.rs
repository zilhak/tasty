//! H — plugin 자동 reload 단위 테스트.
//!
//! 검증 대상:
//! - `capture_plugin_baseline` — entry binary mtime + manifest version 캡처
//! - `check_for_updates` — baseline 대비 변경 감지 (binary mtime / manifest version)
//! - flag 가 off 면 변경 감지 안 함
//!
//! `auto_reload_one` 의 happy path 는 PluginProcess::spawn 이 필요해 별도
//! 통합 테스트로 이전 (본 모듈은 process spawn 없이 검증 가능한 부분만).

// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다 — 전수 가드
// (`tests/let_underscore_documented.rs`)가 테스트 본문을 제외하므로, 여기서 나는
// `let_underscore_must_use` 경고는 정책상 조치 대상이 될 수 없다. 끄지 않으면
// 프로덕션의 진짜 신호가 그 안에 묻힌다 — `docs/dev-guide/error-handling.md`.
#![allow(clippy::let_underscore_must_use)]

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

#[test]
fn check_for_updates_empty_when_disabled() {
    let (_tmp, pkg) = make_pkg_with_binary("0.1.0");
    let pid = pkg.manifest.id.clone();
    let mut m = mgr();
    m.packages.push(pkg);
    m.processes.insert(
        pid.clone(),
        crate::process::PluginProcess::stub_for_test(&pid),
    );
    // baseline 을 일부러 깨뜨려둬도 flag off 면 무시되어야 함.
    m.plugin_binary_mtimes
        .insert(pid.clone(), SystemTime::UNIX_EPOCH);
    m.plugin_manifest_versions.insert(pid, "0.0.0".into());

    assert!(m.check_for_updates().is_empty());
}

#[test]
fn check_for_updates_detects_binary_mtime_change() {
    let (tmp, pkg) = make_pkg_with_binary("0.1.0");
    let pid = pkg.manifest.id.clone();
    let mut m = mgr();
    m.auto_reload_enabled = true;
    m.packages.push(pkg);
    m.processes.insert(
        pid.clone(),
        crate::process::PluginProcess::stub_for_test(&pid),
    );
    // baseline 의 mtime 을 일부러 과거로 — 실제 파일 mtime 과 무조건 다르도록.
    m.plugin_binary_mtimes
        .insert(pid.clone(), SystemTime::UNIX_EPOCH);
    m.plugin_manifest_versions
        .insert(pid.clone(), "0.1.0".into());

    // sanity: 실제 binary mtime 은 epoch 이상.
    let cur_mtime = std::fs::metadata(tmp.path().join("fake-bin"))
        .and_then(|md| md.modified())
        .unwrap();
    assert!(cur_mtime > SystemTime::UNIX_EPOCH);

    assert_eq!(m.check_for_updates(), vec![pid]);
}

#[test]
fn check_for_updates_detects_version_change() {
    let (_tmp, mut pkg) = make_pkg_with_binary("0.1.0");
    let pid = pkg.manifest.id.clone();
    pkg.manifest.version = "0.2.0".into();
    let mut m = mgr();
    m.auto_reload_enabled = true;
    m.packages.push(pkg);
    m.processes.insert(
        pid.clone(),
        crate::process::PluginProcess::stub_for_test(&pid),
    );
    // mtime 은 fresh 로 캡처 — version 신호만 발동.
    m.capture_plugin_baseline(&pid);
    m.plugin_manifest_versions
        .insert(pid.clone(), "0.1.0".into());

    assert_eq!(m.check_for_updates(), vec![pid]);
}

#[test]
fn check_for_updates_no_diff_after_fresh_baseline() {
    let (_tmp, pkg) = make_pkg_with_binary("0.1.0");
    let pid = pkg.manifest.id.clone();
    let mut m = mgr();
    m.auto_reload_enabled = true;
    m.packages.push(pkg);
    m.processes.insert(
        pid.clone(),
        crate::process::PluginProcess::stub_for_test(&pid),
    );
    m.capture_plugin_baseline(&pid);

    assert!(m.check_for_updates().is_empty());
}

#[test]
fn check_for_updates_skips_plugins_not_running() {
    let (_tmp, pkg) = make_pkg_with_binary("0.1.0");
    let pid = pkg.manifest.id.clone();
    let mut m = mgr();
    m.auto_reload_enabled = true;
    m.packages.push(pkg);
    // processes 에 등록 안 함 — 비활성. baseline diff 가 있어도 skip.
    m.plugin_binary_mtimes
        .insert(pid.clone(), SystemTime::UNIX_EPOCH);
    m.plugin_manifest_versions.insert(pid, "0.0.0".into());

    assert!(m.check_for_updates().is_empty());
}

#[test]
fn auto_reload_one_updates_baseline_even_when_respawn_fails() {
    // listener 미존재 — swap_respawn 의 ensure_listener 가 bind 실패할 수도
    // 있고 성공해도 fake-bin 이 실제 PluginProcess 가 아니므로 spawn 실패.
    // 어느 쪽이든 baseline 은 capture 되어 다음 tick 의 무한 swap 회피해야 함.
    let (tmp, pkg) = make_pkg_with_binary("0.1.0");
    let pid = pkg.manifest.id.clone();
    let mut m = mgr();
    m.auto_reload_enabled = true;
    m.packages.push(pkg);
    m.processes.insert(
        pid.clone(),
        crate::process::PluginProcess::stub_for_test(&pid),
    );
    // baseline 의 mtime 을 일부러 과거로 — diff 발동 → swap 시도.
    m.plugin_binary_mtimes
        .insert(pid.clone(), SystemTime::UNIX_EPOCH);
    m.plugin_manifest_versions
        .insert(pid.clone(), "0.1.0".into());

    let _ = m.auto_reload_one(&pid); // 본 테스트는 baseline 갱신만 검증 — respawn 실패 무시.

    // shutdown 후 baseline 의 mtime 은 실제 파일 mtime 으로 갱신되었어야 함.
    let stored = m.plugin_binary_mtimes.get(&pid).copied().unwrap();
    let actual = std::fs::metadata(tmp.path().join("fake-bin"))
        .and_then(|md| md.modified())
        .unwrap();
    assert_eq!(stored, actual);

    // 다음 check_for_updates 호출은 빈 결과 — 동일 mtime 으로 무한 swap 회피.
    // (단 respawn 실패로 processes 는 비어 있으므로 check 자체가 skip.)
    assert!(m.check_for_updates().is_empty());
}

#[test]
fn check_for_updates_skips_when_no_baseline() {
    // baseline 이 한 번도 안 잡힌 plugin — diff 비교 불가 → false. 무한 reload 회피.
    let (_tmp, pkg) = make_pkg_with_binary("0.1.0");
    let pid = pkg.manifest.id.clone();
    let mut m = mgr();
    m.auto_reload_enabled = true;
    m.packages.push(pkg);
    m.processes.insert(
        pid.clone(),
        crate::process::PluginProcess::stub_for_test(&pid),
    );
    // baseline 비어있음.

    assert!(m.check_for_updates().is_empty());
}
