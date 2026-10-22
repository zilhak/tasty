//! egui 오버레이와 native WebView 의 공통 판정. 플랫폼 호출 없이 모드·대상 영역·표시 여부를 정한다.
//! 계약은 docs/design/systems/webview.md 의 "egui 오버레이와 native WebView" 절이다.
//!
//! - 모드는 `Hide < Opaque < Transparent` 다. backend 가 낼 수 있는 모드와 차단 조건(전체화면
//!   무대·튜토리얼, 적용 실패)으로 정한다. `Hide` 는 지금까지의 동작(모든 WebView 숨김)이다.
//! - 대상은 host 의 non-passive 팝업, plugin 팝업, 팝업 안에서 연 드롭다운이다. 토스트와 passive
//!   팝업은 대상이 아니며 겹친 WebView 만 숨기는 기존 규칙을 따른다.
//! - 대상 영역은 egui 논리 좌표로 모았다가 WebView 창 기준 물리 정수 사각형으로 바꿔 backend 에 넘긴다.

use crate::model::{LogicalPx, LogicalRect, PhysicalPx, PhysicalRect};

/// 대상 팝업이 떠 있는 동안 native WebView 를 다루는 방식.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum OverlayMode {
    /// 모든 WebView 를 숨기고 그 자리에 경계 타일을 그린다. 지원하지 않는 OS 의 동작이다.
    #[default]
    Hide,
    /// 페이지를 보인 채 host 가 팝업·드롭다운 상자 안만 불투명하게 칠한다.
    Opaque,
    /// 페이지를 보인 채 host 가 투명도를 섞어 그 위에 합성한다.
    Transparent,
}

/// 오버레이 그림이 화면에 올라가는 곳. 모드와 별개다. 같은 `Opaque` 도 backend 에 따라 주 표면에 구멍을
/// 내 보이거나 별도 오버레이 창에 그린다. 지원 모드를 보고하는 backend 가 모드마다 고른다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayPaintSource {
    /// 주 표면. backend 가 페이지의 대상 영역에 구멍을 내므로 주 pass 가 그린 팝업이 그대로 보인다.
    /// 주 pass 가 그 모드·페이지로 present 되면 준비된 것이다.
    MainSurface,
    /// 별도 오버레이 창. 주 pass 뒤 오버레이 pass 가 그 창에 present 해야 준비된 것이다.
    #[default]
    OverlaySurface,
}

/// 창 단위 오버레이 host 의 상태. WebView 동기화 뒤 훅이 바꾼다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayHostSlot {
    /// 붙은 host 가 없다. 그림 위치가 오버레이 창인 모드가 필요해지면 만든다.
    #[default]
    Absent,
    /// host 와 그 surface 가 붙어 있다.
    Attached,
    /// 이번 대상 팝업 구간에 만들거나 붙이지 못했다. 구간이 끝나면 `Absent` 로 돌아가 한 번 다시 시도한다.
    FailedThisEpisode,
    /// 이 OS·세션은 host 를 만들지 않는다. 창 수명 동안 다시 시도하지 않는다.
    Unavailable,
}

impl OverlayHostSlot {
    /// 그림 위치가 오버레이 창인 모드의 상한. 아직 만들지 않았으면 `None` 이라 낮추지 않는다. 붙어 있으면
    /// `attached` 로 surface 의 상한을 묻고, 만들 수 없으면 `Hide` 다.
    pub fn ceiling(self, attached: impl FnOnce() -> Option<OverlayMode>) -> Option<OverlayMode> {
        match self {
            Self::Absent => None,
            Self::Attached => Some(attached().unwrap_or(OverlayMode::Hide)),
            Self::FailedThisEpisode | Self::Unavailable => Some(OverlayMode::Hide),
        }
    }

    /// 대상 팝업이 닫힌 프레임에 부른다.
    pub fn end_episode(&mut self) {
        if *self == Self::FailedThisEpisode {
            *self = Self::Absent;
        }
    }
}

/// 대상 레이어의 종류. 오버레이 그림과 대상 영역은 이 목록의 레이어만 모은다. 대상 레이어 목록은
/// 이 enum 하나이며 그리는 곳과의 일치는 `source_guards::webview_overlay_targets` 가 검사한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    /// `PopupManager::draw` 가 그린 non-passive host 팝업.
    HostPopup,
    /// `draw_plugin_popups` 가 그린 plugin 팝업 셸.
    PluginPopup,
    /// 팝오버 래퍼(`tasty_egui_theme::with_popover_frame`)가 그린 egui 드롭다운.
    Dropdown,
}

/// 이번 프레임에 보이는 대상 하나(egui 논리 좌표).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayTarget {
    pub kind: TargetKind,
    pub layer: egui::LayerId,
    /// 상자 사각형. 입력 판정과 가장자리(각진 모서리) 판정의 기준이다.
    pub rect: egui::Rect,
    /// 불투명 모드에서 host 가 칠하는 영역. 상자 바깥에 그리는 테두리까지 넣는다([`outlined_box`]).
    pub opaque_rect: egui::Rect,
    /// 상자 바깥으로 번지는 그림(그림자)까지 넣은 사각형. 투명 모드의 합성 영역이다.
    pub paint_rect: egui::Rect,
    /// 이 대상이 그린 scrim 범위. 불투명 모드에서는 scrim 을 그리지 않으므로 쓰지 않는다.
    pub scrim: Option<egui::Rect>,
}

