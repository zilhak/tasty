//! 전체화면 무대의 **OS 창 전환** — 무대가 창을 덮을 때 창 자체가 모니터를 덮는다.
//!
//! 무대의 경계는 작업영역이 아니라 **OS 창까지**다. 브라우저 Fullscreen API 와 같은
//! 모델이다 — `requestFullscreen()` 은 새 창을 만들지 않고 **같은 창**을 OS
//! fullscreen 으로 전환한 뒤 크롬 UI 를 숨긴다. 무대도 새 `View`(별개 OS 창)를 만들지
//! 않고 이 `MainView` 의 winit 창을 전환한다.
//!
//! 무대 동작 모델 전체는 [`docs/design/systems/fullscreen-stage.md`].
//!
//! ## 왜 상태 머신이 아니라 **리컨실러**인가
//!
//! 무대 상태의 단일 수렴점은 `AppState::open_fullscreen_stage` /
//! `close_fullscreen_stage` 인데, `AppState` 는 headless 빌드에도 있고 winit 핸들을
//! 들고 있지 않다. 그래서 전환을 그 두 함수 안에 박는 대신, 그 둘이 만드는 상태
//! (`fullscreen_stage_active()`)를 **매 프레임 창에 반영**한다 — WebView 노출을
//! `has_egui_overlay_open()` 에 맞추는 `MainView::sync_webviews` 와 같은 관례다.
//!
//! 이 방향이 더 튼튼하기도 하다. 무대를 여닫는 경로가 앞으로 몇 개가 되든(단축키 ·
//! debug IPC · 무대 자신의 닫기 액션) 전환 호출을 각자 기억할 필요가 없고, 상태와
//! 창이 어긋나면 다음 프레임에 스스로 수렴한다.

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

/// 저장해 둔 진입 시점 상태로부터 종료 시 동작을 정한다.
///
/// **진입 전부터 fullscreen 이던 창은 해제하지 않는다.** 그건 사용자가 직접 만든
/// 창 상태이고, 무대가 그걸 훔치면 "무대를 한 번 열었다 닫았더니 내가 만든
/// 전체화면이 풀렸다" 가 된다. 무대는 자기가 만든 전환만 되돌린다.
pub(crate) fn restore_for(saved: SavedWindowMode) -> WindowRestore {
    if saved.was_fullscreen {
        WindowRestore::Keep
    } else {
        WindowRestore::Exit {
            maximized: saved.was_maximized,
        }
    }
}

/// 이 창을 덮을 fullscreen 모드.
///
/// **`Borderless` 를 쓴다.** `Exclusive(VideoMode)` 는 모니터 해상도 자체를 바꾸는
/// 게임용 모드라, 다른 창들의 배치를 흐트러뜨리고 복귀 시 원래 배치가 돌아오지 않는
/// 부작용이 있다. 터미널은 해상도를 바꿀 이유가 없다.
///
/// **모니터는 `current_monitor()` 로 명시 지정한다.** `Borderless(None)` 의 "현재
/// 모니터" 판정은 winit 이 플랫폼 백엔드에 위임하므로 DE/컴포지터마다 해석이 다를 수
/// 있다. 명시 지정이면 "이 창이 있는 그 모니터를 덮는다" 는 의도가 코드에 남는다.
/// `current_monitor()` 는 플랫폼·타이밍에 따라 `None` 을 돌려줄 수 있는데, 그 값이
/// 그대로 `Borderless(None)` = 백엔드 판정 폴백이 된다 — 별도 분기가 필요 없다.
fn borderless_for(window: &Window) -> Fullscreen {
    Fullscreen::Borderless(window.current_monitor())
}

/// 창이 지금 사용자 드래그로 크기를 바꿀 수 없는 상태인가 — maximize **또는** OS
/// fullscreen.
///
/// 리사이즈 엣지 hit-test 가 이 판정을 쓴다. `is_maximized()` 만 보면 fullscreen 인
/// 창의 가장자리에서 리사이즈 커서가 살아나고 드래그가 먹는다. 무대 중에는 입력
/// 게이트가 막아주지만, **무대 없이 fullscreen 인 창**(macOS 신호등)이 가능하므로
/// 게이트에 기대지 않고 여기서 함께 본다.
// 이유: macOS 는 네이티브 데코레이션이라 tasty 가 가장자리 리사이즈를 직접 다루지 않는다 —
// 호출부 두 곳이 모두 `#[cfg(not(target_os = "macos"))]` 안이라 그 타깃에서는 쓰이지 않는다
// (`resize_should_yield_to_content` 가 같은 이유로 같은 표기를 쓴다).
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(crate) fn window_size_is_locked(window: &Window) -> bool {
    size_is_locked(window.is_maximized(), window.fullscreen().is_some())
}

/// [`window_size_is_locked`] 의 판정만 떼어낸 것. `Window` 는 실제 winit 창 없이
/// 만들 수 없어 그대로는 단위 테스트가 안 된다.
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
                // 이미 fullscreen 이면 다시 걸지 않는다. 같은 값을 재설정해도 무해한
                // 플랫폼이 있지만, macOS 는 fullscreen 전환이 별도 Space 이동
                // 애니메이션이라 중복 호출이 눈에 보이는 깜빡임이 된다.
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
                        // fullscreen 해제는 그 이전 프레임 상태로 돌려주지만, 진입
                        // 시점이 maximize 였는지까지 보장하지는 않는 플랫폼이 있다.
                        // 저장해 둔 값을 그대로 다시 세운다(같은 값이면 no-op).
                        self.base.winit.set_maximized(maximized);
                    }
                }
                self.stage_saved_window_mode = None;
            }
            _ => {}
        }
    }

    /// 이 창의 전체화면 상태 + 덮고 있는 모니터 신원.
    ///
    /// 소비자는 `debug.fullscreen.state` 다. 멀티 모니터 실측은 개발 환경에서
    /// 재현할 수 없으므로, 사용자가 이 출력만 보내주면 모니터 타겟팅을 판정할 수
    /// 있게 하는 것이 이 함수의 목적이다.
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

    /// maximize 든 fullscreen 든 사용자가 가장자리를 끌어 크기를 바꿀 수 없다 —
    /// 리사이즈 엣지가 살아있으면 안 된다. **fullscreen 단독**이 이 함수 도입 전
    /// 빠져 있던 축이다(그 전에는 `is_maximized()` 만 봤다).
    #[test]
    fn fullscreen_단독으로도_리사이즈가_잠긴다() {
        assert!(!size_is_locked(false, false));
        assert!(size_is_locked(true, false));
        assert!(size_is_locked(false, true));
        assert!(size_is_locked(true, true));
    }

    /// **사용자가 만든 fullscreen 은 무대가 훔치지 않는다.** macOS 신호등으로 이미
    /// 전체화면인 창에서 무대를 열었다 닫아도 전체화면이 유지된다.
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
