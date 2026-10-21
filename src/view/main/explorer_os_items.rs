//! explorer 컨텍스트 메뉴의 OS 연계 행. "시스템에서 열기"는 고른 항목을 모두 OS 기본 앱(폴더는
//! 파일 관리자)으로 열고, "파일 관리자에서 보기"는 한 항목을 고른 채 담은 폴더를 연다.
//! 생성 행처럼 메뉴 구성(`redraw.rs`)과 따로 둔다. 두 행 모두 로컬 칸에서만 쓴다.

use super::MainView;
use crate::platform::native_menu::MenuItem;
use crate::runtime::engine_read::EngineRead;

const OPEN_IN_SYSTEM: u32 = 20;
const REVEAL: u32 = 21;

/// "파일 관리자에서 보기" 행 이름의 번역 키. 각 OS 가 자기 파일 관리자를 부르는 이름을 쓴다.
fn reveal_label_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "explorer.context_menu.reveal_finder"
    } else if cfg!(windows) {
        "explorer.context_menu.reveal_explorer"
    } else {
        "explorer.context_menu.reveal_files"
    }
}

impl MainView {
    /// 휴지통 뒤에 붙는 OS 연계 행. 파일 관리자에서 보기는 한 항목일 때만 둔다.
    pub(super) fn push_explorer_os_items(items: &mut Vec<MenuItem>, multi: bool) {
        items.push(MenuItem::new(
            OPEN_IN_SYSTEM,
            crate::i18n::t("explorer.context_menu.open_in_system"),
        ));
        if !multi {
            items.push(MenuItem::new(REVEAL, crate::i18n::t(reveal_label_key())));
        }
    }

    /// 파일 관리자에서 보기 (아이템 21).
    pub(super) fn explorer_menu_reveal(
        &mut self,
        engine: &EngineRead<'_>,
        surface_id: u32,
        paths: &[std::path::PathBuf],
    ) {
        if engine.is_mirror_surface(surface_id) {
            self.toast_remote_write_unsupported();
            return;
        }
        if let Some(target) = paths.first().cloned() {
            self.state.request_explorer_file(
                engine,
                surface_id,
                crate::app::explorer_files::Operation::Reveal(target),
                crate::intent::IntentOrigin::User {
                    source: crate::intent::UserSource::ContextMenu,
                },
            );
        }
    }
}