/// 창 하나의 오버레이 상태. egui 프레임이 대상을 모으고, WebView 동기화가 모드와 페이지를 정한다.
/// 상자 가장자리와 오버레이 그림은 다음 프레임이 `pages`·`planned_pages` 로 정한다. 그래서 대상이
/// 처음 뜬 프레임에는 그림이 아직 없고, 동기화는 [`overlay_reveal_ready`] 가 참이 될 때까지 페이지를
/// 숨긴 채 다시 그리기를 요청한다.
#[derive(Debug, Default)]
pub struct OverlayFrame {
    /// 이번 프레임에 그린 대상.
    pub targets: Vec<OverlayTarget>,
    /// 마지막 동기화가 정한 모드.
    pub mode: OverlayMode,
    /// 마지막 동기화가 그 모드로 정한 그림 위치. `Hide` 에서는 쓰지 않는다.
    pub paint_source: OverlayPaintSource,
    /// 불투명 모드에서 드러낼 페이지(egui 논리 좌표). 다른 모드에서는 비어 있다.
    pub pages: Vec<egui::Rect>,
    /// 마지막 동기화가 이 모드로 드러내려는 페이지(egui 논리 좌표, surface id 순). 오버레이 그림을
    /// 이 영역으로 자른다. `Hide` 이거나 대상 팝업이 없으면 비어 있다.
    pub planned_pages: Vec<egui::Rect>,
    /// 직전 프레임 plugin 팝업 셸 레이어와 영역.
    pub prev_plugin_rects: Vec<(egui::LayerId, egui::Rect)>,
    pub health: OverlayHealth,
    /// backend 에 오버레이를 적용한 상태다. `Hide` 로 돌아가면 `clear_overlay` 를 부른다.
    pub applied: bool,
    /// 이번 프레임 오버레이 그림. 모드가 `Hide` 가 아니고 그림 위치가 준비됐을 때만([`wants_paint_job`])
    /// 프레임 끝에 채우고 렌더가 가져간다. `None` 은 준비하지 않은 것이고 빈 `shapes` 는 유효한 빈 그림이다.
    pub paint_job: Option<OverlayPaintJob>,
    /// 이번 프레임에 실제로 present 한 그림. 프레임 시작에 비우고 렌더가 present 뒤에 채운다.
    pub painted: Option<PaintedOverlay>,
    /// debug 빌드에서 backend 지원 모드를 덮어쓴다.
    pub forced: Option<OverlayMode>,
    /// 창 단위 오버레이 host 상태.
    pub host: OverlayHostSlot,
    /// 마지막 동기화가 드러낸 surface id.
    pub revealed: Vec<u32>,
    /// 마지막 동기화가 직전에 보이지 않던 페이지를 드러냈다. 그 위로 host 를 다시 올린다.
    pub newly_revealed: bool,
}

/// 한 프레임의 오버레이 그림. 어떤 모드·그림 위치·페이지·표면 크기를 기준으로 만들었는지 함께 든다.
/// 주 표면 그림은 주 pass 가 이미 그리므로 `shapes` 가 비어 있다.
#[derive(Debug, Default)]
pub struct OverlayPaintJob {
    pub mode: OverlayMode,
    pub source: OverlayPaintSource,
    pub pages: Vec<egui::Rect>,
    /// job 을 만들 때 주 표면의 물리 크기.
    pub surface_size: [u32; 2],
    pub shapes: Vec<egui::epaint::ClippedShape>,
}

impl OverlayFrame {
    /// `job` 을 `surface_size` 크기 주 표면 프레임과 함께 present 했다고 기록한다. job 을 만든 뒤 표면 크기가
    /// 바뀌었으면 그 그림은 지금 페이지와 맞지 않으므로 기록하지 않는다.
    pub fn record_present(&mut self, job: OverlayPaintJob, surface_size: [u32; 2]) {
        if job.surface_size != surface_size {
            return;
        }
        self.painted = Some(PaintedOverlay {
            mode: job.mode,
            source: job.source,
            pages: job.pages,
            surface_size,
        });
    }
}

/// present 한 그림의 기준.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintedOverlay {
    pub mode: OverlayMode,
    pub source: OverlayPaintSource,
    pub pages: Vec<egui::Rect>,
    /// present 한 주 표면의 물리 크기.
    pub surface_size: [u32; 2],
}

/// 모드를 정하는 입력. 매 프레임 새로 만든다.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModeInputs {
    /// backend 가 이 창에서 낼 수 있는 가장 좋은 모드. 지원하지 않으면 `Hide`.
    pub support: OverlayMode,
    /// debug 빌드에서만 쓰는 지원 모드 덮어쓰기.
    pub forced: Option<OverlayMode>,
    /// 전체화면 무대나 튜토리얼이 떠 있다.
    pub blocked: bool,
    /// 이번 오버레이 구간에서 backend 적용이 실패했다.
    pub failed: bool,
}

pub fn select_mode(inputs: ModeInputs) -> OverlayMode {
    if inputs.blocked || inputs.failed {
        return OverlayMode::Hide;
    }
    inputs.forced.unwrap_or(inputs.support)
}

