//! Windows WebView child HWND 의 창 region 과 포인터 차단 상태. 안내 카드 구멍(`set_native_holes`)과
//! 대상 팝업 영역(`apply_overlay`)은 같은 창 region 하나를 쓰므로 두 입력을 함께 보관해 합친다.
//! 한쪽이 다른 쪽 구멍을 지우지 않게 하기 위해서다. 계약은 docs/design/systems/webview.md 의
//! "egui 오버레이와 native WebView" 절이다. OS 호출 없이 판단만 하므로 다른 OS 의 시험에서도 돈다.

/// HWND 기준 물리 정수 픽셀 `[x, y, 너비, 높이]`.
pub(crate) type HoleRect = [i32; 4];

/// 창 region 에 적용할 것.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RegionUpdate {
    /// 지금 적용된 region 과 같다.
    Unchanged,
    /// region 을 지워 원래 사각형으로 돌린다.
    Clear,
    /// 크기 `size` 의 사각형에서 `holes` 를 뺀 region 을 적용한다.
    Set {
        size: (i32, i32),
        holes: Vec<HoleRect>,
    },
}

/// 마지막으로 적용한 region. `None` 은 region 이 없는(원래 사각형) 상태다.
type Applied = Option<((i32, i32), Vec<HoleRect>)>;

#[derive(Debug, Default)]
pub(crate) struct WindowRegion {
    clip: Vec<HoleRect>,
    overlay: Vec<HoleRect>,
    /// 실제로 적용된 region. 적용에 성공했을 때만 바꾼다.
    applied: Applied,
}

impl WindowRegion {
    /// 안내 카드 구멍을 바꾼다. 대상 팝업 영역은 유지한다.
    pub(crate) fn set_clip(&mut self, clip: &[HoleRect]) {
        self.clip = clip.to_vec();
    }

    /// 대상 팝업 영역을 바꾼다. 안내 카드 구멍은 유지한다.
    pub(crate) fn set_overlay(&mut self, regions: &[HoleRect]) {
        self.overlay = regions.to_vec();
    }

    /// 대상 팝업 영역만 비운다.
    pub(crate) fn clear_overlay(&mut self) {
        self.overlay.clear();
    }

    /// 지금 입력과 HWND 크기로 만들 region 이 적용된 것과 다르면 그 갱신을 돌려준다.
    pub(crate) fn pending(&self, size: (i32, i32)) -> RegionUpdate {
        let holes = self.holes();
        let wanted: Applied = (!holes.is_empty()).then_some((size, holes));
        if wanted == self.applied {
            return RegionUpdate::Unchanged;
        }
        match wanted {
            None => RegionUpdate::Clear,
            Some((size, holes)) => RegionUpdate::Set { size, holes },
        }
    }

    /// `pending` 이 돌려준 갱신을 OS 에 적용했다. 실패했으면 부르지 않아 다음 호출이 다시 시도한다.
    pub(crate) fn mark_applied(&mut self, update: &RegionUpdate) {
        match update {
            RegionUpdate::Unchanged => {}
            RegionUpdate::Clear => self.applied = None,
            RegionUpdate::Set { size, holes } => self.applied = Some((*size, holes.clone())),
        }
    }

    /// 안내 카드 구멍 뒤에 대상 팝업 영역을 잇는다. 넓이가 없는 사각형은 뺄 것이 없어 넣지 않는다.
    fn holes(&self) -> Vec<HoleRect> {
        self.clip
            .iter()
            .chain(&self.overlay)
            .copied()
            .filter(|&[_, _, w, h]| w > 0 && h > 0)
            .collect()
    }
}

/// WebView 영역 전체의 포인터를 host 로 돌리는 상태. 켜고 끄는 OS 호출이 필요한 때만 알려 준다.
#[derive(Debug, Default)]
pub(crate) struct PointerGate {
    held: bool,
}

impl PointerGate {
    /// 이번 적용이 원하는 상태로 바꿔야 하면 새 상태(참이면 host 가 가져감)를 돌려준다.
    pub(crate) fn wants(&self, host_takes_pointer: bool) -> Option<bool> {
        (self.held != host_takes_pointer).then_some(host_takes_pointer)
    }

