//! 여러 popup 이 동시에 열렸을 때 **한 좌표를 누가 갖는가** 판정.
//!
//! `docs/design/systems/popup.md` 8대 규칙 7번("겹친 영역의 마우스 이벤트는 최상단
//! 팝업만 받는다")의 나머지 절반이다. z-order 렌더 순서는 이미 구현돼 있었지만,
//! "가려진 popup 은 그 좌표의 마우스 이벤트를 받지 않는다" 는 host↔plugin 경계에서
//! 빠져 있었다.
//!
//! **왜 bool 이 아니라 3-상태인가**: outside-click dismiss 는 "진짜 모든 popup 바깥"
//! 일 때만, click-to-front 는 "내 것" 일 때만 일어나야 한다. `rect.contains(p)` 하나로는
//! 그 사이의 "내 rect 밖이지만 위에 있는 popup 이 먹은 좌표" 를 표현할 수 없어, 그
//! 좌표가 dismiss 로 새는 것이 이 판정이 고치는 버그다.
//!
//! z_seq 는 host popup(`PopupState.z_seq`)과 plugin popup(`PopupInstance.z_seq`)이
//! **공유하는 전역 카운터**(`tasty_host_plugin::next_popup_z_seq`, ADR-0068)라 두
//! 종류를 한 배열에 섞어 비교해도 된다.

use egui::{Pos2, Rect};

/// 한 좌표에 대한 popup 하나의 소유 판정.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointOwnership {
    /// 내 rect 안이고, 나보다 위에 있는 popup 이 그 좌표를 덮지 않았다 → 내가 소비.
    Mine,
    /// 나보다 z 가 높은 popup 이 그 좌표를 덮었다 → 내 것도 아니고 "바깥" 도 아니다.
    /// dismiss 도 focus-bump 도 하지 않는다.
    OccludedByHigher,
    /// 열린 어떤 popup 에도 속하지 않는 좌표 → 진짜 outside-click.
    OutsideAll,
}

/// `(rect, z_seq)` 로 표현한 다른 popup 하나. host/plugin 구분 없이 같은 배열에 담는다.
#[derive(Debug, Clone, Copy)]
pub struct Occluder {
    pub rect: Rect,
    pub z_seq: u64,
}

/// `my_rect`/`my_z` 인 popup 이 좌표 `p` 를 소유하는지 판정한다.
///
/// `others` 에는 현재 열려 있는 popup 을 host/plugin 구분 없이 넣는다. 순서는 무관하고
/// (z 비교만 한다), **자기 자신이 섞여 있어도 된다** — 비교가 `>` 라 z 동률은 가림으로
/// 보지 않으므로 자기 rect 가 자기를 가리는 일이 없다. 호출부가 매 프레임 자기 항목만
/// 빼낸 배열을 따로 만들 필요가 없게 한 계약이다.
///
/// 판정 순서가 중요하다 — "위에 가려졌는가" 를 `my_rect.contains` 보다 **먼저** 본다.
/// 내 rect 밖 + 상위 popup 안인 좌표(예: 내 위에 열린 더 큰 popup 의 가장자리)가
/// [`PointOwnership::OutsideAll`] 로 떨어지면 그게 곧 부모가 닫히는 버그다.
///
/// 나보다 **아래** 있는 popup 이 그 좌표를 덮는 것은 판정에 영향이 없다 — 내가 위이므로
/// 내 rect 밖이면 나에겐 여전히 바깥이다(그 좌표는 아래 popup 이 자기 판정에서 가져간다).
pub fn point_ownership(my_rect: Rect, my_z: u64, others: &[Occluder], p: Pos2) -> PointOwnership {
    if others.iter().any(|o| o.z_seq > my_z && o.rect.contains(p)) {
        return PointOwnership::OccludedByHigher;
    }
    if my_rect.contains(p) {
        return PointOwnership::Mine;
    }
    PointOwnership::OutsideAll
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
    }

    /// 재현 시나리오: 420×170 `file-open`(plugin) 위에 640×480 `file_picker`(host).
    /// 화면 중앙 정렬이라 작은 쪽이 큰 쪽 안에 들어간다.
    fn scenario() -> (Rect, Rect) {
        let parent = r(310.0, 415.0, 420.0, 170.0); // plugin file-open
        let child = r(200.0, 260.0, 640.0, 480.0); // host file_picker
        (parent, child)
    }

    #[test]
    fn overlapping_point_under_a_higher_popup_is_occluded() {
        let (parent, child) = scenario();
        let p = parent.center(); // 두 rect 모두 안
        assert_eq!(
            point_ownership(
                parent,
                1,
                &[Occluder {
                    rect: child,
                    z_seq: 2
                }],
                p
            ),
            PointOwnership::OccludedByHigher
        );
    }

    /// 자식 안 + 부모 밖 — 이 좌표가 `OutsideAll` 로 새면 부모가 닫힌다(원 버그).
    #[test]
    fn inside_higher_popup_but_outside_mine_is_occluded_not_outside() {
        let (parent, child) = scenario();
        let p = egui::pos2(child.min.x + 10.0, child.min.y + 10.0);
        assert!(!parent.contains(p));
        assert_eq!(
            point_ownership(
                parent,
                1,
                &[Occluder {
                    rect: child,
                    z_seq: 2
                }],
                p
            ),
            PointOwnership::OccludedByHigher
        );
    }

    #[test]
    fn point_only_inside_mine_is_mine() {
        let (parent, _) = scenario();
        let far = r(0.0, 0.0, 50.0, 50.0);
        assert_eq!(
            point_ownership(
                parent,
                1,
                &[Occluder {
                    rect: far,
                    z_seq: 9
                }],
                parent.center()
            ),
            PointOwnership::Mine
        );
    }

    #[test]
    fn point_outside_every_popup_is_outside_all() {
        let (parent, child) = scenario();
        let p = egui::pos2(5.0, 5.0);
        assert_eq!(
            point_ownership(
                parent,
                1,
                &[Occluder {
                    rect: child,
                    z_seq: 2
                }],
                p
            ),
            PointOwnership::OutsideAll
        );
    }

    /// z 역전(내가 위) — 겹치는 좌표는 내 것이다.
    #[test]
    fn higher_z_wins_the_overlap() {
        let (parent, child) = scenario();
        assert_eq!(
            point_ownership(
                parent,
                5,
                &[Occluder {
                    rect: child,
                    z_seq: 2
                }],
                parent.center()
            ),
            PointOwnership::Mine
        );
    }

    /// 아래 popup 이 덮는 좌표는 나에겐 여전히 바깥이다(내가 위이므로).
    #[test]
    fn lower_popup_coverage_does_not_shield_me() {
        let (parent, child) = scenario();
        let p = egui::pos2(child.min.x + 10.0, child.min.y + 10.0);
        assert_eq!(
            point_ownership(
                parent,
                5,
                &[Occluder {
                    rect: child,
                    z_seq: 2
                }],
                p
            ),
            PointOwnership::OutsideAll
        );
    }

    /// 같은 z 는 "위" 가 아니다 — 동률이면 가려지지 않은 것으로 본다(z_seq 는 전역
    /// 카운터라 실제로 동률이 나오지 않지만, 판정이 `>` 인지 `>=` 인지 고정해 둔다).
    #[test]
    fn equal_z_does_not_occlude() {
        let (parent, child) = scenario();
        assert_eq!(
            point_ownership(
                parent,
                2,
                &[Occluder {
                    rect: child,
                    z_seq: 2
                }],
                parent.center()
            ),
            PointOwnership::Mine
        );
    }
}