/// 이 WebView 를 드러낼지. `overlay_open` 은 `has_egui_overlay_open()` 그대로이며, 모드가 `Hide` 가
/// 아니면 대상 팝업이 떠 있어도 숨기지 않는다. 토스트·passive 팝업 규칙(`under_toast`)은 그대로다.
pub fn reveal_wanted(
    mode: OverlayMode,
    overlay_open: bool,
    active: bool,
    under_toast: bool,
) -> bool {
    active && !under_toast && (!overlay_open || mode != OverlayMode::Hide)
}

/// 이번 프레임에 오버레이 그림 job 을 만들지. 오버레이 창 그림은 그 창이 있어야 한다. 주 표면 그림은
/// 주 pass 가 언제나 그리므로 창이 없어도 만든다.
pub fn wants_paint_job(
    mode: OverlayMode,
    source: OverlayPaintSource,
    has_overlay_surface: bool,
) -> bool {
    mode != OverlayMode::Hide
        && match source {
            OverlayPaintSource::MainSurface => true,
            OverlayPaintSource::OverlaySurface => has_overlay_surface,
        }
}

/// 프레임 끝에 이번 프레임 그림 job 을 준비한다. 직전 동기화의 모드·그림 위치·`planned_pages` 를 기준으로
/// 삼는다. 오버레이 창 그림만 `shapes` 로 대상 레이어를 복제한다. 주 표면 그림은 주 pass 가 그대로 그린다.
pub fn prepare_paint_job(
    frame: &mut OverlayFrame,
    has_overlay_surface: bool,
    surface_size: [u32; 2],
    shapes: impl FnOnce(&[OverlayTarget], &[egui::Rect]) -> Vec<egui::epaint::ClippedShape>,
) {
    if !wants_paint_job(frame.mode, frame.paint_source, has_overlay_surface) {
        return;
    }
    let pages = frame.planned_pages.clone();
    let shapes = match frame.paint_source {
        OverlayPaintSource::MainSurface => Vec::new(),
        OverlayPaintSource::OverlaySurface => shapes(&frame.targets, &pages),
    };
    frame.paint_job = Some(OverlayPaintJob {
        mode: frame.mode,
        source: frame.paint_source,
        pages,
        surface_size,
        shapes,
    });
}

/// 주 표면 그림 job 을 꺼낸다. 오버레이 창 job 은 그대로 둔다. 렌더는 꺼낸 job 을 주 pass present 뒤에
/// [`OverlayFrame::record_present`] 로 남긴다.
pub fn take_main_surface_job(frame: &mut OverlayFrame) -> Option<OverlayPaintJob> {
    frame
        .paint_job
        .take_if(|job| job.source == OverlayPaintSource::MainSurface)
}

/// 대상 팝업이 떠 있는 동안 `mode` 로 페이지를 드러내도 되는지. 같은 프레임에 같은 모드·그림 위치·페이지로
/// 만든 그림이 지금과 같은 크기의 표면에 present 됐을 때만 참이다. 팝업이 없거나 `Hide` 면 이 판정과 무관하다.
pub fn overlay_reveal_ready(
    mode: OverlayMode,
    source: OverlayPaintSource,
    overlay_open: bool,
    painted: Option<&PaintedOverlay>,
    planned_pages: &[egui::Rect],
    surface_size: [u32; 2],
) -> bool {
    if !overlay_open || mode == OverlayMode::Hide {
        return true;
    }
    painted.is_some_and(|p| {
        p.mode == mode
            && p.source == source
            && p.pages == planned_pages
            && p.surface_size == surface_size
    })
}

/// egui 가 상자를 그릴 때 맞추는 픽셀 경계. 테셀레이터는 테두리 종류와 무관하게 상자 네 변을
/// `round_to_pixels` 로 반올림하고(`TessellationOptions::round_rects_to_pixels`), egui-wgpu 는 clip 을 같은
/// 반올림으로 scissor 로 바꾼다. 영역을 바깥으로 내림·올림하면 .5 이상인 변에서 그리지 않은 한 줄이 생긴다.
pub fn painted_box(rect: egui::Rect, pixels_per_point: f32) -> egui::Rect {
    use egui::emath::GuiRounding as _;
    rect.round_to_pixels(pixels_per_point)
}

/// 상자 바깥(`StrokeKind::Outside`)에 그리는 테두리까지 넣은 그림 영역. 소속 범위 밖은 자른다. 상자와 범위는
/// egui 가 그릴 때와 같은 픽셀 경계([`painted_box`])에 맞춘 뒤 테두리를 더한다.
pub fn outlined_box(
    rect: egui::Rect,
    outside_stroke: LogicalPx,
    scope: egui::Rect,
    pixels_per_point: f32,
) -> egui::Rect {
    painted_box(rect, pixels_per_point)
        .expand(outside_stroke.value())
        .intersect(painted_box(scope, pixels_per_point))
}

/// 불투명 모드에서 상자 가장자리를 정할 페이지 영역(egui 논리 좌표). 다른 모드에서는 비운다.
pub fn opaque_pages(mode: OverlayMode, revealed: &[LogicalRect]) -> Vec<egui::Rect> {
    if mode != OverlayMode::Opaque {
        return Vec::new();
    }
    revealed.iter().map(|r| to_egui(*r)).collect()
}

