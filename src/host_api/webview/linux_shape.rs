//! Linux WebView X 창의 shape 합성. 분할선·안내 카드 구멍(`set_native_holes`)과 불투명 오버레이 구멍은
//! 같은 X 창의 shape 하나를 나눠 쓰므로 한곳에서 합쳐 적용한다. 어느 쪽이 먼저 불려도 마지막 적용이
//! 두 입력을 모두 담는다. 계약은 docs/design/systems/webview.md 의 "egui 오버레이와 native WebView" 절.

/// 이 X 창 기준 물리 px `[x, y, 너비, 높이]`.
pub(super) type Rect = [i32; 4];

/// X SHAPE 확장의 그림 shape 종류(`ShapeBounding`). 이 밖은 그려지지도 입력을 받지도 않는다.
const SHAPE_BOUNDING: std::os::raw::c_int = 0;
/// X SHAPE 확장의 입력 shape 종류(`ShapeInput`).
const SHAPE_INPUT: std::os::raw::c_int = 2;

/// 불투명 오버레이가 이 창에서 가져가는 부분. 팝업 상자는 그림·입력에서 빠지고, host 가 포인터를
/// 가져가면 입력 shape 전체가 빈다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OverlayCut {
    pub regions: Vec<Rect>,
    pub host_takes_pointer: bool,
}

impl OverlayCut {
    /// 요청의 영역은 이미 정수 물리 px 다. 반올림은 부동소수 오차만 흡수한다.
    pub fn from_request(request: &super::overlay::OverlayRequest<'_>) -> Self {
        let px = |v: crate::model::PhysicalPx| v.value().round() as i32;
        Self {
            regions: request
                .regions
                .iter()
                .map(|r| [px(r.x), px(r.y), px(r.width), px(r.height)])
                .collect(),
            host_takes_pointer: request.host_takes_pointer,
        }
    }
}

/// X 창 하나의 shape 입력(native 구멍·오버레이 구멍)과 마지막으로 적용한 shape.
#[derive(Debug, Default)]
pub(super) struct ShapeState {
    input_holes: Vec<Rect>,
    clip: Vec<Rect>,
    /// 대상 팝업이 떠 있는 동안 불투명 오버레이가 가져가는 부분.
    overlay: Option<OverlayCut>,
    applied: Option<WindowShape>,
}

impl ShapeState {
    pub fn set_native_holes(&mut self, input_holes: &[Rect], clip: &[Rect]) {
        self.input_holes = input_holes.to_vec();
        self.clip = clip.to_vec();
    }

    /// 오버레이 구멍을 바꾸거나 지운다. 지우기 전에 오버레이가 있었으면 참이다.
    pub fn set_overlay(&mut self, overlay: Option<OverlayCut>) -> bool {
        std::mem::replace(&mut self.overlay, overlay).is_some()
    }

    /// 오버레이가 지금 이 창의 포인터 입력을 host 로 돌리고 있는지.
    pub fn overlay_holds_pointer(&self) -> bool {
        self.overlay.as_ref().is_some_and(|o| o.host_takes_pointer)
    }

    /// `size` 창에 맞춘 shape 가 마지막 적용값과 다르면 그 shape 를 돌려주고 적용값으로 기록한다.
    /// 같으면 X 요청을 다시 보내지 않도록 None 이다.
    pub fn take_change(&mut self, size: (i32, i32)) -> Option<WindowShape> {
        let shape = compose(size, &self.input_holes, &self.clip, self.overlay.as_ref());
        if self.applied.as_ref() == Some(&shape) {
            return None;
        }
        self.applied = Some(shape.clone());
        Some(shape)
    }
}

/// 입력 shape. `Whole` 은 shape 를 지워 창 사각형 전체로 되돌린다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum InputShape {
    Whole,
    Cut(Vec<Rect>),
    /// 입력을 받지 않는다. 포인터와 포인터로 정해지는 키는 아래 부모(winit) 창으로 간다.
    Empty,
}

/// X 창에 적용할 shape. `bounding` 이 비면 그림 shape 를 지운다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WindowShape {
    pub size: (i32, i32),
    pub bounding: Vec<Rect>,
    pub input: InputShape,
}

