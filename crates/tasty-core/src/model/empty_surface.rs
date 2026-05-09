use super::SurfaceId;
use super::surface_trait::Surface;

/// An empty surface placeholder (shows convert button).
pub struct EmptySurface {
    pub id: SurfaceId,
}

impl EmptySurface {
    pub fn new(id: SurfaceId) -> Self {
        Self { id }
    }
}

impl Surface for EmptySurface {
    crate::impl_surface_any!();

    fn kind(&self) -> &'static str {
        "empty"
    }
    fn type_name(&self) -> &'static str {
        "Empty"
    }
    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }
}
