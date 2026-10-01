//! A logical leaf. Kind instances, launch recipes, handles and deferred factories have other owners.
use crate::SurfaceId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceDescriptor {
    pub id: SurfaceId,
    pub kind: String,
    /// Applied activation fact, distinct from process-local PTY and content generations.
    pub activation_generation: Option<u64>,
}

impl SurfaceDescriptor {
    pub fn new(id: SurfaceId, kind: impl Into<String>) -> Self {
        Self {
            id,
            kind: kind.into(),
            activation_generation: None,
        }
    }
    pub fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }
    pub fn kind(&self) -> &str {
        &self.kind
    }
}