/// backend 에 넘길 대상 영역(WebView 창 기준 물리 정수 사각형). 불투명 모드는 상자와 바깥 테두리만, 투명 모드는
/// 그림자와 scrim 범위까지 넣는다. `webview` 는 `set_bounds` 와 같이 반올림한 창 사각형이다.
pub fn webview_regions(
    mode: OverlayMode,
    webview: PhysicalRect,
    targets: &[OverlayTarget],
    scale_factor: f32,
) -> Vec<PhysicalRect> {
    let logical: Vec<egui::Rect> = match mode {
        OverlayMode::Hide => return Vec::new(),
        OverlayMode::Opaque => targets.iter().map(|t| t.opaque_rect).collect(),
        OverlayMode::Transparent => targets
            .iter()
            .flat_map(|t| std::iter::once(t.paint_rect).chain(t.scrim))
            .collect(),
    };
    let physical: Vec<PhysicalRect> = logical
        .into_iter()
        .map(|r| from_egui(r).to_physical(scale_factor))
        .map(|r| match mode {
            OverlayMode::Opaque => snap_float_noise(r),
            _ => r,
        })
        .collect();
    crate::state::webview_edges::notice_clip_holes(webview, &physical)
}

/// 픽셀 경계에 맞춘 불투명 영역은 물리로 되돌리면 정수여야 하지만 f32 곱셈 오차로 정수 바로 옆에 놓일 수
/// 있다(배율 1.1·1.2·1.75 등). 그대로 바깥으로 넓히면 그리지 않은 한 줄이 생기므로 이 오차만 정수로 붙인다.
/// 배율이 정수가 아니라 테두리 바깥 끝이 실제로 픽셀 중간에 오는 경우는 오차보다 훨씬 커서 그대로 남는다.
fn snap_float_noise(r: PhysicalRect) -> PhysicalRect {
    const NOISE: PhysicalPx = PhysicalPx(1.0 / 256.0);
    let snap = |v: PhysicalPx| {
        let nearest = PhysicalPx(v.value().round());
        if (v - nearest).abs() <= NOISE {
            nearest
        } else {
            v
        }
    };
    let (x0, y0) = (snap(r.x), snap(r.y));
    let (x1, y1) = (snap(r.x + r.width), snap(r.y + r.height));
    PhysicalRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

/// 대상 팝업이 떠 있는 동안 WebView 영역 위 클릭·휠은 host 가 받는다. 페이지에는 가지 않는다.
pub fn input_goes_to_host(mode: OverlayMode, overlay_open: bool) -> bool {
    overlay_open && mode != OverlayMode::Hide
}

/// 적용 실패를 오버레이 구간 단위로 기억한다. 실패하면 그 프레임부터 구간이 끝날 때까지 `Hide` 이고,
/// 경고는 구간마다 한 번 남긴다. 구간은 대상 팝업이 모두 닫히면 끝난다.
#[derive(Debug, Default)]
pub struct OverlayHealth {
    failed: bool,
}

impl OverlayHealth {
    pub fn failed(&self) -> bool {
        self.failed
    }

    /// 실패를 기록한다. 이 구간의 첫 실패면 참을 돌려준다(호출자가 경고를 남긴다).
    pub fn record_failure(&mut self) -> bool {
        !std::mem::replace(&mut self.failed, true)
    }

    /// 오버레이가 닫힌 프레임에 부른다. 다음 구간은 다시 backend 를 시도한다.
    pub fn end_episode(&mut self) {
        self.failed = false;
    }
}

/// plugin 팝업은 host 가 직전 프레임 영역으로 입력을 판정한다. 움직인 프레임에도 상자가 비지 않도록
/// 같은 레이어의 직전 영역과 현재 영역을 합친 사각형을 대상으로 둔다.
pub fn plugin_box(
    layer: egui::LayerId,
    current: egui::Rect,
    previous: &[(egui::LayerId, egui::Rect)],
) -> egui::Rect {
    previous
        .iter()
        .filter(|(l, _)| *l == layer)
        .fold(current, |acc, (_, r)| acc.union(*r))
}

/// 프레임 시작에 부른다. 지난 프레임 대상·그림 기록과 드롭다운 기록을 지우고 이번 프레임 그리기가 쓸 페이지를 넣는다.
pub fn begin_frame(ctx: &egui::Context, frame: &mut OverlayFrame) {
    frame.targets.clear();
    frame.paint_job = None;
    frame.painted = None;
    tasty_egui_theme::overlay_edge::clear_frame_popovers(ctx);
    tasty_egui_theme::overlay_edge::set_opaque_pages(ctx, frame.pages.clone());
}

/// 프레임 끝에 부른다. 열린 egui 드롭다운(팝업 레이어)을 대상에 더한다. 팝오버 래퍼가 기록한
/// 드롭다운은 그때 정한 가장자리를 쓰고, 래퍼 밖에서 연 드롭다운도 열려 있으면 대상으로 둔다.
pub fn end_frame(ctx: &egui::Context, frame: &mut OverlayFrame) {
    let records = tasty_egui_theme::overlay_edge::frame_popovers(ctx);
    let shadow = ctx.style().visuals.popup_shadow;
    let open: Vec<(egui::LayerId, egui::Rect)> = ctx.memory(|m| {
        m.layer_ids()
            .filter(|l| l.order == egui::Order::Foreground && m.is_popup_open(l.id))
            .filter_map(|l| m.area_rect(l.id).map(|r| (l, r)))
            .collect()
    });
    for (layer, rect) in open {
        let shadow = if records.iter().any(|r| r.layer == layer && r.edge.opaque) {
            egui::Shadow::NONE
        } else {
            shadow
        };
        frame.targets.push(OverlayTarget {
            kind: TargetKind::Dropdown,
            layer,
            rect,
            // egui 드롭다운 Frame 은 테두리를 상자 안쪽에 그린다.
            opaque_rect: painted_box(rect, ctx.pixels_per_point()),
            paint_rect: rect + shadow.margin(),
            scrim: None,
        });
    }
}

fn to_egui(r: LogicalRect) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(r.x.value(), r.y.value()),
        egui::vec2(r.width.value(), r.height.value()),
    )
}

