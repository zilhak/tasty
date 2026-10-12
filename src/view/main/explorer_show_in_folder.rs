//! explorer 컨텍스트 메뉴의 "들어 있는 폴더에서 보기" 행. 하위 폴더 검색 결과나 링크 하나를 눌렀을 때
//! 메뉴 맨 앞에 둔다. 고르면 항목이 실제로 든 폴더로 옮기고 목록이 들어오면 그 항목을 고른다.
//! 검색 결과에는 그 앞에 "열기" 행을 더 둔다. 고르면 더블클릭처럼 폴더는 들어가고 파일은 연다.
//! 컨텍스트 메뉴의 생성 행처럼 메뉴 구성(`redraw.rs`)과 따로 둔다.

use super::MainView;
use crate::platform::native_menu::MenuItem;
use crate::runtime::engine_read::EngineRead;

const SHOW_IN_FOLDER: u32 = 90;
const OPEN_RESULT: u32 = 91;

/// 메뉴 맨 앞에 붙일 행. 시안 검색 결과 메뉴처럼 열기 → 들어 있는 폴더에서 보기 → 구분선 순서다.
/// 들어 있는 폴더가 없으면 아무것도 붙이지 않는다.
fn enclosing_head(open: bool, show_in_folder: bool) -> Vec<MenuItem> {
    if !show_in_folder {
        return Vec::new();
    }
    let mut head = Vec::with_capacity(3);
    if open {
        head.push(MenuItem::new(OPEN_RESULT, crate::i18n::t("button.open")));
    }
    head.push(MenuItem::new(
        SHOW_IN_FOLDER,
        crate::i18n::t("explorer.menu.show_in_folder"),
    ));
    head.push(MenuItem::separator());
    head
}

impl MainView {
    /// 대상이 하나이고 들어 있는 폴더가 있으면 메뉴 맨 앞에 행과 구분선을 둔다.
    /// 검색 결과면 그 앞에 열기 행도 둔다.
    pub(super) fn with_show_in_folder(
        &self,
        surface_id: u32,
        paths: &[std::path::PathBuf],
        mut items: Vec<MenuItem>,
    ) -> Vec<MenuItem> {
        let view = self.state.explorer_views.get(surface_id);
        let (open, show) = match (paths, view) {
            ([path], Some(v)) if !v.is_remote() && v.enclosing_target(path).is_some() => {
                (v.is_search_result(), true)
            }
            _ => (false, false),
        };
        items.splice(0..0, enclosing_head(open, show));
        items
    }

    /// 이 칸에 붙여넣을 수 있는 목록이 있는가. 다른 컴퓨터의 경로는 붙여넣지 않는다.
    pub(super) fn explorer_menu_has_clip(&self, engine: &EngineRead<'_>, surface_id: u32) -> bool {
        crate::state::ExplorerPathSource::of_surface(engine, surface_id)
            .is_some_and(|target| self.state.explorer_can_paste_into(target))
    }

    /// 다른 행이 처리하지 않은 메뉴 결과. 들어 있는 폴더에서 보기가 아니면 생성 행이나 이력 행이다.
    pub(super) fn explorer_menu_more(
        &mut self,
        engine: &EngineRead<'_>,
        surface_id: u32,
        id: u32,
        paths: &[std::path::PathBuf],
        cwd: &std::path::Path,
    ) {
        if id == OPEN_RESULT {
            return self.explorer_menu_open_result(engine, surface_id, paths);
        }
        if id != SHOW_IN_FOLDER {
            return self.explorer_menu_other(engine, surface_id, id, paths, cwd);
        }
        let Some((folder, item)) = paths.first().and_then(|p| {
            self.state
                .explorer_views
                .get(surface_id)
                .and_then(|v| v.enclosing_target(p))
        }) else {
            return;
        };
        let Some(target) =
            crate::runtime::surface_binding::SurfaceBinding::capture(engine, surface_id)
        else {
            return;
        };
        // 툴바·주소창 이동과 같은 경로라 히스토리(뒤로)에 남는다.
        self.state.dispatch_intent(
            crate::intent::Intent::Engine(crate::app::engine_action::EngineAction::Explorer {
                target,
                action: crate::explorer_ui::ExplorerAction::Navigate(folder.clone()),
            })
            .from_user_context_menu(),
        );
        if let Some(view) = self.state.explorer_views.get_mut(surface_id) {
            view.reveal_after_load(folder, item);
        }
    }

