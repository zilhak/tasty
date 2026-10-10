//! Explorer 클립보드와 OS 파일 클립보드를 잇는다.
//!
//! 로컬 경로를 복사·잘라내면 OS 클립보드에도 올리고, 올린 직후 OS 클립보드를 한 번 읽어 둔다.
//! 붙여넣을 때 OS 클립보드가 그때와 같으면 창의 목록을, 달라졌으면 OS 쪽을 쓴다 — 나중에 한 복사가
//! 이긴다. 다른 앱이 글자 등으로 바꿨으면 오래된 목록은 버린다. 원격에서 복사한 경로는 OS 에 올리지
//! 않는다(같은 문자열의 로컬 파일로 읽히지 않도록).

use std::path::PathBuf;

use crate::clipboard::files::{OsClip, OsFiles};

use super::{ExplorerClipboard, ExplorerPathSource, MainViewState};

/// 붙여넣기가 쓸 목록.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PasteFrom {
    /// 창의 Explorer 목록(원격 출처일 수 있다).
    Window,
    /// OS 클립보드의 로컬 파일.
    Os(OsFiles),
    /// OS 클립보드에 로컬 파일이 아닌 항목이 있다.
    NotLocal,
    /// 다른 프로그램이 나중에 클립보드를 가졌지만 내용을 읽지 못했다.
    Unreadable,
    /// 붙여넣을 것이 없다.
    Nothing,
}

/// 창의 목록과 OS 클립보드를 읽은 결과로 붙여넣을 목록을 정한다.
pub(crate) fn paste_from(
    window: Option<&ExplorerClipboard>,
    os: Result<OsClip, String>,
) -> PasteFrom {
    let os = match os {
        Ok(os) => os,
        // OS 클립보드에 접근하지 못하면 창의 목록만 쓴다.
        Err(_) => {
            return if window.is_some() {
                PasteFrom::Window
            } else {
                PasteFrom::Nothing
            };
        }
    };
    if let Some(clip) = window {
        match &clip.os_seen {
            // 복사한 뒤 OS 클립보드가 그대로다.
            Some(seen) if *seen == os => return PasteFrom::Window,
            // 복사할 때 OS 클립보드를 읽지 못했다. 바뀌었는지 알 수 없으므로 창의 목록을 쓴다.
            // 다만 응답하지 않는 다른 소유자가 있다면 그쪽이 나중에 복사한 것이다.
            None if !matches!(os, OsClip::Unreadable { .. }) => return PasteFrom::Window,
            _ => {}
        }
    }
    match os {
        OsClip::Files(files) => PasteFrom::Os(files),
        OsClip::NotLocal => PasteFrom::NotLocal,
        OsClip::Unreadable { .. } => PasteFrom::Unreadable,
        OsClip::Other => PasteFrom::Nothing,
    }
}

impl MainViewState {
    /// Explorer 의 복사·잘라내기 목록을 정한다. 로컬 경로는 OS 파일 클립보드에도 올린다.
    pub(crate) fn explorer_set_clipboard(
        &mut self,
        paths: Vec<PathBuf>,
        cut: bool,
        source: ExplorerPathSource,
    ) {
        if source == ExplorerPathSource::Local {
            let files = OsFiles {
                paths: paths.clone(),
                cut,
            };
            if let Err(e) = self.file_clipboard.write(&files) {
                tracing::warn!("file clipboard write failed: {e}");
            }
        }
        let os_seen = match self.file_clipboard.read() {
            Ok(clip) => Some(clip),
            Err(e) => {
                tracing::debug!("file clipboard read after copy failed: {e}");
                None
            }
        };
        self.explorer_clipboard = Some(ExplorerClipboard {
            identity: Default::default(),
            paths,
            cut,
            source,
            os_seen,
        });
    }

    /// 붙여넣기 전에 창의 목록을 OS 클립보드와 맞춘다. 붙여넣을 것이 없으면 false.
    /// OS 쪽이 더 새로우면 그 목록을 창의 목록으로 삼아, 잘라내기 소비가 같은 길을 탄다.
    pub(crate) fn explorer_take_os_clipboard(&mut self) -> bool {
        let os = self.file_clipboard.read();
        match paste_from(self.explorer_clipboard.as_ref(), os) {
            PasteFrom::Window => true,
            PasteFrom::Os(files) => {
                self.explorer_clipboard = Some(ExplorerClipboard {
                    identity: Default::default(),
                    paths: files.paths.clone(),
                    cut: files.cut,
                    source: ExplorerPathSource::Local,
                    os_seen: Some(OsClip::Files(files)),
                });
                true
            }
            PasteFrom::NotLocal => {
                self.explorer_refuse_os_paste("explorer.state.os_paste_not_local")
            }
            PasteFrom::Unreadable => {
                self.explorer_refuse_os_paste("explorer.state.os_paste_unreadable")
            }
            PasteFrom::Nothing => {
                self.explorer_clipboard = None;
                false
            }
        }
    }

