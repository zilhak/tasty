//! 원 프로세스가 회수된 뒤 남은 remote surface 를 새 프로세스에 다시 게시한다.

use std::sync::Arc;

use serde_json::json;
use tasty_terminal::waker_factory::NoopWakerFactory;

use super::{PluginManager, RemotePublication};
use crate::host_cmd::{HostCmd, SurfaceHandles};
use crate::process::{PluginProcess, RequestTap};
use crate::protocol;

const PLUGIN: &str = "com.test.document";

fn handles() -> SurfaceHandles {
    SurfaceHandles {
        display_name: Arc::default(),
        snapshot_cache: Arc::default(),
    }
}

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
            params: json!({"file": "/tmp/a.md"}),
            handles: surface.clone(),
        })
        .unwrap();
    mgr.drain_host_cmds();
    (mgr, surface)
}

/// 원 프로세스를 회수하고 새 stub 을 띄운 뒤 재게시한다.
fn restart(mgr: &mut PluginManager) -> RequestTap {
    let process = mgr.processes.remove(PLUGIN).expect("running stub");
    mgr.retire_process(PLUGIN, process, false);
    assert!(!mgr.wait_retired(PLUGIN), "재시작 예약이 없는 회수다");
    let (fresh, requests) = PluginProcess::stub_with_request_rx(PLUGIN);
    mgr.processes.insert(PLUGIN.into(), fresh);
    mgr.reattach_orphan_surfaces(PLUGIN);
    requests
}

fn bound_to_current(mgr: &PluginManager) -> bool {
    let current = mgr.processes[PLUGIN].reply_binding();
    matches!(&mgr.surfaces[&7].publication, RemotePublication::Sent(b) if b.ptr_eq(&current))
}

#[test]
fn a_surface_with_a_snapshot_is_restored_on_the_new_process() {
    let (mut mgr, surface) = manager_with_surface();
    *surface.snapshot_cache.lock().unwrap() = Some(json!({"file": "/tmp/b.md"}));
    let requests = restart(&mut mgr);
    let request = requests
        .try_recv()
        .expect("새 프로세스가 복원 요청을 받는다");
    assert_eq!(request.method, protocol::METHOD_SURFACE_RESTORE);
    assert_eq!(
        request.params,
        json!({"surface_id": 7, "kind": "document", "data": {"file": "/tmp/b.md"}})
    );
    assert!(requests.try_recv().is_err(), "한 번만 보낸다");
    assert!(bound_to_current(&mgr));
}

#[test]
fn a_surface_without_a_snapshot_replays_its_original_request() {
    let (mut mgr, _surface) = manager_with_surface();
    let requests = restart(&mut mgr);
    let request = requests
        .try_recv()
        .expect("새 프로세스가 생성 요청을 받는다");
    assert_eq!(request.method, protocol::METHOD_SURFACE_CREATE);
    assert_eq!(
        request.params,
        json!({"surface_id": 7, "kind": "document", "cwd": null, "params": {"file": "/tmp/a.md"}})
    );
    assert!(bound_to_current(&mgr));
}

#[test]
fn a_surface_already_on_the_current_process_or_another_plugin_is_left_alone() {
    let (mut mgr, _surface) = manager_with_surface();
    let requests = restart(&mut mgr);
    assert!(requests.try_recv().is_ok());
    mgr.reattach_orphan_surfaces(PLUGIN);
    assert!(requests.try_recv().is_err(), "이미 새 프로세스에 게시됐다");
    let (other, other_requests) = PluginProcess::stub_with_request_rx("com.test.other");
    mgr.processes.insert("com.test.other".into(), other);
    mgr.reattach_orphan_surfaces("com.test.other");
    assert!(
        other_requests.try_recv().is_err(),
        "다른 플러그인의 surface 다"
    );
}

#[test]
fn closing_a_reattached_surface_asks_the_new_process_to_destroy_it() {
    let (mut mgr, surface) = manager_with_surface();
    let requests = restart(&mut mgr);
    assert!(requests.try_recv().is_ok());
    let receipt = mgr
        .enqueue_observed_remote_retirement(7, surface.binding())
        .unwrap();
    mgr.drain_host_cmds();
    let request = requests
        .try_recv()
        .expect("새 프로세스가 파괴 요청을 받는다");
    assert_eq!(request.method, protocol::METHOD_SURFACE_DESTROY);
    assert_eq!(receipt.observation(), None, "새 프로세스의 응답을 기다린다");
}
