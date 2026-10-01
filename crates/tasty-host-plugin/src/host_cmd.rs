//! surface 생성·복원 시 PluginManager에 추적할 상태 핸들을 전달한다.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::Value;

/// `RemoteSurface`의 내부 상태에 manager가 외부에서 접근하기 위한 핸들.
#[derive(Clone)]
pub struct SurfaceHandles {
    pub display_name: Arc<Mutex<String>>,
    /// plugin 이 `SurfaceResult.snapshot` 으로 piggyback 한 영속화용 데이터.
    /// 매 응답마다 manager 가 최신값으로 갱신하며, `SavedLayout::capture` 시
    /// `registry.get(kind).snapshot(surface)` 가 이 값을 읽어 disk 에 저장.
    pub snapshot_cache: Arc<Mutex<Option<Value>>>,
}

/// Process-local identity of one remote kind instance, independent of its reusable surface ID.
#[derive(Clone)]
pub struct SurfaceBinding(std::sync::Weak<Mutex<Option<Value>>>);

impl SurfaceHandles {
    pub fn binding(&self) -> SurfaceBinding {
        SurfaceBinding(Arc::downgrade(&self.snapshot_cache))
    }
}

impl SurfaceBinding {
    /// False means every original handle (including queued Created and manager registration)
    /// has been dropped. This is evidence of disposal, not a lookup by reusable surface ID.
    pub fn is_alive(&self)->bool {self.0.strong_count()!=0}

    pub fn same_instance(&self, other:&Self)->bool {self.0.ptr_eq(&other.0)}

    pub fn matches(&self, handles: &SurfaceHandles) -> bool {
        self.0.ptr_eq(&Arc::downgrade(&handles.snapshot_cache))
    }
}

/// Opaque identity of one mesh bootstrap. Only the plugin manager can change its state.
#[derive(Clone, Default)]
pub struct MeshBinding {
    identity: Arc<Mutex<Option<Value>>>,
    pub(crate) publication: Arc<Mutex<MeshPublication>>,
}
#[derive(Default)]
pub(crate) enum MeshPublication {
    #[default]
    NeverSent,
    Sent(Vec<MeshBootstrap>),
    Retiring(RemoteRetirementReceipt),
}
pub(crate) struct MeshBootstrap {
    pub(crate) plugin: String,
    pub(crate) process: std::sync::Weak<()>,
    pub(crate) request: u64,
}
impl MeshBinding {
    pub fn binding(&self) -> SurfaceBinding { SurfaceBinding(Arc::downgrade(&self.identity)) }
}

/// Exact destroy-RPC observation. Neither FIFO enqueue nor dropping the host kind is an ACK.
#[derive(Clone)]
pub struct RemoteRetirementReceipt {
    state: Arc<std::sync::OnceLock<Result<(), String>>>,
    surface_id: u32,
    binding: SurfaceBinding,
    children: Vec<RemoteRetirementReceipt>,
}
pub struct RemoteRetirementCompletion(Arc<std::sync::OnceLock<Result<(), String>>>);
impl RemoteRetirementReceipt {
    pub fn pending(surface_id:u32,binding:SurfaceBinding) -> (Self, RemoteRetirementCompletion) {
        let state = Arc::new(std::sync::OnceLock::new());
        (Self {state:state.clone(),surface_id,binding,children:Vec::new()}, RemoteRetirementCompletion(state))
    }
    pub(crate) fn group(surface_id:u32,binding:SurfaceBinding,children:Vec<Self>)->Self {
        Self {state:Arc::new(std::sync::OnceLock::new()),surface_id,binding,children}
    }
    pub fn observation(&self) -> Option<Result<(), String>> {
        if self.children.is_empty() {return self.state.get().cloned();}
        let mut pending=false;
        for child in &self.children {
            match child.observation() {Some(Err(reason))=>return Some(Err(reason)),None=>pending=true,Some(Ok(()))=>{}}
        }
        if pending {None} else {Some(Ok(()))}
    }
    pub fn matches(&self,surface_id:u32,binding:&SurfaceBinding)->bool {
        self.surface_id==surface_id && self.binding.0.ptr_eq(&binding.0)
    }
}
impl RemoteRetirementCompletion {
    pub fn finish(self, result: Result<(), String>) {
        if self.0.set(result).is_err() {tracing::warn!("remote retirement receipt was already settled");}
    }
}
impl Drop for RemoteRetirementCompletion {
    fn drop(&mut self) {
        if self.0.get().is_none() {
            // A cancelled request/connection is not evidence that the plugin destroyed its owner.
            if self.0.set(Err("remote retirement acknowledgement was lost".into())).is_err() {
                tracing::warn!("remote retirement producer ended during settlement");
            }
        }
    }
}

/// registry create/restore closure가 manager에게 보내는 명령.
pub enum HostCmd {
    RemoteSurfaceRetired {
        surface_id: u32,
        binding: SurfaceBinding,
        completion: Option<RemoteRetirementCompletion>,
    },
    RemoteSurfaceCreated {
        surface_id: u32,
        plugin_id: String,
        kind: String,
        /// 호스트가 전달한 시작 cwd. docs/design/policies/cwd.md#surface-cwd-invariant 참조.
        cwd: Option<PathBuf>,
        params: Value,
        handles: SurfaceHandles,
    },
    RemoteSurfaceRestored {
        surface_id: u32,
        plugin_id: String,
        kind: String,
        data: Value,
        handles: SurfaceHandles,
    },
}
