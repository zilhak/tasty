//! Explorer 메뉴와 단축키가 공유하는 판정. MainView 없이 시험할 수 있게 상태 쪽에 둔다.

use crate::runtime::engine_read::EngineRead;
use crate::runtime::surface_binding::SurfaceBinding;

/// 메뉴를 연 뒤 고른 항목을 실행해도 되는가.
/// 파일시스템을 바꾸지 않는 항목(`read_only`)은 같은 explorer surface 이기만 하면 된다.
/// 쓰기 항목은 mirror projection 세대까지 같아야 한다.
pub(crate) fn explorer_menu_admits(
    binding: Option<&SurfaceBinding>,
    engine: &EngineRead<'_>,
    read_only: bool,
) -> bool {
    binding.is_some_and(|binding| {
        if read_only {
            binding.same_surface_in(engine)
        } else {
            binding.current_in(engine)
        }
    })
}

/// 단축키가 있는 explorer 메뉴 항목(이름 변경 40·휴지통 30)에 첫 바인딩을 표시로 붙인다.
/// 바인딩이 없으면 표시하지 않는다.
pub(crate) fn attach_shortcut_hints(
    items: &mut [crate::platform::native_menu::MenuItem],
    settings: &crate::settings::Settings,
) {
    for item in items {
        let field = match item.id {
            40 => "explorer_rename",
            30 => "explorer_trash",
            _ => continue,
        };
        item.shortcut = menu_shortcut(settings, field);
    }
}

fn menu_shortcut(
    settings: &crate::settings::Settings,
    field: &str,
) -> Option<crate::platform::native_menu::MenuShortcut> {
    let binding = settings.keybindings.get_bindings(field)?.first()?;
    Some(crate::platform::native_menu::MenuShortcut {
        binding: binding.clone(),
        display: menu_display(binding, &settings.general),
    })
}

/// 메뉴 단축키 열의 표시. 키 이름은 설정 화면과 같고, 앞 지우기 키만 메뉴 관례대로 `Del` 로 줄인다.
fn menu_display(binding: &str, general: &crate::settings::GeneralSettings) -> String {
    let mut parts = tasty_settings::KeybindingSettings::format_display_parts(binding, general);
    if let Some(key) = parts.last_mut()
        && key.eq_ignore_ascii_case("delete")
    {
        *key = "Del".to_string();
    }
    parts.join("+")
}

impl super::MainViewState {
    /// 클립보드의 경로를 `destination` 에 붙여넣도록 요청한다.
    /// mirror surface 와 원격에서 복사한 클립보드는 로컬 파일 작업으로 바꾸지 않는다.
    pub(crate) fn explorer_paste(
        &mut self,
        engine: &EngineRead<'_>,
        surface_id: u32,
        destination: std::path::PathBuf,
        origin: crate::intent::IntentOrigin,
    ) {
        // 표시된 경로는 원격 호스트의 경로다(ADR-0022). 같은 문자열의 로컬 경로를 바꾸지 않는다.
        if engine.is_mirror_surface(surface_id) {
            self.toasts.push(
                crate::i18n::t("explorer.state.remote_write_unsupported").to_string(),
                crate::adapters::ui::ToastKind::Info,
                crate::adapters::ui::ToastScope::Window,
            );
            return;
        }
        let Some(clip) = self.explorer_clipboard.clone() else {
            return;
        };
        // 원격 경로를 같은 문자열의 로컬 파일로 복사하지 않는다.
        if !clip.is_local() {
            self.toasts.push(
                crate::i18n::t("explorer.state.remote_paste_unsupported").to_string(),
                crate::adapters::ui::ToastKind::Info,
                crate::adapters::ui::ToastScope::Window,
            );
            return;
        }
        self.request_explorer_file(
            engine,
            surface_id,
            crate::app::explorer_files::Operation::Paste {
                paths: clip.paths,
                destination,
                cut: clip.cut,
            },
            origin,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ExplorerClipboard, ExplorerPathSource};

    fn clipboard(source: ExplorerPathSource) -> ExplorerClipboard {
        ExplorerClipboard {
            identity: Default::default(),
            paths: vec![crate::test_support::abs_path("remote/proj/a.txt")],
            cut: false,
            source,
        }
    }

    fn user() -> crate::intent::IntentOrigin {
        crate::intent::IntentOrigin::User {
            source: crate::intent::UserSource::ContextMenu,
        }
    }

    #[test]
    fn a_clipboard_copied_from_a_remote_explorer_is_not_pasted_locally() {
        let (mut state, session, sid, root) = crate::state::tests::explorer_fixture(false);
        state.explorer_clipboard = Some(clipboard(ExplorerPathSource::Remote { workspace: 9 }));
        state.explorer_paste(&session.read(), sid, root.clone(), user());
        assert_eq!(state.explorer_file_requests.len(), 0);
        assert_eq!(
            state.toasts.messages(),
            vec![crate::i18n::t("explorer.state.remote_paste_unsupported")]
        );

        state.explorer_clipboard = Some(clipboard(ExplorerPathSource::Local));
        state.explorer_paste(&session.read(), sid, root, user());
        assert_eq!(
            state.explorer_file_requests.len(),
            1,
            "a local clipboard is pasted"
        );
    }

    #[test]
    fn a_menu_choice_runs_only_on_the_surface_generation_it_was_opened_on() {
        // 자원 세대를 가진 surface 로 교체를 흉내 낸다. 판정 규칙은 kind 와 무관하다.
        let (_state, mut session) = crate::state::tests::test_state();
        let sid = session.read().workspace_at(0).unwrap().all_surface_ids()[0];
        let binding = SurfaceBinding::capture(&session.read(), sid);
        assert!(explorer_menu_admits(
            binding.as_ref(),
            &session.read(),
            false
        ));
        assert!(explorer_menu_admits(
            binding.as_ref(),
            &session.read(),
            true
        ));
        assert!(!explorer_menu_admits(None, &session.read(), true));

        // 같은 id 에 다른 자원이 들어오면 세대가 달라진다.
        session.borrow_mut().runtime.terminals.insert(
            sid,
            tasty_terminal::Terminal::new_detached(80, 24),
            None,
        );
        assert!(!explorer_menu_admits(
            binding.as_ref(),
            &session.read(),
            false
        ));
        assert!(!explorer_menu_admits(
            binding.as_ref(),
            &session.read(),
            true
        ));
    }

    #[test]
    fn read_only_choices_survive_a_rebuilt_mirror_projection_but_writes_do_not() {
        let (_state, mut session, sid, _root) = crate::state::tests::explorer_fixture(true);
        let binding = SurfaceBinding::capture(&session.read(), sid);
        assert!(explorer_menu_admits(
            binding.as_ref(),
            &session.read(),
            false
        ));

        // 원격 트리를 다시 받으면 projection 토큰만 새로 만든다. surface 는 그대로다.
        let pane = crate::model::Pane::new_with_surface(
            sid,
            sid,
            "Explorer".into(),
            crate::model::SurfaceDescriptor::new(sid, "explorer"),
        );
        let mut workspace = crate::model::Workspace::new_with_pane(sid, "Explorer".into(), pane);
        workspace.mirror = true;
        assert!(
            session
                .core_state
                .replace_mirror_workspace(workspace)
                .is_ok()
        );
        assert!(explorer_menu_admits(
            binding.as_ref(),
            &session.read(),
            true
        ));
        assert!(!explorer_menu_admits(
            binding.as_ref(),
            &session.read(),
            false
        ));
    }
}