    /// OS 에 적용한 상태를 기록한다.
    pub(crate) fn set(&mut self, held: bool) {
        self.held = held;
    }

    pub(crate) fn held(&self) -> bool {
        self.held
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(region: &mut WindowRegion, size: (i32, i32)) -> RegionUpdate {
        let update = region.pending(size);
        region.mark_applied(&update);
        update
    }

    #[test]
    fn the_notice_and_the_popup_share_one_region_without_erasing_each_other() {
        let mut region = WindowRegion::default();
        region.set_clip(&[[300, 0, 100, 40]]);
        assert_eq!(
            apply(&mut region, (400, 300)),
            RegionUpdate::Set {
                size: (400, 300),
                holes: vec![[300, 0, 100, 40]]
            }
        );
        region.set_overlay(&[[50, 60, 200, 120]]);
        assert_eq!(
            apply(&mut region, (400, 300)),
            RegionUpdate::Set {
                size: (400, 300),
                holes: vec![[300, 0, 100, 40], [50, 60, 200, 120]]
            }
        );
        // 안내 카드를 같은 값으로 다시 넣어도 팝업 구멍은 그대로다.
        region.set_clip(&[[300, 0, 100, 40]]);
        assert_eq!(apply(&mut region, (400, 300)), RegionUpdate::Unchanged);
    }

    #[test]
    fn clearing_the_overlay_returns_to_the_notice_only_region() {
        let mut region = WindowRegion::default();
        region.set_clip(&[[300, 0, 100, 40]]);
        region.set_overlay(&[[50, 60, 200, 120]]);
        apply(&mut region, (400, 300));
        region.clear_overlay();
        assert_eq!(
            apply(&mut region, (400, 300)),
            RegionUpdate::Set {
                size: (400, 300),
                holes: vec![[300, 0, 100, 40]]
            }
        );
    }

    #[test]
    fn without_any_hole_the_region_is_removed() {
        let mut region = WindowRegion::default();
        assert_eq!(region.pending((400, 300)), RegionUpdate::Unchanged);
        region.set_overlay(&[[50, 60, 200, 120]]);
        apply(&mut region, (400, 300));
        region.clear_overlay();
        assert_eq!(apply(&mut region, (400, 300)), RegionUpdate::Clear);
        assert_eq!(region.pending((400, 300)), RegionUpdate::Unchanged);
    }

    #[test]
    fn a_resized_window_or_a_new_scale_gets_a_new_region() {
        let mut region = WindowRegion::default();
        region.set_overlay(&[[50, 60, 200, 120]]);
        apply(&mut region, (400, 300));
        // 같은 구멍이어도 HWND 크기가 바뀌면 전체 사각형이 바뀐다.
        assert_eq!(
            apply(&mut region, (500, 300)),
            RegionUpdate::Set {
                size: (500, 300),
                holes: vec![[50, 60, 200, 120]]
            }
        );
        // 배율이 바뀐 프레임은 host 가 같은 상자를 새 물리 픽셀로 보낸다.
        region.set_overlay(&[[75, 90, 300, 180]]);
        assert_eq!(
            apply(&mut region, (750, 450)),
            RegionUpdate::Set {
                size: (750, 450),
                holes: vec![[75, 90, 300, 180]]
            }
        );
    }

    #[test]
    fn a_failed_update_is_tried_again() {
        let mut region = WindowRegion::default();
        region.set_overlay(&[[50, 60, 200, 120]]);
        let first = region.pending((400, 300));
        // OS 적용이 실패하면 기록하지 않는다.
        assert_eq!(region.pending((400, 300)), first);
    }

    #[test]
    fn empty_rects_make_no_hole() {
        let mut region = WindowRegion::default();
        region.set_overlay(&[[50, 60, 0, 120], [10, 10, 20, 0]]);
        assert_eq!(region.pending((400, 300)), RegionUpdate::Unchanged);
    }

    #[test]
    fn the_pointer_gate_only_asks_for_a_change() {
        let mut gate = PointerGate::default();
        assert_eq!(gate.wants(false), None);
        assert_eq!(gate.wants(true), Some(true));
        gate.set(true);
        assert!(gate.held());
        assert_eq!(gate.wants(true), None);
        assert_eq!(gate.wants(false), Some(false));
    }
}
