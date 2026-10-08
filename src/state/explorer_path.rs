//! Explorer 경로의 출처. 원격 mirror 경로와 로컬 경로는 문자열이 같아도 다른 파일을 가리킨다.

use super::{ExplorerClipboard, ExplorerPathSource};

impl ExplorerPathSource {
    /// surface가 보여 주는 파일시스템. surface가 없으면 None이다.
    pub(crate) fn of_surface(
        engine: &crate::runtime::engine_read::EngineRead<'_>,
        surface_id: u32,
    ) -> Option<Self> {
        let (index, _) = engine.find_workspace_index_for_surface(surface_id)?;
        let workspace = engine.workspace_at(index)?;
        Some(if workspace.mirror {
            Self::Remote {
                workspace: workspace.id,
            }
        } else {
            Self::Local
        })
    }
}

impl ExplorerClipboard {
    /// 로컬 파일 작업의 입력으로 쓸 수 있는가. 원격에서 복사한 경로는 로컬 경로로 해석하지 않는다.
    pub(crate) fn is_local(&self) -> bool {
        self.source == ExplorerPathSource::Local
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_surface_is_a_local_source() {
        let (_state, engine) = crate::state::tests::test_state();
        let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
        assert_eq!(
            ExplorerPathSource::of_surface(&engine.read(), sid),
            Some(ExplorerPathSource::Local)
        );
        assert_eq!(ExplorerPathSource::of_surface(&engine.read(), 9_999), None);
    }

    #[test]
    fn a_mirror_surface_is_a_remote_source_and_its_paths_are_not_local() {
        let (_state, engine) = crate::state::tests::test_mirror_state();
        let read = engine.read();
        let workspace = read.workspace_at(0).unwrap();
        let sid = workspace.all_surface_ids()[0];
        let source = ExplorerPathSource::of_surface(&read, sid);
        assert_eq!(
            source,
            Some(ExplorerPathSource::Remote {
                workspace: workspace.id
            })
        );
        let clipboard = ExplorerClipboard {
            identity: Default::default(),
            paths: vec!["/same/path/on/both/hosts".into()],
            cut: false,
            source: source.unwrap(),
        };
        assert!(!clipboard.is_local());
    }
}
