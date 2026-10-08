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
    /// 같은 surface 와 자원 세대인지만 본다. mirror projection 이 다시 만들어져도 참이다.
    /// 파일시스템을 바꾸지 않는 메뉴 결과처럼 projection 세대가 결과를 바꾸지 않는 곳에 쓴다.
    #[cfg(feature = "gui")]
    pub(crate) fn same_surface_in(
        &self,
        engine: &crate::runtime::engine_read::EngineRead<'_>,
    ) -> bool {
        engine
            .core
            .find_surface_by_id(self.surface)
            .is_some_and(|descriptor| descriptor.activation_generation == self.activation)
            && self.resource.is_none_or(|generation| {
                engine.terminals.generation(self.surface) == Some(generation)
            })
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
    /// 화면 쪽 읽기 view에서 같은 surface 세대인지 확인한다. 메뉴 결과처럼 나중에 도착하는 선택에 쓴다.
    #[cfg(feature = "gui")]
    pub(crate) fn current_in(&self, engine: &crate::runtime::engine_read::EngineRead<'_>) -> bool {
        self.same_surface_in(engine)
            && self.mirror.as_ref().is_none_or(|(workspace, token)| {
                engine.matches_mirror_projection(*workspace, token)
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

/// 같은 대상을 같은 세대로 잡았으면 같다. projection 토큰은 주소로 비교한다.
impl PartialEq for SurfaceBinding {
    fn eq(&self, other: &Self) -> bool {
        self.surface == other.surface
            && self.activation == other.activation
            && self.resource == other.resource
            && match (&self.mirror, &other.mirror) {
                (None, None) => true,
                (Some((a, x)), Some((b, y))) => a == b && x.ptr_eq(y),
                _ => false,
            }
    }
}
impl Eq for SurfaceBinding {}

#[cfg(all(test, feature = "gui"))]
mod tests {
    use super::SurfaceBinding;

    #[test]
    fn a_menu_target_stops_matching_once_its_surface_is_replaced_or_gone() {
        let (_state, mut engine) = crate::state::tests::test_state();
        let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
        let binding = SurfaceBinding::capture(&engine.read(), sid).unwrap();
        assert!(binding.current_in(&engine.read()));
        assert!(SurfaceBinding::capture(&engine.read(), 9_999).is_none());

        engine.borrow_mut().runtime.terminals.insert(
            sid,
            tasty_terminal::Terminal::new_detached(80, 24),
            None,
        );
        assert!(
            !binding.current_in(&engine.read()),
            "a replaced terminal is a different target"
        );

        let replaced = SurfaceBinding::capture(&engine.read(), sid).unwrap();
        assert!(engine.borrow_mut().runtime.terminals.remove(sid).is_some());
        assert!(!replaced.current_in(&engine.read()), "a closed surface");
    }
}
