//! Cross-platform native context menu.
//!
//! Uses OS-native menus (NSMenu on macOS, Win32 TrackPopupMenu on Windows,
//! GTK Menu on Linux) so they render above native child views (WebView).
//!
//! `show_context_menu` returns a [`MenuOutcome`] on every platform — the API
//! *shape* is unified, but when the outcome resolves is not. macOS / Windows
//! track the popup inside the run loop / message pump the main window already
//! owns, so they always answer `Ready` before returning. Linux drives a GTK
//! event loop of its own that must not be spun from inside winit's callback,
//! so it answers `Pending` and the caller pumps the returned handle once per
//! frame. Rationale: `docs/dev-guide/context-menu.md#네이티브-메뉴-api-cratestasty-platformsrcnative_menu`.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::show_context_menu;
#[cfg(target_os = "macos")]
pub use macos::show_context_menu;
#[cfg(windows)]
pub use windows::show_context_menu;

#[cfg(target_os = "linux")]
pub use linux::warn_if_menu_anchor_scale_premise_broken;

/// 이 배율 비교는 Linux/GDK용이다. 다른 백엔드는 별도 비교를 수행하지 않는다.
#[cfg(not(target_os = "linux"))]
pub fn warn_if_menu_anchor_scale_premise_broken(_winit_scale: f64) {}

/// Result of asking the platform to show a context menu.
pub enum MenuOutcome {
    /// The menu already ran to completion — selected item id, or `None` when
    /// dismissed (or when the menu could not be shown at all).
    Ready(Option<u32>),
    /// The menu is on screen and resolves later. The caller must keep the
    /// handle, [`MenuHandle::poll`] it once per frame, and act on the result
    /// when polling yields one.
    ///
    /// Linux 백엔드만 이 variant 를 만든다. 다른 플랫폼 빌드에서는 생성처가
    /// 없지만, 호출자(`view/main/redraw.rs`)가 플랫폼 분기 없이 하나의 match
    /// 로 처리하도록 타입에는 남겨 둔다.
    // 이유: 이 variant 를 만드는 것이 Linux 백엔드뿐이다(위) — 타입에 남겨 호출부 match 를 한 벌로 둔다.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Pending(MenuHandle),
}

/// 아직 결과를 받지 않은 메뉴 핸들. poll로 대기 중 이벤트를 처리하며 사용자 선택을 기다리지 않는다.
/// 플랫폼 이벤트 처리 자체의 실행 시간까지 제한하는 API는 아니다.
pub struct MenuHandle(HandleImpl);

enum HandleImpl {
    #[cfg(target_os = "linux")]
    Gtk(linux::GtkMenuHandle),
    /// 테스트 전용 시뮬레이션 — 디스플레이/GTK 없이 비동기 계약(폴링이 즉시
    /// 반환하고 N 프레임 뒤에 해소된다)을 헤드리스로 검증하기 위한 경로.
    #[cfg(test)]
    Simulated {
        remaining_polls: u32,
        result: Option<u32>,
    },
    /// 위 variant 가 모두 `cfg` 로 빠지는 빌드 조합(비-Linux release)에서도
    /// 타입이 성립하도록 두는 자리표시자 — 구성 불가능하다.
    #[allow(dead_code)]
    Never(std::convert::Infallible),
}

impl MenuHandle {
    #[cfg(target_os = "linux")]
    pub(super) fn from_gtk(handle: linux::GtkMenuHandle) -> Self {
        Self(HandleImpl::Gtk(handle))
    }

    /// 헤드리스 테스트용 시뮬레이션 핸들 — `polls` 회까지는 `None`(미완)을,
    /// 그 다음 폴링에서 `result` 를 돌려준다. 테스트 빌드 외 미노출.
    #[cfg(test)]
    pub fn debug_simulated(polls: u32, result: Option<u32>) -> Self {
        Self(HandleImpl::Simulated {
            remaining_polls: polls,
            result,
        })
    }

    /// Service the menu once. `None` = still open (keep rendering frames),
    /// `Some(result)` = the menu closed and this is its outcome.
    pub fn poll(&mut self) -> Option<Option<u32>> {
        match &mut self.0 {
            #[cfg(target_os = "linux")]
            HandleImpl::Gtk(h) => h.poll(),
            #[cfg(test)]
            HandleImpl::Simulated {
                remaining_polls,
                result,
            } => {
                if *remaining_polls > 0 {
                    *remaining_polls -= 1;
                    None
                } else {
                    Some(*result)
                }
            }
            HandleImpl::Never(_) => unreachable!("HandleImpl::Never is unconstructible"),
        }
    }

    /// Close the menu without a selection. The cancel still surfaces through
    /// the next [`MenuHandle::poll`], so completion keeps a single path.
    pub fn dismiss(&mut self) {
        match &mut self.0 {
            #[cfg(target_os = "linux")]
            HandleImpl::Gtk(h) => h.dismiss(),
            #[cfg(test)]
            HandleImpl::Simulated {
                remaining_polls,
                result,
            } => {
                *remaining_polls = 0;
                *result = None;
            }
            HandleImpl::Never(_) => unreachable!("HandleImpl::Never is unconstructible"),
        }
    }
}

/// A single item in a native context menu.
pub struct MenuItem {
    /// Unique identifier returned when this item is selected.
    pub id: u32,
    /// Display label.
    pub label: String,
    /// Whether this item is enabled (grayed out if false).
    pub enabled: bool,
}

impl MenuItem {
    pub fn new(id: u32, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            enabled: true,
        }
    }

    pub fn disabled(id: u32, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            enabled: false,
        }
    }

    pub fn separator() -> Self {
        Self {
            id: 0,
            label: String::new(),
            enabled: false,
        }
    }

    pub fn is_separator(&self) -> bool {
        self.label.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 모의 핸들의 poll이 대기하지 않고 결과를 한 번만 반환하는지 확인한다.
    #[test]
    fn pending_menu_polls_return_immediately_until_resolved() {
        let mut handle = MenuHandle::debug_simulated(5, Some(42));
        let start = std::time::Instant::now();
        for i in 0..5 {
            assert_eq!(
                handle.poll(),
                None,
                "poll #{i} must report 'still open' without blocking"
            );
        }
        assert_eq!(handle.poll(), Some(Some(42)));
        // 6 회 폴링이 프레임 예산(8ms/프레임)을 통째로 넘길 이유가 없다 —
        // 블로킹 대기 루프가 되살아나면 여기서 깨진다.
        assert!(
            start.elapsed() < std::time::Duration::from_millis(100),
            "polling a pending menu must not block (took {:?})",
            start.elapsed()
        );
    }

    /// 바깥 클릭 dismiss 도 별도 반환 경로를 만들지 않고 다음 poll 로 해소된다.
    #[test]
    fn dismiss_resolves_through_poll_as_cancel() {
        let mut handle = MenuHandle::debug_simulated(3, Some(7));
        assert_eq!(handle.poll(), None);
        handle.dismiss();
        assert_eq!(handle.poll(), Some(None));
    }

    #[test]
    fn ready_outcome_carries_selection_directly() {
        // 동기 해소 플랫폼(macOS/Windows)이 쓰는 경로 — 핸들 없이 값만 온다.
        match MenuOutcome::Ready(Some(3)) {
            MenuOutcome::Ready(v) => assert_eq!(v, Some(3)),
            MenuOutcome::Pending(_) => panic!("Ready must not carry a handle"),
        }
    }
}
