//! explorer 의 새 폴더·새 파일 명령. 컨텍스트 메뉴 행, 좁은 칸의 More 메뉴, 단축키가 같은 시작점을 쓴다.
//! More 메뉴는 툴바 view 묶음의 Find 도 함께 보인다.
//! 명령은 이름 입력을 열기만 한다. 실제 생성은 이름을 확정한 뒤 `ExplorerAction::Create` 로 한다.

use super::MainView;
use crate::platform::native_menu::MenuItem;
use crate::runtime::engine_read::EngineRead;

const NEW_FOLDER: u32 = 80;
const NEW_FILE: u32 = 81;
const FIND: u32 = 82;

impl MainView {
    /// 생성 행 두 개. 빈 영역 메뉴는 맨 앞 묶음이라 뒤에, 폴더 메뉴는 파일 조작 묶음이라 앞에 구분선을 둔다.
    pub(super) fn push_explorer_create_items(items: &mut Vec<MenuItem>, leading: bool) {
        if !leading {
            items.push(MenuItem::separator());
        }
        items.push(MenuItem::new(
            NEW_FOLDER,
            crate::i18n::t("explorer.command.new_folder"),
        ));
        items.push(MenuItem::new(
            NEW_FILE,
            crate::i18n::t("explorer.command.new_file"),
        ));
        if leading {
            items.push(MenuItem::separator());
        }
    }

    /// 컨텍스트 메뉴의 생성 행. 폴더 행이면 그 폴더 안에, 빈 영역이면 현재 폴더에 만든다.
    pub(super) fn explorer_menu_create(
        &mut self,
        engine: &EngineRead<'_>,
        surface_id: u32,
        id: u32,
        paths: &[std::path::PathBuf],
        cwd: &std::path::Path,
    ) {
        let folder = match id {
            NEW_FOLDER => true,
            NEW_FILE => false,
            _ => return,
        };
        let dir = paths.first().cloned().unwrap_or_else(|| cwd.to_path_buf());
        self.state
            .start_explorer_create(engine, surface_id, Some(dir), folder);
    }

    /// 좁은 칸의 More 메뉴. 툴바에서 접은 명령을 같은 순서로 보인다.
    pub(super) fn handle_explorer_more_native_menu(
        &mut self,
        engine: &EngineRead<'_>,
        surface_id: u32,
        x: f32,
        y: f32,
    ) {
        let mut items = Vec::new();
        if !engine.is_mirror_surface(surface_id) {
            let writable = self
                .state
                .explorer_views
                .get(surface_id)
                .is_none_or(|v| v.can_write_here());
            for (id, key) in [
                (NEW_FOLDER, "explorer.command.new_folder"),
                (NEW_FILE, "explorer.command.new_file"),
            ] {
                let label = crate::i18n::t(key);
                items.push(if writable {
                    MenuItem::new(id, label)
                } else {
                    MenuItem::disabled(id, label)
                });
            }
        }
        if !items.is_empty() {
            items.push(MenuItem::separator());
        }
        items.push(MenuItem::new(FIND, crate::i18n::t("explorer.command.find")));
        self.open_native_menu(
            engine,
            x,
            y,
            &items,
            move |this, engine, result| match result {
                Some(id @ (NEW_FOLDER | NEW_FILE)) => {
                    this.state
                        .start_explorer_create(engine, surface_id, None, id == NEW_FOLDER);
                }
                Some(FIND) => {
                    if let Some(view) = this.state.explorer_views.get_mut(surface_id) {
                        view.toggle_find();
                    }
                }
                _ => {}
            },
        );
    }
}

impl crate::state::MainViewState {
    /// 탐색기의 Find 바를 열고 입력에 포커스를 준다.
    pub(crate) fn open_explorer_find(&mut self, surface_id: u32) {
        if let Some(view) = self.explorer_views.get_mut(surface_id) {
            view.open_find();
        }
    }

    /// 이름 입력을 연다. `dir` 이 없으면 지금 보는 폴더다.
    /// 원격 explorer 와 쓸 수 없다고 확인한 폴더는 입력을 열지 않고 이유를 알린다.
    pub(crate) fn start_explorer_create(
        &mut self,
        engine: &EngineRead<'_>,
        surface_id: u32,
        dir: Option<std::path::PathBuf>,
        folder: bool,
    ) {
        if !engine.has_surface(surface_id) {
            return;
        }
        if engine.is_mirror_surface(surface_id) {
            self.toasts.push(
                crate::i18n::t("explorer.state.remote_write_unsupported").to_string(),
                crate::adapters::ui::ToastKind::Info,
                crate::adapters::ui::ToastScope::Window,
            );
            return;
        }
        let Some(view) = self.explorer_views.get_mut(surface_id) else {
            return;
        };
        let Some(dir) = dir.or_else(|| view.shown_dir().map(std::path::Path::to_path_buf)) else {
            return;
        };
        if view.shown_dir() == Some(dir.as_path()) && !view.can_write_here() {
            self.toasts.push(
                crate::i18n::t("explorer.command.cannot_write").to_string(),
                crate::adapters::ui::ToastKind::Info,
                crate::adapters::ui::ToastScope::Surface(surface_id),
            );
            return;
        }
        view.start_create(dir, folder);
    }
}