fn from_egui(r: egui::Rect) -> LogicalRect {
    LogicalRect {
        x: LogicalPx(r.min.x),
        y: LogicalPx(r.min.y),
        width: LogicalPx(r.width()),
        height: LogicalPx(r.height()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn er(x: f32, y: f32, w: f32, h: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
    }

    fn pr(x: f32, y: f32, w: f32, h: f32) -> PhysicalRect {
        PhysicalRect {
            x: PhysicalPx(x),
            y: PhysicalPx(y),
            width: PhysicalPx(w),
            height: PhysicalPx(h),
        }
    }

    fn values(rects: &[PhysicalRect]) -> Vec<[f32; 4]> {
        rects
            .iter()
            .map(|r| [r.x.value(), r.y.value(), r.width.value(), r.height.value()])
            .collect()
    }

    fn target(rect: egui::Rect, paint: egui::Rect, scrim: Option<egui::Rect>) -> OverlayTarget {
        OverlayTarget {
            kind: TargetKind::HostPopup,
            layer: egui::LayerId::new(egui::Order::Foreground, egui::Id::new("t")),
            rect,
            opaque_rect: rect,
            paint_rect: paint,
            scrim,
        }
    }

    /// 바깥 1px 테두리가 불투명 영역 안에 들어간다. 입력 판정 상자(`rect`)는 그대로다.
    #[test]
    fn the_opaque_region_keeps_the_outside_border() {
        let popup = er(100.0, 100.0, 100.0, 100.0);
        let scope = er(0.0, 0.0, 400.0, 300.0);
        let at = |ppp: f32| OverlayTarget {
            opaque_rect: outlined_box(popup, LogicalPx(1.0), scope, ppp),
            ..target(popup, popup.expand(8.0), None)
        };
        let webview = pr(0.0, 0.0, 400.0, 300.0);
        let at1 = webview_regions(OverlayMode::Opaque, webview, &[at(1.0)], 1.0);
        assert_eq!(values(&at1), vec![[99.0, 99.0, 102.0, 102.0]]);
        let at2 = webview_regions(
            OverlayMode::Opaque,
            pr(0.0, 0.0, 800.0, 600.0),
            &[at(2.0)],
            2.0,
        );
        assert_eq!(values(&at2), vec![[198.0, 198.0, 204.0, 204.0]]);
        assert_eq!(at(1.0).rect, popup);
        // 소속 범위 끝에 붙은 상자는 범위 밖 테두리를 칠하지 않는다.
        assert_eq!(
            outlined_box(er(0.0, 0.0, 50.0, 50.0), LogicalPx(1.0), scope, 1.0),
            er(0.0, 0.0, 51.0, 51.0)
        );
    }

    /// egui 가 실제로 칠하는 픽셀 행·열 범위. 테셀레이터가 만든 mesh 에서 가장 바깥 정점은 계조(AA) 띠의 투명
    /// 정점이고 칠한 경계보다 반 픽셀 바깥이다. 그 경계를 바깥 정수로 넓힌 범위가 한 번이라도 칠해지는 픽셀이다.
    fn tessellated_pixels(rect: egui::Rect, stroke: egui::StrokeKind, ppp: f32) -> [f32; 4] {
        use egui::epaint::{Mesh, RectShape, TessellationOptions, Tessellator};
        let shape = RectShape::new(
            rect,
            0.0,
            egui::Color32::WHITE,
            egui::Stroke::new(1.0, egui::Color32::WHITE),
            stroke,
        );
        let mut mesh = Mesh::default();
        Tessellator::new(ppp, TessellationOptions::default(), [1, 1], Vec::new())
            .tessellate_shape(shape.into(), &mut mesh);
        let outer = mesh.vertices.iter().fold(egui::Rect::NOTHING, |r, v| {
            r.union(egui::Rect::from_min_max(v.pos, v.pos))
        });
        assert!(
            mesh.vertices
                .iter()
                .filter(|v| v.pos.y == outer.min.y)
                .all(|v| v.color.a() == 0),
            "가장 바깥 정점은 계조 띠의 투명 정점이어야 한다"
        );
        // 계조 띠는 1 물리 픽셀이다(`feathering_size_in_pixels` 기본값).
        let painted = outer.shrink(PhysicalPx(0.5).to_logical(ppp).value());
        let px = |v: f32| LogicalPx(v).to_physical(ppp);
        let noise = PhysicalPx(1.0 / 256.0);
        [
            (px(painted.min.x) + noise).value().floor(),
            (px(painted.min.y) + noise).value().floor(),
            (px(painted.max.x) - noise).value().ceil(),
            (px(painted.max.y) - noise).value().ceil(),
        ]
    }

    fn region_edges(region: &[PhysicalRect]) -> [f32; 4] {
        assert_eq!(region.len(), 1);
        let r = region[0];
        [
            r.x.value(),
            r.y.value(),
            (r.x + r.width).value(),
            (r.y + r.height).value(),
        ]
    }

    /// 불투명 영역은 egui 가 칠하는 픽셀과 정확히 같다. 변마다 소수부(0, .3, .5, .7)와 배율(정수·비정수)을
    /// 바꿔 host 팝업(바깥 테두리)과 드롭다운(안쪽 테두리)을 실제 테셀레이터 결과와 비교한다. 영역이 넓으면
    /// 페이지와 팝업 사이에 host 배경 한 줄이 보이고, 좁으면 페이지가 테두리를 덮는다.
    #[test]
    fn the_opaque_region_matches_the_pixels_egui_paints() {
        let scope = er(0.0, 0.0, 2000.0, 1500.0);
        for ppp in [1.0_f32, 1.25, 1.5, 1.75, 2.0, 1.1, 1.2] {
            let webview = pr(
                0.0,
                0.0,
                LogicalPx(2000.0).to_physical(ppp).value(),
                LogicalPx(1500.0).to_physical(ppp).value(),
            );
            for fx in [0.0_f32, 0.3, 0.5, 0.7] {
                for fy in [0.0_f32, 0.3, 0.5, 0.7] {
                    for (w, h) in [(540.0_f32, 419.0_f32), (240.3, 180.5), (75.7, 33.2)] {
                        let popup = er(370.0 + fx, 150.0 + fy, w, h);
                        let host = OverlayTarget {
                            opaque_rect: outlined_box(popup, LogicalPx(1.0), scope, ppp),
                            ..target(popup, popup, None)
                        };
                        assert_eq!(
                            region_edges(&webview_regions(
                                OverlayMode::Opaque,
                                webview,
                                &[host],
                                ppp
                            )),
                            tessellated_pixels(popup, egui::StrokeKind::Outside, ppp),
                            "host popup {popup:?} ppp {ppp}"
                        );
                        let dropdown = OverlayTarget {
                            kind: TargetKind::Dropdown,
                            opaque_rect: painted_box(popup, ppp),
                            ..target(popup, popup, None)
                        };
                        assert_eq!(
                            region_edges(&webview_regions(
                                OverlayMode::Opaque,
                                webview,
                                &[dropdown],
                                ppp
                            )),
                            tessellated_pixels(popup, egui::StrokeKind::Inside, ppp),
                            "dropdown {popup:?} ppp {ppp}"
                        );
                    }
                }
            }
        }
    }

    /// 감사에서 본 실제 경우: 배율 1, 위 변 150.5. 영역 위 끝은 egui 테두리가 시작하는 150 이다(149 가 아니다).
    /// 범위(scope) 경계도 egui-wgpu scissor 와 같은 반올림으로 자른다.
    #[test]
    fn a_half_pixel_edge_does_not_expose_a_row_above_the_border() {
        let scope = er(0.0, 0.0, 1280.0, 800.0);
        let webview = pr(0.0, 0.0, 1280.0, 800.0);
        let popup = er(370.0, 150.5, 540.0, 419.0);
        let t = OverlayTarget {
            opaque_rect: outlined_box(popup, LogicalPx(1.0), scope, 1.0),
            ..target(popup, popup, None)
        };
        assert_eq!(
            values(&webview_regions(OverlayMode::Opaque, webview, &[t], 1.0)),
            vec![[369.0, 150.0, 542.0, 421.0]]
        );
        let clipped = outlined_box(
            er(370.0, 90.0, 540.0, 419.0),
            LogicalPx(1.0),
            er(0.0, 100.5, 1280.0, 699.5),
            1.0,
        );
        assert_eq!(clipped.min.y, 101.0);
    }

    /// 프레임 끝에 모은 egui 드롭다운의 불투명 영역도 egui 가 그릴 픽셀 경계에 맞춘다(위 변 20.5 → 21).
    #[test]
    fn an_open_dropdown_takes_the_painted_pixel_box() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("dropdown");
        let mut frame = OverlayFrame::default();
        let _output = ctx.run(egui::RawInput::default(), |ctx| {
            ctx.memory_mut(|m| m.open_popup(id));
            egui::Area::new(id)
                .order(egui::Order::Foreground)
                .fixed_pos(egui::pos2(10.0, 20.5))
                .show(ctx, |ui| ui.allocate_space(egui::vec2(50.0, 30.0)));
            end_frame(ctx, &mut frame);
        });
        let dropdown = frame
            .targets
            .iter()
            .find(|t| t.kind == TargetKind::Dropdown)
            .expect("open dropdown");
        assert_eq!(dropdown.rect.min.y, 20.5);
        assert_eq!(dropdown.opaque_rect, painted_box(dropdown.rect, 1.0));
        assert_eq!(dropdown.opaque_rect.min.y, 21.0);
    }

    /// 대상 팝업이 처음 뜬 프레임에는 그림이 없으므로 페이지를 드러내지 않는다. 같은 모드·그림 위치·페이지로
    /// 만든 그림이 같은 크기 표면에 present 된 뒤에만 드러낸다. 빈 그림도 그 기준으로 올렸다면 준비된 것이다.
    #[test]
    fn pages_wait_for_an_overlay_frame_painted_for_the_same_mode_and_pages() {
        use OverlayPaintSource::{MainSurface, OverlaySurface};
        const SIZE: [u32; 2] = [800, 600];
        let page = er(0.0, 0.0, 300.0, 200.0);
        let painted = |mode, source, pages: &[egui::Rect], size| PaintedOverlay {
            mode,
            source,
            pages: pages.to_vec(),
            surface_size: size,
        };
        let ready = |mode, source, p: Option<&PaintedOverlay>, pages: &[egui::Rect]| {
            overlay_reveal_ready(mode, source, true, p, pages, SIZE)
        };
        let opaque_on_overlay = painted(OverlayMode::Opaque, OverlaySurface, &[page], SIZE);
        assert!(!ready(OverlayMode::Opaque, OverlaySurface, None, &[page]));
        assert!(!ready(
            OverlayMode::Transparent,
            OverlaySurface,
            Some(&opaque_on_overlay),
            &[page]
        ));
        assert!(!ready(
            OverlayMode::Opaque,
            OverlaySurface,
            Some(&painted(
                OverlayMode::Opaque,
                OverlaySurface,
                &[er(0.0, 0.0, 100.0, 100.0)],
                SIZE
            )),
            &[page]
        ));
        assert!(ready(
            OverlayMode::Opaque,
            OverlaySurface,
            Some(&opaque_on_overlay),
            &[page]
        ));
        assert!(ready(
            OverlayMode::Transparent,
            OverlaySurface,
            Some(&painted(
                OverlayMode::Transparent,
                OverlaySurface,
                &[],
                SIZE
            )),
            &[]
        ));
        // 같은 모드·페이지라도 다른 그림 위치나 다른 크기 표면의 기록은 쓰지 않는다.
        assert!(!ready(
            OverlayMode::Opaque,
            MainSurface,
            Some(&opaque_on_overlay),
            &[page]
        ));
        assert!(!ready(
            OverlayMode::Opaque,
            OverlaySurface,
            Some(&painted(OverlayMode::Opaque, MainSurface, &[page], SIZE)),
            &[page]
        ));
        assert!(!ready(
            OverlayMode::Opaque,
            MainSurface,
            Some(&painted(
                OverlayMode::Opaque,
                MainSurface,
                &[page],
                [800, 599]
            )),
            &[page]
        ));
        assert!(ready(
            OverlayMode::Opaque,
            MainSurface,
            Some(&painted(OverlayMode::Opaque, MainSurface, &[page], SIZE)),
            &[page]
        ));
        // 팝업이 없거나 숨김 모드면 기존 표시 판정 그대로다.
        assert!(overlay_reveal_ready(
            OverlayMode::Opaque,
            OverlaySurface,
            false,
            None,
            &[page],
            SIZE
        ));
        assert!(overlay_reveal_ready(
            OverlayMode::Hide,
            OverlaySurface,
            true,
            None,
            &[page],
            SIZE
        ));
    }

    /// 주 표면 그림은 오버레이 창 없이도 job 을 만든다. 오버레이 창 그림은 창이 있어야 한다. 렌더는 주 표면
    /// job 만 꺼내 present 뒤 기록으로 남기고, 오버레이 창 job 은 오버레이 pass 에 남긴다.
    #[test]
    fn a_main_surface_job_needs_no_overlay_surface() {
        use OverlayPaintSource::{MainSurface, OverlaySurface};
        assert!(wants_paint_job(OverlayMode::Opaque, MainSurface, false));
        assert!(!wants_paint_job(OverlayMode::Opaque, OverlaySurface, false));
        assert!(wants_paint_job(
            OverlayMode::Transparent,
            OverlaySurface,
            true
        ));
        assert!(!wants_paint_job(OverlayMode::Hide, MainSurface, true));

        let mut frame = OverlayFrame {
            paint_job: Some(OverlayPaintJob {
                source: OverlaySurface,
                ..OverlayPaintJob::default()
            }),
            ..OverlayFrame::default()
        };
        assert!(take_main_surface_job(&mut frame).is_none());
        assert!(frame.paint_job.is_some());
        frame.paint_job = Some(OverlayPaintJob {
            mode: OverlayMode::Opaque,
            source: MainSurface,
            surface_size: [10, 10],
            ..OverlayPaintJob::default()
        });
        let job = take_main_surface_job(&mut frame).expect("main surface job");
        assert!(frame.paint_job.is_none());
        // 만든 뒤 크기가 바뀐 표면에 올린 그림은 기록하지 않는다.
        let resized = OverlayPaintJob {
            surface_size: [10, 10],
            ..OverlayPaintJob::default()
        };
        frame.record_present(resized, [12, 10]);
        assert!(frame.painted.is_none());
        frame.record_present(job, [10, 10]);
        let painted = frame.painted.as_ref().expect("recorded");
        assert_eq!(
            (painted.mode, painted.source),
            (OverlayMode::Opaque, MainSurface)
        );
        assert_eq!(painted.surface_size, [10, 10]);
    }

    #[test]
    fn an_unsupported_backend_keeps_the_current_hide_behavior() {
        let mode = select_mode(ModeInputs::default());
        assert_eq!(mode, OverlayMode::Hide);
        // 대상 팝업이 뜨면 활성 WebView 도 숨긴다 — 지금 동작 그대로다.
        assert!(!reveal_wanted(mode, true, true, false));
        assert!(reveal_wanted(mode, false, true, false));
        assert!(webview_regions(mode, pr(0.0, 0.0, 100.0, 100.0), &[], 1.0).is_empty());
        assert!(!input_goes_to_host(mode, true));
    }

    #[test]
    fn a_supporting_backend_keeps_the_webview_visible_under_a_popup() {
        let mode = select_mode(ModeInputs {
            support: OverlayMode::Opaque,
            ..ModeInputs::default()
        });
        assert_eq!(mode, OverlayMode::Opaque);
        assert!(reveal_wanted(mode, true, true, false));
        assert!(input_goes_to_host(mode, true));
        // 비활성 탭과 토스트 규칙은 그대로다.
        assert!(!reveal_wanted(mode, true, false, false));
        assert!(!reveal_wanted(mode, true, true, true));
    }

    #[test]
    fn stage_tutorial_and_failure_fall_back_to_hide() {
        for inputs in [
            ModeInputs {
                support: OverlayMode::Transparent,
                blocked: true,
                ..ModeInputs::default()
            },
            ModeInputs {
                support: OverlayMode::Transparent,
                failed: true,
                ..ModeInputs::default()
            },
            ModeInputs {
                forced: Some(OverlayMode::Opaque),
                blocked: true,
                ..ModeInputs::default()
            },
        ] {
            assert_eq!(select_mode(inputs), OverlayMode::Hide);
        }
    }

    #[test]
    fn the_debug_override_replaces_the_backend_report() {
        let mode = select_mode(ModeInputs {
            support: OverlayMode::Hide,
            forced: Some(OverlayMode::Opaque),
            ..ModeInputs::default()
        });
        assert_eq!(mode, OverlayMode::Opaque);
    }

    /// 팝업과 드롭다운 상자는 WebView 창 기준 좌표, 바깥쪽 정수 픽셀로 간다. scrim 은 넘기지 않는다.
    #[test]
    fn opaque_regions_are_the_boxes_in_webview_coordinates() {
        let webview = pr(200.0, 100.0, 600.0, 400.0);
        let popup = target(
            er(300.0, 150.0, 240.0, 160.0),
            er(280.0, 150.0, 280.0, 200.0),
            Some(er(0.0, 0.0, 1000.0, 800.0)),
        );
        let dropdown = OverlayTarget {
            kind: TargetKind::Dropdown,
            ..target(
                er(320.5, 290.25, 100.0, 300.0),
                er(320.0, 290.0, 100.0, 300.0),
                None,
            )
        };
        let regions = webview_regions(OverlayMode::Opaque, webview, &[popup, dropdown], 1.0);
        assert_eq!(
            values(&regions),
            vec![
                [100.0, 50.0, 240.0, 160.0],
                // 아래로 WebView 밖까지 늘어진 부분은 잘린다.
                [120.0, 190.0, 101.0, 210.0],
            ]
        );
    }

    #[test]
    fn transparent_regions_include_the_shadow_and_scrim() {
        let webview = pr(0.0, 0.0, 400.0, 300.0);
        let popup = target(
            er(100.0, 100.0, 50.0, 50.0),
            er(90.0, 95.0, 70.0, 70.0),
            Some(er(0.0, 0.0, 400.0, 300.0)),
        );
        let regions = webview_regions(OverlayMode::Transparent, webview, &[popup], 1.0);
        assert_eq!(
            values(&regions),
            vec![[90.0, 95.0, 70.0, 70.0], [0.0, 0.0, 400.0, 300.0]]
        );
    }

    /// 배율이 바뀌면 같은 논리 상자가 다른 물리 사각형이 된다. 매 프레임 현재 배율로 다시 계산한다.
    #[test]
    fn regions_follow_the_scale_factor() {
        let webview = pr(0.0, 0.0, 2000.0, 2000.0);
        let popup = target(
            er(10.0, 10.0, 100.0, 50.0),
            er(10.0, 10.0, 100.0, 50.0),
            None,
        );
        let at1 = webview_regions(OverlayMode::Opaque, webview, &[popup], 1.0);
        let at2 = webview_regions(OverlayMode::Opaque, webview, &[popup], 2.0);
        assert_eq!(values(&at1), vec![[10.0, 10.0, 100.0, 50.0]]);
        assert_eq!(values(&at2), vec![[20.0, 20.0, 200.0, 100.0]]);
    }

    #[test]
    fn a_failure_warns_once_per_episode() {
        let mut health = OverlayHealth::default();
        assert!(health.record_failure());
        assert!(!health.record_failure());
        assert!(health.failed());
        health.end_episode();
        assert!(!health.failed());
        assert!(health.record_failure());
    }

    #[test]
    fn a_plugin_box_covers_its_previous_and_current_frame() {
        let mine = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("p1"));
        let other = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("p2"));
        let prev = er(0.0, 0.0, 10.0, 10.0);
        let now = er(5.0, 0.0, 10.0, 10.0);
        assert_eq!(
            plugin_box(
                mine,
                now,
                &[(mine, prev), (other, er(100.0, 0.0, 1.0, 1.0))]
            ),
            er(0.0, 0.0, 15.0, 10.0)
        );
        assert_eq!(plugin_box(mine, now, &[]), now);
    }

    #[test]
    fn opaque_pages_are_empty_outside_the_opaque_mode() {
        let page = LogicalRect {
            x: LogicalPx(1.0),
            y: LogicalPx(2.0),
            width: LogicalPx(3.0),
            height: LogicalPx(4.0),
        };
        assert!(opaque_pages(OverlayMode::Hide, &[page]).is_empty());
        assert!(opaque_pages(OverlayMode::Transparent, &[page]).is_empty());
        assert_eq!(
            opaque_pages(OverlayMode::Opaque, &[page]),
            vec![er(1.0, 2.0, 3.0, 4.0)]
        );
    }
}