    /// 검색 결과 하나를 연다.
    fn explorer_menu_open_result(
        &mut self,
        engine: &EngineRead<'_>,
        surface_id: u32,
        paths: &[std::path::PathBuf],
    ) {
        if let [path] = paths {
            open_result(&mut self.state, engine, surface_id, path);
        }
    }
}

/// 더블클릭과 같은 액션을 같은 호스트 경로로 보낸다. 폴더는 그 폴더로 들어가고 파일은 연다.
/// 파일 열기는 엔진이 아니라 호스트가 처리하므로 엔진 액션으로 직접 보내면 아무 일도 일어나지 않는다.
fn open_result(
    state: &mut crate::state::MainViewState,
    engine: &EngineRead<'_>,
    surface_id: u32,
    path: &std::path::Path,
) {
    let action = if path.is_dir() {
        crate::explorer_ui::ExplorerAction::Navigate(path.to_path_buf())
    } else {
        crate::explorer_ui::ExplorerAction::OpenFile(path.to_path_buf())
    };
    crate::adapters::ui::apply_explorer_action(state, engine, surface_id, action);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(items: &[MenuItem]) -> Vec<Option<u32>> {
        items
            .iter()
            .map(|i| (!i.is_separator()).then_some(i.id))
            .collect()
    }

    #[test]
    fn a_search_result_opens_first_then_shows_its_folder() {
        assert_eq!(
            ids(&enclosing_head(true, true)),
            vec![Some(OPEN_RESULT), Some(SHOW_IN_FOLDER), None]
        );
    }

    #[test]
    fn a_link_only_gets_the_folder_row() {
        assert_eq!(
            ids(&enclosing_head(false, true)),
            vec![Some(SHOW_IN_FOLDER), None]
        );
    }

    /// 파일 결과의 열기는 더블클릭처럼 파일 dispatch가 되고, 폴더 결과는 그 폴더로 들어간다.
    #[test]
    fn opening_a_result_dispatches_a_file_or_enters_a_folder() {
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let engine = engine_session.borrow_mut();
        let sid = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().to_path_buf();
        let file = dir.join("note.md");
        std::fs::write(&file, "x").expect("write the file");

        open_result(&mut state, &engine.read(), sid, &file);
        let intents = state.take_pending_intents();
        assert_eq!(intents.len(), 1);
        let crate::intent::Intent::Domain(crate::app::command::DomainIntent::DispatchFile {
            target,
            origin_surface_id,
            ..
        }) = &intents[0].body
        else {
            panic!("expected DispatchFile, got {:?}", intents[0].body);
        };
        assert_eq!(target.as_path(), file.as_path());
        assert_eq!(*origin_surface_id, Some(sid));

        open_result(&mut state, &engine.read(), sid, &dir);
        let intents = state.take_pending_intents();
        assert_eq!(intents.len(), 1);
        let crate::intent::Intent::Engine(crate::app::engine_action::EngineAction::Explorer {
            action: crate::explorer_ui::ExplorerAction::Navigate(to),
            ..
        }) = &intents[0].body
        else {
            panic!("expected Navigate, got {:?}", intents[0].body);
        };
        assert_eq!(to, &dir);
    }

    #[test]
    fn no_enclosing_folder_adds_nothing() {
        assert!(enclosing_head(true, false).is_empty());
        assert!(enclosing_head(false, false).is_empty());
    }
}
