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
    fn type_name(&self) -> &'static str {
        "Empty"
    }
    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }
    fn as_empty_surface(&self) -> Option<&EmptySurface> {
        Some(self)
    }
}
