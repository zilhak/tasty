//! explorer 컨텍스트 메뉴의 "들어 있는 폴더에서 보기" 행. 하위 폴더 검색 결과나 링크 하나를 눌렀을 때
//! 메뉴 맨 앞에 둔다. 고르면 항목이 실제로 든 폴더로 옮기고 목록이 들어오면 그 항목을 고른다.
//! 컨텍스트 메뉴의 생성 행처럼 메뉴 구성(`redraw.rs`)과 따로 둔다.

use super::MainView;
use crate::platform::native_menu::MenuItem;
use crate::runtime::engine_read::EngineRead;

const SHOW_IN_FOLDER: u32 = 90;

impl MainView {
    /// 대상이 하나이고 들어 있는 폴더가 있으면 메뉴 맨 앞에 행과 구분선을 둔다.
    pub(super) fn with_show_in_folder(
        &self,
        surface_id: u32,
        paths: &[std::path::PathBuf],
        mut items: Vec<MenuItem>,
    ) -> Vec<MenuItem> {
        let has_target = matches!(paths, [path] if self
            .state
            .explorer_views
            .get(surface_id)
            .is_some_and(|v| !v.is_remote() && v.enclosing_target(path).is_some()));
        if has_target {
            items.splice(
                0..0,
                [
                    MenuItem::new(
                        SHOW_IN_FOLDER,
                        crate::i18n::t("explorer.menu.show_in_folder"),
                    ),
                    MenuItem::separator(),
                ],
            );
        }
        items
    }

    /// 붙여넣을 로컬 클립보드가 있는가. 원격에서 복사한 경로는 로컬에 붙여넣을 수 없다.
    pub(super) fn explorer_menu_has_clip(&self) -> bool {
        self.state
            .explorer_clipboard
            .as_ref()
            .is_some_and(|c| !c.paths.is_empty() && c.is_local())
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
}
