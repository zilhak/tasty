//! A captured target retains logical, physical and mirror generations without exposing an owner.
use crate::runtime::engine_access::EngineRef;

#[derive(Clone, Debug)]
pub(crate) struct SurfaceBinding {
    surface: u32,
    activation: Option<u64>,
    resource: Option<tasty_terminal::ResourceGeneration>,
    mirror: Option<(u32, std::sync::Weak<()>)>,
}
impl SurfaceBinding {
    pub(crate) fn surface_id(&self) -> u32 {
        self.surface
    }
    #[cfg(feature = "gui")]
    pub(crate) fn mirror_workspace(&self) -> Option<u32> {
        self.mirror.as_ref().map(|(workspace, _)| *workspace)
    }

    #[cfg(feature = "gui")]
    pub(crate) fn capture(
        engine: &crate::runtime::engine_read::EngineRead<'_>,
        surface: u32,
    ) -> Option<Self> {
        let descriptor = engine.core.find_surface_by_id(surface)?;
        let mirror = engine
            .find_workspace_index_for_surface(surface)
            .and_then(|(index, _)| engine.workspace_at(index))
            .filter(|workspace| workspace.mirror)
            .and_then(|workspace| {
                engine
                    .mirror_projection_token(workspace.id)
                    .map(|token| (workspace.id, token))
            });
        Some(Self {
            surface,
            activation: descriptor.activation_generation,
            resource: engine.terminals.generation(surface),
            mirror,
        })
    }
    pub(crate) fn current(&self, engine: &EngineRef<'_>) -> bool {
        engine
            .core
            .find_surface_by_id(self.surface)
            .is_some_and(|descriptor| descriptor.activation_generation == self.activation)
            && self.resource.is_none_or(|generation| {
                engine
                    .runtime
                    .terminals
                    .matches_generation(self.surface, generation)
            })
            && self.mirror.as_ref().is_none_or(|(workspace, token)| {
                engine.matches_mirror_projection(*workspace, token)
            })
    }
}