    /// OS 클립보드가 나중 것이지만 붙여넣을 수 없다. 오래된 목록을 버리고 이유를 알린다.
    fn explorer_refuse_os_paste(&mut self, key: &str) -> bool {
        self.explorer_clipboard = None;
        self.toasts.push(
            crate::i18n::t(key).to_string(),
            crate::adapters::ui::ToastKind::Info,
            crate::adapters::ui::ToastScope::Window,
        );
        false
    }

    /// 붙여넣기 메뉴를 켤 것인가. 창의 로컬 목록이나 OS 클립보드의 로컬 파일이 있으면 참이다.
    pub(crate) fn explorer_can_paste(&self) -> bool {
        match paste_from(self.explorer_clipboard.as_ref(), self.file_clipboard.read()) {
            PasteFrom::Window => self
                .explorer_clipboard
                .as_ref()
                .is_some_and(|c| !c.paths.is_empty() && c.is_local()),
            PasteFrom::Os(_) => true,
            PasteFrom::NotLocal | PasteFrom::Unreadable | PasteFrom::Nothing => false,
        }
    }

    /// 잘라낸 목록을 옮겼다. OS 클립보드가 아직 그 목록이면 비워 같은 파일을 다시 붙여넣지 못하게 한다.
    pub(crate) fn explorer_cut_consumed(&self, clip: &ExplorerClipboard) {
        if clip.is_local() {
            self.file_clipboard.clear_if_holding(&OsFiles {
                paths: clip.paths.clone(),
                cut: true,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(cut: bool) -> OsFiles {
        OsFiles {
            paths: vec![crate::test_support::abs_path("a/b.txt")],
            cut,
        }
    }

    fn window(os_seen: Option<OsClip>) -> ExplorerClipboard {
        ExplorerClipboard {
            identity: Default::default(),
            paths: vec![crate::test_support::abs_path("w/x.txt")],
            cut: true,
            source: ExplorerPathSource::Local,
            os_seen,
        }
    }

    #[test]
    fn the_later_copy_wins() {
        let ours = OsClip::Files(files(false));
        // OS 클립보드가 복사한 뒤 그대로면 창의 목록이다.
        assert_eq!(
            paste_from(Some(&window(Some(ours.clone()))), Ok(ours.clone())),
            PasteFrom::Window
        );
        // 다른 앱이 파일을 복사했으면 그쪽이다.
        let theirs = files(true);
        assert_eq!(
            paste_from(
                Some(&window(Some(ours.clone()))),
                Ok(OsClip::Files(theirs.clone()))
            ),
            PasteFrom::Os(theirs.clone())
        );
        assert_eq!(
            paste_from(None, Ok(OsClip::Files(theirs.clone()))),
            PasteFrom::Os(theirs)
        );
        // 다른 앱이 글자를 복사했으면 오래된 잘라내기 목록을 쓰지 않는다.
        assert_eq!(
            paste_from(Some(&window(Some(ours.clone()))), Ok(OsClip::Other)),
            PasteFrom::Nothing
        );
        assert_eq!(
            paste_from(Some(&window(Some(ours))), Ok(OsClip::NotLocal)),
            PasteFrom::NotLocal
        );
    }

    #[test]
    fn without_os_access_the_window_list_stays() {
        assert_eq!(
            paste_from(Some(&window(Some(OsClip::Other))), Err("no display".into())),
            PasteFrom::Window
        );
        assert_eq!(
            paste_from(None, Err("no display".into())),
            PasteFrom::Nothing
        );
        // 복사할 때 읽지 못했으면 바뀌었는지 모르므로 창의 목록이다.
        assert_eq!(
            paste_from(Some(&window(None)), Ok(OsClip::Files(files(false)))),
            PasteFrom::Window
        );
    }

    #[test]
    fn a_local_copy_reaches_the_os_and_a_remote_one_does_not() {
        let (mut state, _engine) = crate::state::tests::test_state();
        let (handle, memory) = crate::clipboard::files::Handle::memory();
        state.file_clipboard = handle;
        let paths = files(false).paths;
        state.explorer_set_clipboard(paths.clone(), true, ExplorerPathSource::Local);
        assert_eq!(
            memory.get(),
            OsClip::Files(OsFiles {
                paths: paths.clone(),
                cut: true
            })
        );
        assert!(state.explorer_can_paste());

        memory.set(OsClip::Other);
        state.explorer_set_clipboard(paths, false, ExplorerPathSource::Remote { workspace: 3 });
        // 원격 경로는 OS 에 올리지 않는다. 창의 목록도 로컬에 붙여넣을 수 없다.
        assert_eq!(memory.get(), OsClip::Other);
        assert!(!state.explorer_can_paste());
    }

    #[test]
    fn an_os_cut_becomes_the_window_list_and_moving_it_clears_the_os() {
        let (mut state, _engine) = crate::state::tests::test_state();
        let (handle, memory) = crate::clipboard::files::Handle::memory();
        state.file_clipboard = handle;
        memory.set(OsClip::Files(files(true)));
        assert!(state.explorer_can_paste());
        assert!(state.explorer_take_os_clipboard());
        let clip = state.explorer_clipboard.clone().expect("clipboard");
        assert_eq!((clip.paths.clone(), clip.cut), (files(true).paths, true));
        state.explorer_cut_consumed(&clip);
        assert_eq!(memory.get(), OsClip::Other);
    }

    #[test]
    fn a_text_copy_elsewhere_drops_the_stale_cut() {
        let (mut state, _engine) = crate::state::tests::test_state();
        let (handle, memory) = crate::clipboard::files::Handle::memory();
        state.file_clipboard = handle;
        state.explorer_set_clipboard(files(false).paths, true, ExplorerPathSource::Local);
        memory.set(OsClip::Other);
        assert!(!state.explorer_can_paste());
        assert!(!state.explorer_take_os_clipboard());
        assert!(state.explorer_clipboard.is_none());
    }

    #[test]
    fn a_non_local_os_list_is_refused_with_a_toast() {
        let (mut state, _engine) = crate::state::tests::test_state();
        let (handle, memory) = crate::clipboard::files::Handle::memory();
        state.file_clipboard = handle;
        state.explorer_set_clipboard(files(false).paths, true, ExplorerPathSource::Local);
        memory.set(OsClip::NotLocal);
        assert!(!state.explorer_can_paste());
        let before = state.toasts.len();
        assert!(!state.explorer_take_os_clipboard());
        assert_eq!(state.toasts.len(), before + 1);
        // 나중 복사가 이겼으므로 오래된 목록도 버린다.
        assert!(state.explorer_clipboard.is_none());
    }

    #[test]
    fn an_owner_that_does_not_answer_is_a_later_copy() {
        let hung = OsClip::Unreadable { owner: 7 };
        let ours = OsClip::Files(files(false));
        assert_eq!(
            paste_from(Some(&window(Some(ours))), Ok(hung.clone())),
            PasteFrom::Unreadable
        );
        // 복사할 때 읽지 못했어도 응답하지 않는 소유자는 나중 것이다.
        assert_eq!(
            paste_from(Some(&window(None)), Ok(hung.clone())),
            PasteFrom::Unreadable
        );
        // 복사 직후에도 같은 소유자가 응답하지 않았다면 바뀐 것이 없다.
        assert_eq!(
            paste_from(Some(&window(Some(hung.clone()))), Ok(hung.clone())),
            PasteFrom::Window
        );
        assert_eq!(paste_from(None, Ok(hung)), PasteFrom::Unreadable);

        let (mut state, _engine) = crate::state::tests::test_state();
        let (handle, memory) = crate::clipboard::files::Handle::memory();
        state.file_clipboard = handle;
        state.explorer_set_clipboard(files(false).paths, true, ExplorerPathSource::Local);
        memory.set(OsClip::Unreadable { owner: 9 });
        assert!(!state.explorer_can_paste());
        let before = state.toasts.len();
        assert!(!state.explorer_take_os_clipboard());
        assert_eq!(state.toasts.len(), before + 1);
        assert!(state.explorer_clipboard.is_none());
    }

    /// macOS pasteboard 처럼 잘라내기 표지를 버리는 클립보드에서도 옮긴 뒤 클립보드를 비운다.
    #[test]
    fn a_moved_cut_is_cleared_even_where_the_clipboard_drops_the_cut_mark() {
        let (mut state, _engine) = crate::state::tests::test_state();
        let (handle, memory) = crate::clipboard::files::Handle::memory();
        memory
            .drops_cut_mark
            .store(true, std::sync::atomic::Ordering::Relaxed);
        state.file_clipboard = handle;
        state.explorer_set_clipboard(files(false).paths, true, ExplorerPathSource::Local);
        assert_eq!(
            memory.get(),
            OsClip::Files(OsFiles {
                paths: files(false).paths,
                cut: false
            })
        );
        // 표지가 없어진 채로 읽혀도 Tasty 목록은 잘라내기다.
        assert!(state.explorer_take_os_clipboard());
        let clip = state.explorer_clipboard.clone().expect("clipboard");
        assert!(clip.cut);
        state.explorer_cut_consumed(&clip);
        assert_eq!(memory.get(), OsClip::Other);
    }
}