/// native 구멍과 오버레이 구멍을 합친다. 그림에서는 안내 카드와 팝업 상자를 빼고, 입력에서는 모든
/// 구멍을 뺀다. 오버레이가 포인터를 가져가면 입력 shape 는 빈다.
pub(super) fn compose(
    size: (i32, i32),
    input_holes: &[Rect],
    clip: &[Rect],
    overlay: Option<&OverlayCut>,
) -> WindowShape {
    let overlay_regions = overlay.map_or(&[][..], |o| o.regions.as_slice());
    let bounding: Vec<Rect> = clip.iter().chain(overlay_regions).copied().collect();
    let input = if overlay.is_some_and(|o| o.host_takes_pointer) {
        InputShape::Empty
    } else {
        let cut: Vec<Rect> = input_holes
            .iter()
            .chain(clip)
            .chain(overlay_regions)
            .copied()
            .collect();
        if cut.is_empty() {
            InputShape::Whole
        } else {
            InputShape::Cut(cut)
        }
    };
    WindowShape {
        size,
        bounding,
        input,
    }
}

/// 서버에 SHAPE 확장이 있는지. XFixes 의 창 shape 요청은 이 확장에 기댄다. 없으면 경고한다.
pub(super) fn shape_extension_ready(
    xlib: &x11_dl::xlib::Xlib,
    display: *mut x11_dl::xlib::Display,
) -> bool {
    let (mut opcode, mut event, mut error) = (0, 0, 0);
    // SAFETY: display 는 호출자가 확인한 유효한 연결이고 이름은 NUL 로 끝나는 정적 문자열이다.
    // 출력 인자는 지역 변수다.
    let ready = unsafe {
        (xlib.XQueryExtension)(
            display,
            c"SHAPE".as_ptr(),
            &mut opcode,
            &mut event,
            &mut error,
        )
    } != 0;
    if !ready {
        tracing::warn!("WebView input shape disabled: the X server has no SHAPE extension");
    }
    ready
}

