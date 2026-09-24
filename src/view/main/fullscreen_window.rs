//! 전체화면 무대 상태를 같은 OS 창에 반영한다. AppState는 창 핸들이 없어
//! 매 프레임 이 모듈에서 전환하고, 무대가 만든 전환만 원래 상태로 되돌린다.
//! docs/design/systems/fullscreen-stage.md 참고.

use winit::window::{Fullscreen, Window};

use super::MainView;

/// 무대 진입 **직전**의 창 상태. 종료 시 정확히 이 상태로 되돌린다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SavedWindowMode {
    /// 진입 시점에 이미 OS fullscreen 이었는가. macOS 신호등의 풀스크린 버튼처럼
    /// **사용자가 직접 만든** 창 상태가 여기 해당한다.
    pub(crate) was_fullscreen: bool,
    /// 진입 시점에 maximize 였는가.
    pub(crate) was_maximized: bool,
}

/// 무대 종료 시 창에 적용할 동작.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowRestore {
    /// 창을 건드리지 않는다.
    Keep,
    /// fullscreen 을 풀고 `maximized` 상태로 되돌린다.
    Exit { maximized: bool },
}

/// 원래 전체화면이었으면 그대로 두고, 무대 때문에 전환한 경우만 복원한다.
pub(crate) fn restore_for(saved: SavedWindowMode) -> WindowRestore {
    if saved.was_fullscreen {
        WindowRestore::Keep
    } else {
        WindowRestore::Exit {
            maximized: saved.was_maximized,
        }
    }
}

/// 해상도를 바꾸지 않는 Borderless로 현재 모니터를 덮는다.
/// current_monitor가 없으면 백엔드의 기본 모니터 판정을 사용한다.
fn borderless_for(window: &Window) -> Fullscreen {
    Fullscreen::Borderless(window.current_monitor())
}

/// 최대화·전체화면에서는 가장자리 리사이즈를 시작하지 않는다.
// 이유: macOS는 네이티브 리사이즈를 사용해 이 함수의 호출부가 없다.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(crate) fn window_size_is_locked(window: &Window) -> bool {
    size_is_locked(window.is_maximized(), window.fullscreen().is_some())
}

/// 실제 창 없이도 확인할 수 있는 크기 고정 판정.
// 이유: 유일한 호출자가 위 `window_size_is_locked` 라 그것이 죽는 macOS 에서 함께 죽는다.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn size_is_locked(maximized: bool, os_fullscreen: bool) -> bool {
    maximized || os_fullscreen
}

/// 창이 덮고 있는 모니터의 신원. 멀티 모니터에서 "그 창이 있던 모니터를 덮었는가" 를
/// 출력만 보고 판정할 수 있게 하는 값이다.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MonitorReport {
    pub(crate) name: Option<String>,
    pub(crate) position: (i32, i32),
    pub(crate) size: (u32, u32),
    pub(crate) scale_factor: f64,
}

/// 창의 전체화면 관련 상태 덤프(읽기 전용).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FullscreenWindowReport {
    pub(crate) stage_active: bool,
    pub(crate) os_fullscreen: bool,
    pub(crate) maximized: bool,
    pub(crate) inner_size: (u32, u32),
    pub(crate) monitor: Option<MonitorReport>,
}

impl MainView {
    /// 무대 상태(`fullscreen_stage_active`)에 OS 창 fullscreen 을 맞춘다.
    ///
    /// `stage_saved_window_mode` 가 "무대 때문에 전환해 둔 상태" 의 마커를 겸한다 —
    /// `Some` 이면 이 창은 무대가 전환한 것이고, 그 안에 되돌릴 원래 상태가 있다.
    pub(super) fn sync_window_fullscreen(&mut self) {
        let want = self.state.fullscreen_stage_active();
        match (want, self.stage_saved_window_mode) {
            (true, None) => {
                let saved = SavedWindowMode {
                    was_fullscreen: self.base.winit.fullscreen().is_some(),
                    was_maximized: self.base.winit.is_maximized(),
                };
                // 같은 fullscreen 전환을 다시 요청해 macOS Space 이동을 반복하지 않는다.
                if !saved.was_fullscreen {
                    let target = borderless_for(&self.base.winit);
                    self.base.winit.set_fullscreen(Some(target));
                }
                self.stage_saved_window_mode = Some(saved);
            }
            (false, Some(saved)) => {
                match restore_for(saved) {
                    WindowRestore::Keep => {}
                    WindowRestore::Exit { maximized } => {
                        self.base.winit.set_fullscreen(None);
                        // fullscreen 해제만으로 최대화가 복원되지 않는 플랫폼에도 원래 값을 적용한다.
                        self.base.winit.set_maximized(maximized);
                    }
                }
                self.stage_saved_window_mode = None;
            }
            _ => {}
        }
    }

    /// debug.fullscreen.state가 읽는 창 상태와 현재 모니터 정보.
    // 이유: 그 IPC 가 debug 전용이라 release 빌드에는 호출부가 없다.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub(crate) fn fullscreen_window_report(&self) -> FullscreenWindowReport {
        let size = self.base.winit.inner_size();
        FullscreenWindowReport {
            stage_active: self.state.fullscreen_stage_active(),
            os_fullscreen: self.base.winit.fullscreen().is_some(),
            maximized: self.base.winit.is_maximized(),
            inner_size: (size.width, size.height),
            monitor: self.base.winit.current_monitor().map(|m| {
                let pos = m.position();
                let msize = m.size();
                MonitorReport {
                    name: m.name(),
                    position: (pos.x, pos.y),
                    size: (msize.width, msize.height),
                    scale_factor: m.scale_factor(),
                }
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 평범한 창에서 진입했으면 종료 시 fullscreen 을 풀고 일반 창으로 돌아간다.
    #[test]
    fn 일반_창은_종료_시_fullscreen_을_푼다() {
        let saved = SavedWindowMode {
            was_fullscreen: false,
            was_maximized: false,
        };
        assert_eq!(restore_for(saved), WindowRestore::Exit { maximized: false });
    }

    /// maximize 상태에서 진입했으면 maximize 로 복귀한다 — 일반 창으로 떨어뜨리지
    /// 않는다.
    #[test]
    fn maximize_였으면_maximize_로_복귀한다() {
        let saved = SavedWindowMode {
            was_fullscreen: false,
            was_maximized: true,
        };
        assert_eq!(restore_for(saved), WindowRestore::Exit { maximized: true });
    }

    #[test]
    fn fullscreen_단독으로도_리사이즈가_잠긴다() {
        assert!(!size_is_locked(false, false));
        assert!(size_is_locked(true, false));
        assert!(size_is_locked(false, true));
        assert!(size_is_locked(true, true));
    }

    /// 사용자가 이미 전체화면으로 만든 창은 무대 종료 뒤에도 유지한다.
    #[test]
    fn 진입_전부터_fullscreen_이면_종료_시_유지한다() {
        for was_maximized in [false, true] {
            let saved = SavedWindowMode {
                was_fullscreen: true,
                was_maximized,
            };
            assert_eq!(restore_for(saved), WindowRestore::Keep);
        }
    }
}
