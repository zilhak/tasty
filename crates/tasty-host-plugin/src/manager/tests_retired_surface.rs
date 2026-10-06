//! 원 프로세스가 회수된 뒤 그 프로세스가 만든 surface 를 닫으면 회수를 소멸 증거로 쓴다.

use std::sync::Arc;

use serde_json::json;
use tasty_terminal::waker_factory::NoopWakerFactory;

use super::PluginManager;
use crate::host_cmd::{HostCmd, SurfaceHandles};
use crate::process::PluginProcess;

const PLUGIN: &str = "com.test.document";

fn handles() -> SurfaceHandles {
    SurfaceHandles {
        display_name: Arc::default(),
        snapshot_cache: Arc::default(),
    }
}

/// 실행 중인 stub 프로세스와 그 프로세스에 게시된 surface 7.
fn manager_with_surface() -> (PluginManager, SurfaceHandles) {
    let mut mgr = PluginManager::new(Arc::new(NoopWakerFactory));
    let (process, _requests) = PluginProcess::stub_with_request_rx(PLUGIN);
    mgr.processes.insert(PLUGIN.into(), process);
    let surface = handles();
    mgr.host_cmd_tx
        .send(HostCmd::RemoteSurfaceCreated {
            surface_id: 7,
            plugin_id: PLUGIN.into(),
            kind: "document".into(),
            cwd: None,
            params: json!({}),
            handles: surface.clone(),
        })
        .unwrap();
    mgr.drain_host_cmds();
    (mgr, surface)
}

fn retire(mgr: &mut PluginManager) {
    let process = mgr.processes.remove(PLUGIN).expect("running stub");
    mgr.retire_process(PLUGIN, process, false);
}

fn close(
    mgr: &mut PluginManager,
    surface: &SurfaceHandles,
) -> crate::host_cmd::RemoteRetirementReceipt {
    let receipt = mgr
        .enqueue_observed_remote_retirement(7, surface.binding())
        .unwrap();
    mgr.drain_host_cmds();
    receipt
}

#[test]
fn closing_after_the_original_process_was_reaped_succeeds() {
    let (mut mgr, surface) = manager_with_surface();
    retire(&mut mgr);
    assert!(!mgr.wait_retired(PLUGIN), "재시작 예약이 없는 회수다");
    let receipt = close(&mut mgr, &surface);
    assert_eq!(receipt.observation(), Some(Ok(())));
}

#[test]
fn closing_after_a_new_process_started_still_uses_the_old_generation() {
    let (mut mgr, surface) = manager_with_surface();
    retire(&mut mgr);
    assert!(!mgr.wait_retired(PLUGIN), "재시작 예약이 없는 회수다");
    let (fresh, requests) = PluginProcess::stub_with_request_rx(PLUGIN);
    mgr.processes.insert(PLUGIN.into(), fresh);
    let receipt = close(&mut mgr, &surface);
    assert_eq!(receipt.observation(), Some(Ok(())));
    assert!(
        requests.try_recv().is_err(),
        "새 프로세스는 옛 surface 를 모르므로 파괴 요청을 받지 않는다"
    );
}

#[test]
fn closing_while_the_original_process_is_retiring_settles_at_the_reap() {
    let (mut mgr, surface) = manager_with_surface();
    retire(&mut mgr);
    let receipt = close(&mut mgr, &surface);
    assert_eq!(receipt.observation(), None, "회수 전에는 확정하지 않는다");
    assert!(!mgr.wait_retired(PLUGIN), "재시작 예약이 없는 회수다");
    assert_eq!(receipt.observation(), Some(Ok(())));
}

#[test]
fn a_process_that_vanished_without_a_retirement_stays_unconfirmed() {
    let (mut mgr, surface) = manager_with_surface();
    drop(mgr.processes.remove(PLUGIN));
    let receipt = close(&mut mgr, &surface);
    assert_eq!(
        receipt.observation(),
        Some(Err(
            "original plugin process is unavailable for destruction".into()
        ))
    );
}
