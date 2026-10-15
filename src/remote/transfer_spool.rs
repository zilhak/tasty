//! Private transfer staging bounds memory without imposing a new maximum file size.
use crate::runtime::engine_access::EngineRef;
use std::{
    fs::{File, OpenOptions},
    io::{Seek, Write},
    path::{Path, PathBuf},
    sync::Weak,
};
use tasty_ipc::stream_hub::StreamHub;

pub(crate) struct Spool {
    file: Option<File>,
    path: PathBuf,
    len: u64,
}
impl Spool {
    pub(crate) fn new() -> Result<Self, String> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "tasty-transfer-{}-{now}-{serial}",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(|error| error.to_string())?;
        Ok(Self {
            file: Some(file),
            path,
            len: 0,
        })
    }
    pub(crate) fn len(&self) -> u64 {
        self.len
    }
    pub(crate) fn append(&mut self, bytes: &[u8]) -> Result<(), String> {
        let next = self
            .len
            .checked_add(bytes.len() as u64)
            .ok_or("transfer length overflow")?;
        self.file
            .as_mut()
            .ok_or("transfer spool closed")?
            .write_all(bytes)
            .map_err(|error| error.to_string())?;
        self.len = next;
        Ok(())
    }
    pub(crate) fn save_to(mut self, path: &Path) -> Result<(), String> {
        let source = self.file.as_mut().ok_or("transfer spool closed")?;
        source.rewind().map_err(|error| error.to_string())?;
        let mut destination = File::create(path).map_err(|error| error.to_string())?;
        std::io::copy(source, &mut destination).map_err(|error| error.to_string())?;
        Ok(())
    }
}
impl Drop for Spool {
    fn drop(&mut self) {
        drop(self.file.take());
        if let Err(error) = std::fs::remove_file(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(%error,"transfer spool cleanup failed");
        }
    }
}
#[derive(Clone)]
pub(crate) struct TransferOwner {
    registration: Weak<()>,
    grants: Vec<(u32, u32, u64)>,
}
impl TransferOwner {
    pub(crate) fn capture(
        engine: &EngineRef<'_>,
        hub: &StreamHub,
        client: u32,
        bulk: bool,
    ) -> Option<Self> {
        let workspace = if bulk {
            Some(hub.bulk_workspace(client)?)
        } else {
            None
        };
        let grants = engine
            .live
            .occupancy
            .workspaces_snapshot()
            .into_iter()
            .filter(|(id, grant)| {
                grant.ready
                    && workspace.map_or(grant.holder == client, |workspace| *id == workspace)
            })
            .map(|(id, grant)| (id, grant.holder, grant.granted_seq))
            .collect::<Vec<_>>();
        if grants.is_empty() {
            return None;
        }
        Some(Self {
            registration: hub.client_binding(client)?,
            grants,
        })
    }
    pub(crate) fn current(&self, engine: &EngineRef<'_>, hub: &StreamHub, client: u32) -> bool {
        hub.matches_client_binding(client, &self.registration)
            && engine
                .live
                .occupancy
                .workspaces_snapshot()
                .into_iter()
                .any(|(id, grant)| {
                    grant.ready && self.grants.contains(&(id, grant.holder, grant.granted_seq))
                })
    }
    /// 시험에서 grant 없이 연결 binding 만 가진 소유자를 만든다.
    #[cfg(all(test, feature = "gui"))]
    pub(crate) fn for_connection(registration: Weak<()>) -> Self {
        Self {
            registration,
            grants: Vec::new(),
        }
    }
    /// 업로드를 시작한 연결이 아직 같은 client 번호로 붙어 있는가. 끊긴 뒤 같은 번호로 새로
    /// 붙은 연결은 다른 연결이다.
    #[cfg(feature = "gui")]
    pub(crate) fn same_connection(&self, hub: &StreamHub, client: u32) -> bool {
        hub.matches_client_binding(client, &self.registration)
    }
    pub(crate) fn same(&self, other: &Self) -> bool {
        self.registration.ptr_eq(&other.registration)
            && self.grants.iter().any(|grant| other.grants.contains(grant))
    }
}