/// `shape` 를 창에 적용한다. 호출자는 같은 shape 를 다시 보내지 않도록 마지막 적용값과 비교한다.
///
/// # Safety
/// `display`·`window` 가 유효하고 이 연결을 만든 스레드에서 불러야 한다.
pub(super) unsafe fn apply(
    fixes: &x11_dl::xfixes::Xlib,
    xlib: &x11_dl::xlib::Xlib,
    display: *mut x11_dl::xlib::Display,
    window: std::os::raw::c_ulong,
    shape: &WindowShape,
) {
    let xrect = |&[x, y, w, h]: &Rect| x11_dl::xlib::XRectangle {
        x: x.clamp(i16::MIN.into(), i16::MAX.into()) as i16,
        y: y.clamp(i16::MIN.into(), i16::MAX.into()) as i16,
        width: w.clamp(0, u16::MAX.into()) as u16,
        height: h.clamp(0, u16::MAX.into()) as u16,
    };
    let whole = [0, 0, shape.size.0, shape.size.1];
    // 아래 SAFETY 는 모두 호출자 계약(유효한 display·window, 생성 스레드)에 기댄다. 만든 region 은
    // shape 에 복사된 뒤 바로 지운다. region None(0)은 그 종류의 shape 를 지워 창 전체 사각형으로 되돌린다.
    let create = |rects: &mut [x11_dl::xlib::XRectangle]| {
        // SAFETY: 사각형 배열은 호출 동안 살아 있고 개수가 함께 간다. 0개면 빈 region 이다.
        unsafe { (fixes.XFixesCreateRegion)(display, rects.as_mut_ptr(), rects.len() as i32) }
    };
    let destroy = |region| {
        // SAFETY: 이 함수가 만든 region 을 한 번만 지운다.
        unsafe { (fixes.XFixesDestroyRegion)(display, region) }
    };
    let set = |kind, region| {
        // SAFETY: region 은 이 함수가 만든 것이거나 None(0)이다.
        unsafe { (fixes.XFixesSetWindowShapeRegion)(display, window, kind, 0, 0, region) }
    };
    let set_cut = |kind, cut: &[Rect]| {
        if cut.is_empty() {
            set(kind, 0);
            return;
        }
        let region = create(&mut [xrect(&whole)]);
        let mut cut_rects: Vec<_> = cut.iter().map(xrect).collect();
        let holes = create(&mut cut_rects);
        // SAFETY: 두 region 모두 방금 만든 것이다.
        unsafe { (fixes.XFixesSubtractRegion)(display, region, region, holes) };
        set(kind, region);
        destroy(holes);
        destroy(region);
    };
    set_cut(SHAPE_BOUNDING, &shape.bounding);
    match &shape.input {
        InputShape::Whole => set_cut(SHAPE_INPUT, &[]),
        InputShape::Cut(cut) => set_cut(SHAPE_INPUT, cut),
        InputShape::Empty => {
            let empty = create(&mut []);
            set(SHAPE_INPUT, empty);
            destroy(empty);
        }
    }
    // SAFETY: 호출자 계약대로 display 는 유효하다.
    unsafe { (xlib.XFlush)(display) };
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (i32, i32) = (800, 600);
    const DIVIDER: Rect = [0, 0, 6, 600];
    const NOTICE: Rect = [600, 0, 200, 40];
    const POPUP: Rect = [100, 100, 300, 200];

    fn overlay(host_takes_pointer: bool) -> OverlayCut {
        OverlayCut {
            regions: vec![POPUP],
            host_takes_pointer,
        }
    }

    #[test]
    fn no_holes_restore_the_whole_window() {
        let shape = compose(SIZE, &[], &[], None);
        assert!(shape.bounding.is_empty());
        assert_eq!(shape.input, InputShape::Whole);
    }

    #[test]
    fn native_holes_alone_keep_their_existing_shape() {
        let shape = compose(SIZE, &[DIVIDER], &[NOTICE], None);
        assert_eq!(shape.bounding, vec![NOTICE]);
        assert_eq!(shape.input, InputShape::Cut(vec![DIVIDER, NOTICE]));
    }

    #[test]
    fn overlay_cuts_popup_boxes_from_drawing_with_the_notice() {
        let shape = compose(SIZE, &[DIVIDER], &[NOTICE], Some(&overlay(true)));
        assert_eq!(shape.bounding, vec![NOTICE, POPUP]);
    }

    #[test]
    fn host_pointer_empties_the_whole_input_shape() {
        let shape = compose(SIZE, &[DIVIDER], &[NOTICE], Some(&overlay(true)));
        assert_eq!(shape.input, InputShape::Empty);
    }

    #[test]
    fn overlay_without_host_pointer_only_cuts_its_boxes_from_input() {
        let shape = compose(SIZE, &[DIVIDER], &[], Some(&overlay(false)));
        assert_eq!(shape.input, InputShape::Cut(vec![DIVIDER, POPUP]));
    }

    #[test]
    fn clearing_the_overlay_returns_to_the_native_holes_shape() {
        let with = compose(SIZE, &[DIVIDER], &[NOTICE], Some(&overlay(true)));
        let without = compose(SIZE, &[DIVIDER], &[NOTICE], None);
        assert_ne!(with, without);
        assert_eq!(without, compose(SIZE, &[DIVIDER], &[NOTICE], None));
    }

    #[test]
    fn an_unchanged_shape_is_not_sent_again() {
        let mut state = ShapeState::default();
        state.set_native_holes(&[DIVIDER], &[]);
        assert!(state.take_change(SIZE).is_some());
        assert!(state.take_change(SIZE).is_none());
        state.set_overlay(Some(overlay(true)));
        assert_eq!(
            state.take_change(SIZE).map(|s| s.input),
            Some(InputShape::Empty)
        );
        assert!(state.take_change(SIZE).is_none());
    }

    #[test]
    fn clearing_the_overlay_sends_the_native_shape_back() {
        let mut state = ShapeState::default();
        state.set_native_holes(&[DIVIDER], &[NOTICE]);
        state.set_overlay(Some(overlay(true)));
        state.take_change(SIZE);
        assert!(state.overlay_holds_pointer());
        assert!(state.set_overlay(None));
        assert!(!state.overlay_holds_pointer());
        assert_eq!(
            state.take_change(SIZE),
            Some(compose(SIZE, &[DIVIDER], &[NOTICE], None))
        );
        assert!(!state.set_overlay(None));
    }

    #[test]
    fn a_new_window_size_is_a_different_shape() {
        let before = compose(SIZE, &[], &[], Some(&overlay(true)));
        let after = compose((1000, 600), &[], &[], Some(&overlay(true)));
        assert_ne!(before, after);
    }
}
