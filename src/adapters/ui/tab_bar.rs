//! Pane tab bars — pane 별 상단 탭 표시 + 사용자 입력 (focus / drag / context menu / 새 탭).
//!
//! ## Split: wrapper / view / action
//!
//! 순수 시각 `draw_pane_tab_bars_view` 는 [`PaneTabBarsProps`] 만 받고
//! [`PaneTabBarsOutput`] (collected actions + measured height) 만 반환한다.
//! AppState/CoreState/`theme::theme()` 비의존. Gallery (`tasty-gallery`) 는
//! view 를 mock props 로 mirror 해서 시각 검증.
//!
//! wrapper `draw_pane_tab_bars` 는 (a) state/engine 에서 props 추출,
//! (b) view 호출, (c) 반환된 [`TabBarAction`] 리스트를 state mutation 으로 변환,
//! (d) measured height 를 `state.tab_bar_height` 에 기록.

use egui::emath::GuiRounding as _;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::{LogicalPx, PhysicalPx};
use tasty_type_geometry::rect::PhysicalRect;

/// 활성 탭 마커가 `Dot` 일 때의 점 지름. 스케일 밖(4) — 점 치수 토큰은
/// `status-dot-size`(8) 하나뿐이라 여기를 그리로 보내면 점이 두 배가 된다.
/// `docs/adr/0126-off-scale-font-values-are-not-snapped-to-tokens.md` 대로 이름만 붙인다.
///
/// **이 상수가 생긴 이유가 값이 아니라 이름이다.** 종전에는 밑줄 마커의 *두께*
/// (`tab-indicator-width`, 2)를 그대로 점의 *반지름*으로 재사용하고 있었다. 두 치수는
/// 의미가 달라 한쪽만 바뀌어야 하는 날이 오는데, 이름을 공유하면 그때 둘이 같이 움직인다.
/// 지금 두 값이 짝(2 ↔ 4)인 것은 **우연이다** — 밑줄 두께가 바뀌어도 이 점은 안 바뀐다.
const TAB_ACTIVE_DOT_SIZE: LogicalPx = LogicalPx(4.0);

/// 탭의 busy 표시 점 지름. 스케일 밖(6) — 점 치수 토큰은 `status-dot-size`(8) 하나뿐이라
/// 그리로 보내면 배율 1 에서 픽셀이 바뀐다(ADR-0126 대로 이름만 붙인다).
///
/// **이 자리에는 겨냥하는 토큰 이름이 이미 있다** — `component.tab-dot-size` 인데 값이
/// `{component.status-dot-size}` = 8 이라 부르면 6 → 8 이 된다. 그래서 부르지 않았다.
/// 그 토큰이 디자인이 정한 8 인지, 다른 세 dot 이름을 만들 때 대칭으로 딸려 나온 8 인지가
/// 갈려야 이 자리가 토큰으로 갈지 값을 지킬지 정해진다.
///
/// **같은 6 을 `src/adapters/ui/sidebar/view.rs` 의 rail 상태 점도 쓴다** — 무관한 두
/// 화면이 독립적으로 고른 값이라, 판단이 서면 둘이 한 이름으로 모인다.
const TAB_BUSY_DOT_SIZE: LogicalPx = LogicalPx(6.0);

use crate::adapters::ui::icons;
use crate::core::AttentionKind;
use crate::state::AppState;
use crate::theme;

/// View 입력 — pane 한 개 분의 탭 데이터.
#[derive(Clone, Debug)]
pub struct PaneTabBarView {
    pub pane_id: u32,
    /// Pane 의 *물리* 좌표 사각형 (view 가 scale_factor 로 logical 변환).
    pub rect: PhysicalRect,
    pub tab_names: Vec<String>,
    /// 탭별 leading 아이콘. wrapper 가 registry(kind→`SurfaceKindDef.icon`)에서 해석해
    /// 담는다 — view 는 kind 를 모른 채 아이콘만 그린다(엔진 비의존 유지).
    pub tab_icons: Vec<icons::Icon>,
    /// 탭별 attention kind — 탭에 속한 surface 들의 dominant kind
    /// (`CoreState::attention_dominant_kind`). `Some(NeedsInput)`=노랑,
    /// `Some(Completion)`=파랑, `None`=attention 없음(active/평상시 색으로 폴백).
    pub tab_attention_kind: Vec<Option<AttentionKind>>,
    /// 탭별 busy(녹색 점) 여부.
    pub tab_is_busy: Vec<bool>,
    pub active_tab: usize,
    /// 이 pane 이 현재 focus 인지 — 배경 (surface0 vs mantle) 결정.
    pub is_focused: bool,
    /// 가로 스크롤 오프셋 (logical px).
    pub scroll_offset: f32,
}

/// View 입력 — drag 진행 중인 탭의 상태. None 이면 drag overlay 미표시.
#[derive(Clone, Debug)]
pub struct TabDragView {
    pub pane_id: u32,
    pub tab_index: usize,
    /// 현재 마우스 x (logical pane 좌표).
    pub current_x: f32,
}

/// View 입력 — 전체 pane 의 탭 바 + drag 상태 + appearance 옵션.
pub struct PaneTabBarsProps<'a> {
    pub theme: &'a Theme,
    /// 탭 slot 키캡 문자를 읽을 키바인딩 설정 (switch-number overlay 표시=동작 일치).
    pub kb: &'a crate::settings::KeybindingSettings,
    pub panes: &'a [PaneTabBarView],
    pub scale_factor: f32,
    /// 사용자 옵션 — 탭 1 개의 가로 너비 (logical px).
    pub tab_width: f32,
    /// 사용자 옵션 — 탭 라벨 폰트 크기 (logical px).
    pub tab_font_size: f32,
    /// 사용자 옵션 — 활성 탭 인디케이터 스타일 (Underline / Fill / Dot).
    pub active_tab_indicator: crate::settings::ActiveTabIndicator,
    /// 현재 drag 진행 상태 (None 이면 overlay 미표시).
    pub drag: Option<TabDragView>,
    /// switch-number overlay — 키캡을 그릴 **focused pane id**.
    /// 사용자가 `tab_switch_modifier`(대상=Tab)를 누르고 있는 동안만 `Some(focused_pane)`,
    /// 그 외엔 `None`. 이 pane 의 탭바에서만 leading 아이콘을 숫자 키캡(`Ctrl+1`…`0`)으로
    /// in-place 교체한다(비-focused pane 은 held 여도 아이콘 유지). release 시 `None` → 원복.
    pub switch_overlay_pane: Option<u32>,
}

/// View 가 발생시킨 사용자 의도. wrapper 가 state/engine 으로 반영.
#[derive(Clone, Debug, PartialEq)]
pub enum TabBarAction {
    SwitchTab {
        pane_id: u32,
        tab_index: usize,
    },
    CloseTab {
        pane_id: u32,
        tab_index: usize,
    },
    AddTab {
        pane_id: u32,
    },
    /// 탭스트립 우측 Split 아이콘 — 해당 pane 을 분할 (기존 split_pane 경로 재사용).
    RequestSplit {
        pane_id: u32,
    },
    /// 탭스트립 우측 Search 아이콘 — 해당 pane 활성 surface 검색 (기존 find 경로 재사용).
    OpenSearch {
        pane_id: u32,
    },
    ScrollLeft {
        pane_id: u32,
    },
    ScrollRight {
        pane_id: u32,
    },
    /// 활성 탭 전환(또는 pane 리사이즈)으로 활성 탭이 뷰포트 밖으로 밀려났을 때
    /// view 가 계산한 보정 스크롤 오프셋. 사용자가 직접 누른 액션이 아니라 뷰
    /// 렌더링 결과에 대한 보정이므로 `focus_target_pane` 대상에서 제외한다.
    AutoScrollToActiveTab {
        pane_id: u32,
        offset: f32,
    },
    /// 탭바의 탭 없는 빈 영역(뷰포트) primary click — 탭 전환 없이 그 pane 으로
    /// focus 만 이동한다.
    FocusPane {
        pane_id: u32,
    },
    OpenContextMenu {
        pane_id: u32,
        tab_index: usize,
        pos: egui::Pos2,
    },
    OpenPaneContextMenu {
        pane_id: u32,
        pos: egui::Pos2,
    },
    /// 탭 "+" 버튼 우클릭 — 프리셋으로 탭/페인 생성 진입점.
    OpenNewTabButtonContextMenu {
        pane_id: u32,
        pos: egui::Pos2,
    },
    DragStart {
        pane_id: u32,
        tab_index: usize,
    },
    DragUpdate {
        pane_id: u32,
        mouse_x: f32,
    },
    DragEnd {
        pane_id: u32,
    },
}

impl TabBarAction {
    /// 이 액션이 유래한 pane. `Some` 이면 처리 전에 그 pane 으로 focus 를 옮긴다
    /// (탭바 primary-click 계열 — 탭 클릭/닫기/스크롤/빈 영역 클릭/+·split·search 버튼).
    /// 우클릭 컨텍스트 메뉴는 대상 `pane_id`/`tab_index` 를 메뉴 항목에 그대로 실어
    /// 나르므로 focus 이동이 필요 없다(조회/메뉴-오픈이지 조작 commit 이 아님).
    ///
    /// `DragStart` 도 focus 이동 대상에 포함한다 — egui 0.31.1 의 `clicked()`/
    /// `drag_started_by()` 는 같은 press-release 상호작용에서 발생 프레임이 겹치지
    /// 않고 상호 배타적이라(`clicked()` 는 pointer-up 프레임에서만, `drag_started_by()`
    /// 는 그 이전에 drag threshold 를 넘는 프레임에서만 세팅됨 — vendored
    /// `egui-0.31.1/src/{context.rs,interaction.rs}` 확인), 비-focused pane 의 탭을
    /// 클릭 없이 곧장 드래그하면 `SwitchTab` 없이 `DragStart` 만 단독으로 발생한다.
    /// 이 경우에도 "탭바 조작은 그 pane 을 조작하는 행위"라는 원칙(위 문단)을 그대로
    /// 적용해 focus 가 따라가야 한다. `DragUpdate`/`DragEnd` 는 이미 `DragStart` 에서
    /// focus 가 이동한 뒤에 오는 후속 프레임이라 별도 이동이 불필요.
    fn focus_target_pane(&self) -> Option<u32> {
        match *self {
            TabBarAction::SwitchTab { pane_id, .. }
            | TabBarAction::CloseTab { pane_id, .. }
            | TabBarAction::AddTab { pane_id }
            | TabBarAction::RequestSplit { pane_id }
            | TabBarAction::OpenSearch { pane_id }
            | TabBarAction::ScrollLeft { pane_id }
            | TabBarAction::ScrollRight { pane_id }
            | TabBarAction::FocusPane { pane_id }
            | TabBarAction::DragStart { pane_id, .. } => Some(pane_id),
            TabBarAction::OpenContextMenu { .. }
            | TabBarAction::OpenPaneContextMenu { .. }
            | TabBarAction::OpenNewTabButtonContextMenu { .. }
            | TabBarAction::DragUpdate { .. }
            | TabBarAction::DragEnd { .. }
            | TabBarAction::AutoScrollToActiveTab { .. } => None,
        }
    }
}

/// View 의 출력 — 사용자 의도 리스트 + 측정된 탭 바 높이.
#[derive(Default)]
pub struct PaneTabBarsOutput {
    pub actions: Vec<TabBarAction>,
    /// 첫 pane 의 탭 바 높이. 측정 못 했으면 None. 좌표계는 주석이 아니라 **타입**이
    /// 보증한다 — egui 가 준 logical 높이를 `to_physical(scale_factor)` 로 변환해 담는다.
    pub measured_height_physical: Option<PhysicalPx>,
}

/// 순수 시각 view. AppState/CoreState/`theme::theme()` 비의존.
#[allow(clippy::cognitive_complexity)] // complexity-exempt: egui 즉시모드 draw — pane별 탭바 horizontal 클로저 나열이 구조적(clippy 가 클로저를 과대계상)
pub fn draw_pane_tab_bars_view(
    ctx: &egui::Context,
    props: &PaneTabBarsProps<'_>,
) -> PaneTabBarsOutput {
    let th = props.theme;
    let scale_factor = props.scale_factor;
    let mut output = PaneTabBarsOutput::default();

    let tab_w = props.tab_width;
    let label_font_size = props.tab_font_size;
    // 탭바는 host UI zoom 영향 받지 않는다 (사용자 제약). zoom-aware 토큰 (item_height_tab /
    // font_size_body / font_size_caption) 대신 zoom 미적용 tab_bar_* 토큰 사용.
    let bar_h = th.tab_bar_height.value();
    let plus_w: f32 = 28.0;
    // 우측 고정 IconButton (Split / Search) — 디자인 TabStrip 우측 클러스터.
    // 디자인 IconButton sm(control-height-tab) 에 맞춰 "+" 와 동일 폭.
    let icon_btn_w: f32 = 28.0;
    let right_icons_w: f32 = icon_btn_w * 2.0;
    let icon_glyph: f32 = 14.0;
    let arrow_w: f32 = 20.0;
    let separator_w: f32 = 1.0;
    let h_padding: f32 = 8.0;
    let dot_radius = TAB_BUSY_DOT_SIZE.value() * 0.5;
    let dot_pad: f32 = 6.0;
    let active_indicator_h = th.tab_indicator_width.value();
    let plus_font_size = th.tab_bar_label_font_size.value();
    let arrow_font_size = th.tab_bar_arrow_font_size.value();

    for info in props.panes {
        let logical_x = info.rect.x.to_logical(scale_factor).value().round_ui();
        let logical_y = info.rect.y.to_logical(scale_factor).value().round_ui();
        let logical_w = info.rect.width.to_logical(scale_factor).value().round_ui();
        let n = info.tab_names.len();
        let content_w =
            n as f32 * tab_w + (n.max(1) - 1) as f32 * separator_w + separator_w + plus_w;
        // 우측 IconButton 클러스터(Split/Search) 폭을 항상 확보한 뒤 남은 폭으로 탭/화살표 배치.
        let avail_w = (logical_w - right_icons_w).max(0.0);
        let needs_scroll = content_w > avail_w;
        let viewport_w = if needs_scroll {
            (avail_w - arrow_w * 2.0).max(0.0)
        } else {
            avail_w
        };
        let max_scroll = (content_w - viewport_w).max(0.0);
        let mut scroll = info.scroll_offset.clamp(0.0, max_scroll);

        // 활성 탭 추종 스크롤 — 활성 인덱스가 바뀌었거나(키보드/마우스 전환 공통)
        // pane 지오메트리(폭/탭 수)가 바뀌어 이전엔 보이던 활성 탭이 뷰포트 밖으로
        // 밀려난 경우에만 보정한다. 매 프레임 무조건 트리거하면 사용자가 화살표로
        // 수동 스크롤해 둔 상태(활성 탭 변경 없음)를 덮어써 버리므로, 직전 프레임과
        // 비교 가능한 상태를 `egui::Context` persistent memory 에 추적한다
        // (`switch_overlay::appear_fade` 와 동일 패턴 — view 는 AppState/CoreState
        // 비의존을 유지하면서 프레임 간 상태를 ctx 에 위임).
        let scroll_track_id = egui::Id::new("tab_bar_active_scroll_track").with(info.pane_id);
        let prev_track: Option<(usize, f32, usize)> = ctx.data(|d| d.get_temp(scroll_track_id));
        let active_changed = match prev_track {
            Some((active, _, _)) => active != info.active_tab,
            None => true,
        };
        let geometry_changed = match prev_track {
            Some((_, w, count)) => (w - logical_w).abs() > 0.5 || count != n,
            None => false,
        };
        ctx.data_mut(|d| d.insert_temp(scroll_track_id, (info.active_tab, logical_w, n)));

        if n > 0 && (active_changed || geometry_changed) {
            let active_start = info.active_tab as f32 * (tab_w + separator_w);
            let active_end = active_start + tab_w;
            if active_start < scroll {
                scroll = active_start;
            } else if active_end > scroll + viewport_w {
                scroll = active_end - viewport_w;
            }
            scroll = scroll.clamp(0.0, max_scroll);
            if (scroll - info.scroll_offset).abs() > f32::EPSILON {
                output.actions.push(TabBarAction::AutoScrollToActiveTab {
                    pane_id: info.pane_id,
                    offset: scroll,
                });
            }
        }

        let area_response = egui::Area::new(egui::Id::new(format!("pane_tabs_{}", info.pane_id)))
            .fixed_pos(egui::pos2(logical_x, logical_y))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let bg = if info.is_focused {
                    // component tab_bg()=mantle 라 부적합 — focus strip 은 surface-raised 값.
                    th.surface_raised()
                } else {
                    th.bg_sidebar()
                };

                egui::Frame::new()
                    .fill(bg.into())
                    .inner_margin(egui::Margin::ZERO)
                    .show(ui, |ui| {
                        ui.set_min_width(logical_w);
                        ui.set_max_width(logical_w);
                        ui.set_min_height(bar_h);
                        ui.set_max_height(bar_h);

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;

                            // Left arrow
                            if needs_scroll {
                                let can_left = scroll > 0.0;
                                let (r, resp) = ui.allocate_exact_size(
                                    egui::vec2(arrow_w, bar_h),
                                    egui::Sense::click(),
                                );
                                // divergence: disabled 화살표는 surface1 값(text-role 접근자 부재).
                                // 값-보존 위해 border_strong() 사용(§B3).
                                let arrow_color = if can_left {
                                    th.text_muted()
                                } else {
                                    th.border_strong()
                                };
                                if resp.hovered() && can_left {
                                    // divergence: hover 채움이 surface0(=surface-raised) 불투명값.
                                    // hover-overlay 로 바꾸면 픽셀 변함 → 값-보존 surface_raised().
                                    ui.painter().rect_filled(r, 0.0, th.surface_raised());
                                }
                                ui.painter().text(
                                    r.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "<",
                                    egui::FontId::proportional(arrow_font_size),
                                    arrow_color.into(),
                                );
                                if resp.clicked() && can_left {
                                    output.actions.push(TabBarAction::ScrollLeft {
                                        pane_id: info.pane_id,
                                    });
                                }
                            }

                            // Clipped tab area
                            let clip_start_x = ui.cursor().min.x;
                            let clip_rect = egui::Rect::from_min_size(
                                egui::pos2(clip_start_x, ui.cursor().min.y),
                                egui::vec2(viewport_w, bar_h),
                            );
                            let (_, viewport_resp) = ui.allocate_exact_size(
                                egui::vec2(viewport_w, bar_h),
                                egui::Sense::click(),
                            );
                            if viewport_resp.secondary_clicked() {
                                output.actions.push(TabBarAction::OpenPaneContextMenu {
                                    pane_id: info.pane_id,
                                    pos: viewport_resp.interact_pointer_pos().unwrap_or_default(),
                                });
                                ui.painter().rect_stroke(
                                    clip_rect,
                                    0.0,
                                    egui::Stroke::new(
                                        th.focus_ring_width.value(),
                                        th.accent_success(),
                                    ),
                                    egui::StrokeKind::Inside,
                                );
                            } else if viewport_resp.clicked() {
                                // 탭이 없는 빈 영역 primary click — 탭 전환 없이 그
                                // pane 으로 focus 만 이동. 탭 rect 클릭 시에도 같은
                                // 프레임에 SwitchTab 이 함께 emit 될 수 있으나 동일
                                // pane_id 라 focus 적용은 멱등(idempotent).
                                output.actions.push(TabBarAction::FocusPane {
                                    pane_id: info.pane_id,
                                });
                            }

                            let painter = ui.painter().with_clip_rect(clip_rect);
                            let mut x = clip_start_x - scroll;

                            for (i, name) in info.tab_names.iter().enumerate() {
                                if i > 0 {
                                    let sep = egui::Rect::from_min_size(
                                        egui::pos2(x, clip_rect.min.y),
                                        egui::vec2(separator_w, bar_h),
                                    );
                                    // divergence: 탭 구분선. 코드=surface1, 디자인 tab_separator()=
                                    // 반투명(값 다름) → 채택 금지. 값-보존 border_strong() (§B3).
                                    painter.rect_filled(sep, 0.0, th.border_strong());
                                    x += separator_w;
                                }

                                let is_active = i == info.active_tab;
                                let tab_kind = info.tab_attention_kind.get(i).copied().flatten();
                                let is_busy = info.tab_is_busy.get(i).copied().unwrap_or(false);
                                // Fill 스타일만 활성 탭 배경을 채운다. Underline/Dot 은
                                // 배경을 비활성과 동일하게 두고 별도 마커로 표시.
                                let tab_bg = if is_active
                                    && props.active_tab_indicator
                                        == crate::settings::ActiveTabIndicator::Fill
                                {
                                    th.bg_panel()
                                } else {
                                    bg
                                };
                                // 탭 제목 색 위계(디자인 확정): NeedsInput → Completion →
                                // active → 평상시. attention 은 포커스 시 해제되므로
                                // active 탭이 attention 틴트를 갖는 실제 충돌은 없다
                                // (방어적 순서일 뿐).
                                let text_color = match tab_kind {
                                    Some(AttentionKind::NeedsInput) => th.accent_warning(),
                                    Some(AttentionKind::Completion) => th.accent_primary(),
                                    None if is_active => th.text_primary(),
                                    None => th.text_muted(),
                                };

                                let tab_rect = egui::Rect::from_min_size(
                                    egui::pos2(x, clip_rect.min.y),
                                    egui::vec2(tab_w, bar_h),
                                );

                                painter.rect_filled(tab_rect, 0.0, tab_bg);

                                if is_active {
                                    use crate::settings::ActiveTabIndicator;
                                    match props.active_tab_indicator {
                                        ActiveTabIndicator::Underline => {
                                            let line_rect = egui::Rect::from_min_size(
                                                egui::pos2(tab_rect.min.x, tab_rect.min.y),
                                                egui::vec2(tab_w, active_indicator_h),
                                            );
                                            painter.rect_filled(
                                                line_rect,
                                                0.0,
                                                th.accent_primary(),
                                            );
                                        }
                                        // Fill: 배경은 위에서 이미 bg_panel() 로 채움 — 추가 마커 없음.
                                        ActiveTabIndicator::Fill => {}
                                        ActiveTabIndicator::Dot => {
                                            // 탭 상단 중앙의 accent 점 마커.
                                            let r = TAB_ACTIVE_DOT_SIZE.value() * 0.5;
                                            let center = egui::pos2(
                                                tab_rect.center().x,
                                                tab_rect.min.y + r * 2.0,
                                            );
                                            painter.circle_filled(center, r, th.accent_primary());
                                        }
                                    }
                                }

                                // close 버튼 슬롯(우측 h_padding + 14px)을 비워두고 dot 은
                                // 그 왼쪽에 둔다 (close 와 겹치지 않게).
                                let dot_right = tab_rect.max.x - h_padding - 14.0;
                                if is_busy {
                                    let dot_center =
                                        egui::pos2(dot_right - dot_radius, tab_rect.center().y);
                                    let color: egui::Color32 = th.accent_success().into();
                                    painter.circle_filled(dot_center, dot_radius, color);
                                }

                                // kind 아이콘 (leading) — ui_kit tab strip.
                                let icon_size = 14.0;
                                let icon_rect = egui::Rect::from_min_size(
                                    egui::pos2(
                                        tab_rect.min.x + h_padding,
                                        tab_rect.center().y - icon_size / 2.0,
                                    ),
                                    egui::vec2(icon_size, icon_size),
                                );
                                // switch-number overlay: tab_switch_modifier 홀드 + 단축키
                                // 있는 탭(1–9,0)은 아이콘 자리를 숫자 키캡으로 in-place 교체.
                                // focused pane(switch_overlay_pane) 의 탭바에서만 — 비-focused
                                // pane 은 held 여도 아이콘 유지(거짓 안내 방지).
                                // 폭/text_x 는 불변(아이콘 slot 중앙에 키캡) → 리플로 없음.
                                let switch_digit =
                                    crate::adapters::ui::switch_overlay::tab_keycap_for(
                                        props.kb,
                                        props.switch_overlay_pane,
                                        info.pane_id,
                                        i,
                                    );
                                // 등장 페이드(90ms, motion-ui-fast) — 이 pane 의 오버레이
                                // 활성 여부로 매 프레임 구동(키캡 미표시 프레임 포함 priming).
                                let overlay_active =
                                    props.switch_overlay_pane == Some(info.pane_id);
                                let fade = crate::adapters::ui::switch_overlay::appear_fade(
                                    ui.ctx(),
                                    th,
                                    info.pane_id,
                                    overlay_active,
                                );
                                if let Some(digit) = switch_digit {
                                    crate::adapters::ui::switch_overlay::paint_keycap(
                                        &painter,
                                        th,
                                        icon_rect.center(),
                                        digit,
                                        is_active,
                                        fade,
                                    );
                                } else {
                                    let icon =
                                        info.tab_icons.get(i).copied().unwrap_or(icons::FILE);
                                    // Image::paint_at 은 ui.painter()(=탭바 전폭 clip)를 쓰므로
                                    // 배경/텍스트와 달리 뷰포트 밖으로 새어 화살표/우측 버튼과
                                    // 겹친다. paint 동안만 ui clip 을 뷰포트로 좁혀 정합.
                                    let prev_clip = ui.clip_rect();
                                    ui.set_clip_rect(clip_rect.intersect(prev_clip));
                                    icon.image(icon_size, text_color.into())
                                        .paint_at(ui, icon_rect);
                                    ui.set_clip_rect(prev_clip);
                                }

                                // 텍스트 — 아이콘 뒤, 좌측 정렬. 우측엔 dot 공간 확보.
                                let text_x = icon_rect.max.x + 6.0;
                                // 텍스트 우측 한계: dot/close 슬롯(dot_right) 왼쪽.
                                let mut text_right = dot_right - 4.0;
                                if is_busy {
                                    text_right -= dot_radius * 2.0 + dot_pad;
                                }
                                let available_w = (text_right - text_x).max(0.0);
                                let font_id = egui::FontId::proportional(label_font_size);
                                let galley = painter.layout_no_wrap(
                                    name.clone(),
                                    font_id.clone(),
                                    text_color.into(),
                                );
                                let final_galley = if galley.size().x > available_w {
                                    let mut truncated = name.clone();
                                    loop {
                                        truncated.pop();
                                        let candidate = format!("{truncated}…");
                                        let g = painter.layout_no_wrap(
                                            candidate.clone(),
                                            font_id.clone(),
                                            text_color.into(),
                                        );
                                        if g.size().x <= available_w || truncated.is_empty() {
                                            break g;
                                        }
                                    }
                                } else {
                                    galley
                                };
                                let text_y = tab_rect.center().y - final_galley.size().y / 2.0;
                                painter.galley(
                                    egui::pos2(text_x, text_y),
                                    final_galley,
                                    text_color.into(),
                                );

                                let tab_clip = tab_rect.intersect(clip_rect);
                                if !tab_clip.is_negative() {
                                    let resp = ui.interact(
                                        tab_clip,
                                        egui::Id::new(format!("tab_{}_{}", info.pane_id, i)),
                                        egui::Sense::click_and_drag(),
                                    );
                                    // close 버튼 (active or hover) — 우측 끝. 클릭은
                                    // SwitchTab 보다 우선.
                                    let show_close = is_active || resp.hovered();
                                    let close_clicked = if show_close {
                                        let cs = 14.0;
                                        let close_rect = egui::Rect::from_center_size(
                                            egui::pos2(
                                                tab_rect.max.x - h_padding - cs / 2.0,
                                                tab_rect.center().y,
                                            ),
                                            egui::vec2(cs, cs),
                                        );
                                        let cr = ui.interact(
                                            close_rect,
                                            egui::Id::new(("tabclose", info.pane_id, i)),
                                            egui::Sense::click(),
                                        );
                                        if cr.hovered() {
                                            painter.rect_filled(
                                                close_rect,
                                                2.0,
                                                th.active_overlay.to_egui_premultiplied(),
                                            );
                                        }
                                        let cc: egui::Color32 = if cr.hovered() {
                                            th.text_primary().into()
                                        } else {
                                            th.text_muted().into()
                                        };
                                        // kind 아이콘과 동일: paint 동안만 ui clip 을 뷰포트로
                                        // 좁혀 우측 경계 탭의 close ✕ 가 화살표/버튼 위로
                                        // 새지 않게 한다(배경/텍스트 클립과 일관).
                                        let prev_clip = ui.clip_rect();
                                        ui.set_clip_rect(clip_rect.intersect(prev_clip));
                                        icons::CLOSE.image(cs, cc).paint_at(ui, close_rect);
                                        ui.set_clip_rect(prev_clip);
                                        cr.clicked()
                                    } else {
                                        false
                                    };
                                    if close_clicked {
                                        output.actions.push(TabBarAction::CloseTab {
                                            pane_id: info.pane_id,
                                            tab_index: i,
                                        });
                                    } else if resp.clicked() {
                                        output.actions.push(TabBarAction::SwitchTab {
                                            pane_id: info.pane_id,
                                            tab_index: i,
                                        });
                                    }
                                    if resp.secondary_clicked() {
                                        output.actions.push(TabBarAction::OpenContextMenu {
                                            pane_id: info.pane_id,
                                            tab_index: i,
                                            pos: resp.interact_pointer_pos().unwrap_or_default(),
                                        });
                                        painter.rect_stroke(
                                            tab_clip,
                                            0.0,
                                            egui::Stroke::new(
                                                th.focus_ring_width.value(),
                                                th.accent_success(),
                                            ),
                                            egui::StrokeKind::Inside,
                                        );
                                    }
                                    if resp.drag_started_by(egui::PointerButton::Primary) {
                                        output.actions.push(TabBarAction::DragStart {
                                            pane_id: info.pane_id,
                                            tab_index: i,
                                        });
                                    }
                                    if resp.dragged_by(egui::PointerButton::Primary)
                                        && let Some(pos) = resp.interact_pointer_pos()
                                    {
                                        output.actions.push(TabBarAction::DragUpdate {
                                            pane_id: info.pane_id,
                                            mouse_x: pos.x,
                                        });
                                    }
                                    if resp.drag_stopped_by(egui::PointerButton::Primary) {
                                        output.actions.push(TabBarAction::DragEnd {
                                            pane_id: info.pane_id,
                                        });
                                    }
                                }

                                x += tab_w;
                            }

                            // Separator before "+"
                            {
                                let sep = egui::Rect::from_min_size(
                                    egui::pos2(x, clip_rect.min.y),
                                    egui::vec2(separator_w, bar_h),
                                );
                                // divergence: 탭 구분선. 코드=surface1, 디자인 tab_separator()=
                                // 반투명(값 다름) → 채택 금지. 값-보존 border_strong() (§B3).
                                painter.rect_filled(sep, 0.0, th.border_strong());
                                x += separator_w;
                            }

                            // "+" button
                            {
                                let plus_rect = egui::Rect::from_min_size(
                                    egui::pos2(x, clip_rect.min.y),
                                    egui::vec2(plus_w, bar_h),
                                );
                                let plus_clip = plus_rect.intersect(clip_rect);
                                if !plus_clip.is_negative() {
                                    let resp = ui.interact(
                                        plus_clip,
                                        egui::Id::new(format!("tab_plus_{}", info.pane_id)),
                                        egui::Sense::click(),
                                    );
                                    if resp.hovered() {
                                        // divergence: hover 채움이 surface0(=surface-raised) 불투명값.
                                        // 값-보존 surface_raised() (hover-overlay 로 바꾸면 픽셀 변함).
                                        painter.rect_filled(plus_rect, 0.0, th.surface_raised());
                                    }
                                    painter.text(
                                        plus_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "+",
                                        egui::FontId::proportional(plus_font_size),
                                        th.text_muted().into(),
                                    );
                                    if resp.clicked() {
                                        output.actions.push(TabBarAction::AddTab {
                                            pane_id: info.pane_id,
                                        });
                                    }
                                    if resp.secondary_clicked() {
                                        output.actions.push(
                                            TabBarAction::OpenNewTabButtonContextMenu {
                                                pane_id: info.pane_id,
                                                pos: resp
                                                    .interact_pointer_pos()
                                                    .unwrap_or_default(),
                                            },
                                        );
                                        painter.rect_stroke(
                                            plus_clip,
                                            0.0,
                                            egui::Stroke::new(
                                                th.focus_ring_width.value(),
                                                th.accent_success(),
                                            ),
                                            egui::StrokeKind::Inside,
                                        );
                                    }
                                }
                            }

                            // Right arrow
                            if needs_scroll {
                                let can_right = scroll < max_scroll;
                                let (r, resp) = ui.allocate_exact_size(
                                    egui::vec2(arrow_w, bar_h),
                                    egui::Sense::click(),
                                );
                                // divergence: disabled 화살표는 surface1 값(text-role 접근자 부재).
                                // 값-보존 위해 border_strong() 사용(§B3).
                                let arrow_color = if can_right {
                                    th.text_muted()
                                } else {
                                    th.border_strong()
                                };
                                if resp.hovered() && can_right {
                                    // divergence: hover 채움이 surface0(=surface-raised) 불투명값 → 값-보존.
                                    ui.painter().rect_filled(r, 0.0, th.surface_raised());
                                }
                                ui.painter().text(
                                    r.center(),
                                    egui::Align2::CENTER_CENTER,
                                    ">",
                                    egui::FontId::proportional(arrow_font_size),
                                    arrow_color.into(),
                                );
                                if resp.clicked() && can_right {
                                    output.actions.push(TabBarAction::ScrollRight {
                                        pane_id: info.pane_id,
                                    });
                                }
                            }

                            // 우측 IconButton 클러스터 — Split / Search (디자인 TabStrip).
                            // 탭바는 zoom 비적용 → 고정 px. "+" 와 동일 호버 스타일.
                            for (icon, is_split) in [(icons::SPLIT, true), (icons::SEARCH, false)] {
                                let (r, resp) = ui.allocate_exact_size(
                                    egui::vec2(icon_btn_w, bar_h),
                                    egui::Sense::click(),
                                );
                                let color = if resp.hovered() {
                                    th.text_primary()
                                } else {
                                    th.text_muted()
                                };
                                if resp.hovered() {
                                    // divergence: hover 채움이 surface0(=surface-raised) 불투명값 → 값-보존.
                                    ui.painter().rect_filled(r, 0.0, th.surface_raised());
                                }
                                let icon_rect = egui::Rect::from_center_size(
                                    r.center(),
                                    egui::vec2(icon_glyph, icon_glyph),
                                );
                                icon.image(icon_glyph, color.into()).paint_at(ui, icon_rect);
                                if resp.clicked() {
                                    output.actions.push(if is_split {
                                        TabBarAction::RequestSplit {
                                            pane_id: info.pane_id,
                                        }
                                    } else {
                                        TabBarAction::OpenSearch {
                                            pane_id: info.pane_id,
                                        }
                                    });
                                }
                            }
                        });
                    });
            });

        if output.measured_height_physical.is_none() {
            let logical_h = LogicalPx(area_response.response.rect.height());
            output.measured_height_physical = Some(logical_h.to_physical(scale_factor));
        }
    }

    // Drag overlay (ghost tab + insert marker)
    if let Some(ref drag) = props.drag
        && let Some(pane_info) = props.panes.iter().find(|i| i.pane_id == drag.pane_id)
    {
        let pane_rect = pane_info.rect;
        let pane_logical_x = pane_rect.x.to_logical(scale_factor).value().round_ui();
        let pane_logical_y = pane_rect.y.to_logical(scale_factor).value().round_ui();
        let pane_logical_w = pane_rect.width.to_logical(scale_factor).value().round_ui();
        let n = pane_info.tab_names.len();
        let content_w =
            n as f32 * tab_w + (n.max(1) - 1) as f32 * separator_w + separator_w + plus_w;
        let avail_w = (pane_logical_w - right_icons_w).max(0.0);
        let needs_scroll_arrows = content_w > avail_w;
        let viewport_start = if needs_scroll_arrows {
            pane_logical_x + arrow_w
        } else {
            pane_logical_x
        };

        let drop_idx = compute_drop_index(
            drag.current_x,
            pane_logical_x,
            pane_info.scroll_offset,
            pane_info.tab_names.len(),
            tab_w,
            separator_w,
            pane_logical_w,
        );

        let marker_x =
            viewport_start - pane_info.scroll_offset + drop_idx as f32 * (tab_w + separator_w);
        let marker_rect = egui::Rect::from_min_size(
            egui::pos2(marker_x - 1.0, pane_logical_y),
            egui::vec2(2.0, bar_h),
        );
        let overlay_painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("tab_drag_overlay"),
        ));
        overlay_painter.rect_filled(marker_rect, 0.0, th.accent_primary());

        let ghost_name = pane_info
            .tab_names
            .get(drag.tab_index)
            .cloned()
            .unwrap_or_default();
        let ghost_rect = egui::Rect::from_min_size(
            egui::pos2(drag.current_x - tab_w / 2.0, pane_logical_y),
            egui::vec2(tab_w, bar_h),
        );
        let ghost_bg = th.bg_panel().with_alpha(180).to_egui();
        let ghost_fg = th.text_primary().with_alpha(180).to_egui();
        overlay_painter.rect_filled(ghost_rect, 0.0, ghost_bg);
        overlay_painter.text(
            ghost_rect.center(),
            egui::Align2::CENTER_CENTER,
            &ghost_name,
            egui::FontId::proportional(label_font_size),
            ghost_fg,
        );
    }

    output
}

/// 탭별 busy(녹색 점) 여부 계산. `is_surface_busy()`(로컬 ∪ mirror busy 합집합)를
/// 거쳐야 원격 attach mirror surface 를 담은 탭도 dot 이 뜬다.
fn compute_tab_is_busy(engine: &crate::core::CoreState, tabs: &[crate::model::Tab]) -> Vec<bool> {
    tabs.iter()
        .map(|t| {
            let sids = t.all_surface_ids();
            sids.iter().any(|sid| engine.is_surface_busy(*sid))
        })
        .collect()
}

/// Mouse x → drop target tab index. Pure 함수 — view/wrapper 양쪽에서 호출.
pub fn compute_drop_index(
    mouse_x: f32,
    pane_logical_x: f32,
    scroll_offset: f32,
    tab_count: usize,
    tab_w: f32,
    separator_w: f32,
    _pane_w: f32,
) -> usize {
    let content_x = mouse_x - pane_logical_x + scroll_offset;
    let slot = content_x / (tab_w + separator_w);
    slot.round()
        .clamp(0.0, (tab_count.saturating_sub(1)) as f32) as usize
}

/// Wrapper — state/engine 에서 props 추출 → view 호출 → action 적용.
///
/// 시그니처는 기존과 동일 (외부 호출처 무영향).
pub fn draw_pane_tab_bars(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    pane_rects: &[(u32, PhysicalRect)],
    scale_factor: f32,
) {
    let th = theme::theme();
    let focused_pane_id = state.focused_pane_id(engine);

    let mut panes: Vec<PaneTabBarView> = Vec::new();
    {
        let ws = state.active_workspace(engine);
        for &(pane_id, pane_rect) in pane_rects {
            let pane = match ws.pane_layout().find_pane(pane_id) {
                Some(p) => p,
                None => continue,
            };
            let tab_attention_kind: Vec<Option<AttentionKind>> = pane
                .tabs
                .iter()
                .map(|t| {
                    let sids = t.all_surface_ids();
                    engine.attention_dominant_kind(&sids)
                })
                .collect();
            let tab_is_busy = compute_tab_is_busy(engine, &pane.tabs);
            panes.push(PaneTabBarView {
                pane_id,
                rect: pane_rect,
                tab_names: pane.tabs.iter().map(|t| t.display_name()).collect(),
                tab_icons: pane
                    .tabs
                    .iter()
                    .map(|t| {
                        let kind = engine
                            .find_surface_by_id(t.focused_surface)
                            .map(|s| s.kind())
                            .unwrap_or("terminal");
                        // kind→아이콘: registry 의 SurfaceKindDef.icon 이름을 host
                        // 아이콘 세트로 해석(하드코딩 없음). 미선언/미등록은 FILE.
                        engine
                            .surface_registry
                            .get(kind)
                            .and_then(|d| d.icon.clone())
                            .map(|n| icons::from_name(&n))
                            .unwrap_or(icons::FILE)
                    })
                    .collect(),
                tab_attention_kind,
                tab_is_busy,
                active_tab: pane.active_tab,
                is_focused: pane_id == focused_pane_id,
                scroll_offset: pane.tab_scroll_offset,
            });
        }
    }

    let appearance = &engine.settings.appearance;
    let tab_w = appearance.tab_width;
    let tab_font_size = appearance.tab_font_size;

    let drag = state.dialogs.tab_drag.as_ref().map(|d| TabDragView {
        pane_id: d.pane_id,
        tab_index: d.tab_index,
        current_x: d.current_x,
    });

    // switch-number overlay — `switch_overlay()` 스냅샷(사용자 입력 ModifiersChanged 로만
    // 갱신)에서 Tab 대상 + 그릴 focused pane id 를 읽는다. 그 pane 의 탭바에서만 키캡을
    // 그리므로 비-focused pane 에는 거짓 안내가 뜨지 않는다. 스냅샷은 egui raw_input 의
    // 사용자 키 입력만 반영 → IPC/CLI/에이전트로는 강제 표시될 수 없다(순수 미리보기).
    let switch_overlay_pane = state.switch_overlay().and_then(|o| match o.target {
        crate::adapters::ui::switch_overlay::SwitchTarget::Tab => o.pane_id,
        crate::adapters::ui::switch_overlay::SwitchTarget::Workspace
        | crate::adapters::ui::switch_overlay::SwitchTarget::Category => None,
    });

    let props = PaneTabBarsProps {
        theme: &th,
        kb: &engine.settings.keybindings,
        panes: &panes,
        scale_factor,
        tab_width: tab_w,
        tab_font_size,
        active_tab_indicator: appearance.active_tab_indicator,
        drag,
        switch_overlay_pane,
    };

    let output = draw_pane_tab_bars_view(ctx, &props);

    if let Some(h_phys) = output.measured_height_physical {
        state.tab_bar_height = h_phys;
    }

    apply_tab_bar_actions(state, engine, output.actions, &panes, tab_w, scale_factor);
}

/// 탭바 유래 액션을 상태에 반영한다. egui `Context` 비의존이라 단위 테스트 가능.
///
/// primary-click 계열 액션은 개별 처리 전에 그 pane 으로 `focused_pane` 을 먼저
/// 옮긴다([`TabBarAction::focus_target_pane`]) — 탭바 클릭은 그 pane 을 직접
/// 조작하는 사용자 행위이므로, 콘텐츠 영역 클릭(경로 B)과 대칭으로 focus 가 따라가는
/// 것이 일관된 동작이다.
pub fn apply_tab_bar_actions(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    actions: Vec<TabBarAction>,
    panes: &[PaneTabBarView],
    tab_w: f32,
    scale_factor: f32,
) {
    let separator_w: f32 = 1.0;

    for action in actions {
        if let Some(pane_id) = action.focus_target_pane() {
            state.active_workspace_mut(engine).focused_pane = pane_id;
        }
        match action {
            TabBarAction::SwitchTab { pane_id, tab_index } => {
                let mut to_wake: Vec<u32> = Vec::new();
                if let Some(pane) = state
                    .active_workspace_mut(engine)
                    .pane_layout_mut()
                    .find_pane_mut(pane_id)
                {
                    pane.active_tab = tab_index;
                    if let Some(tab) = pane.tabs.get(tab_index) {
                        to_wake = tab.deferred_surface_ids();
                    }
                }
                for sid in to_wake {
                    engine.ensure_surface_initialized(sid);
                }
            }
            TabBarAction::CloseTab { pane_id, tab_index } => {
                state.close_tab(engine, pane_id, tab_index);
            }
            TabBarAction::AddTab { pane_id: _ } => {
                if let Err(e) = state.add_tab(engine) {
                    tracing::warn!("add_tab failed: {e}");
                }
            }
            TabBarAction::RequestSplit { pane_id: _ } => {
                // 단축키(`split_pane_vertical`)와 동일 경로. focus 는 위에서 이미 대상
                // pane 으로 이동했다(cascade 가 새 pane 으로 다시 focus 이동).
                use crate::intent::Intent;
                use crate::model::SplitDirection;
                state.dispatch_intent(
                    Intent::SplitPane {
                        direction: SplitDirection::Vertical,
                    }
                    .from_user_shortcut("split_pane_vertical"),
                );
            }
            TabBarAction::OpenSearch { pane_id: _ } => {
                // 단축키(`find`)와 동일 경로 — 대상 pane 활성 surface 에 검색창을 연다.
                // focus 는 위에서 이미 대상 pane 으로 이동했다.
                open_search_for_focused_terminal(state, engine);
            }
            TabBarAction::ScrollLeft { pane_id } => {
                if let Some(pane) = state
                    .active_workspace_mut(engine)
                    .pane_layout_mut()
                    .find_pane_mut(pane_id)
                {
                    pane.tab_scroll_offset = (pane.tab_scroll_offset - tab_w).max(0.0);
                }
            }
            TabBarAction::ScrollRight { pane_id } => {
                if let Some(pane) = state
                    .active_workspace_mut(engine)
                    .pane_layout_mut()
                    .find_pane_mut(pane_id)
                {
                    pane.tab_scroll_offset += tab_w;
                }
            }
            TabBarAction::AutoScrollToActiveTab { pane_id, offset } => {
                apply_auto_scroll(state, engine, pane_id, offset);
            }
            // 빈 영역 클릭은 focus 이동이 전부다(탭 전환 없음) — 위 pre-match 에서 이미
            // 처리됐으므로 여기선 추가 작업이 없다.
            TabBarAction::FocusPane { pane_id: _ } => {}
            TabBarAction::OpenContextMenu {
                pane_id,
                tab_index,
                pos,
            } => {
                state.dialogs.pending_native_menu = Some(crate::state::PendingNativeMenu::Tab {
                    pane_id,
                    tab_index,
                    x: pos.x,
                    y: pos.y,
                });
            }
            TabBarAction::OpenPaneContextMenu { pane_id, pos } => {
                state.dialogs.pending_native_menu = Some(crate::state::PendingNativeMenu::Pane {
                    pane_id,
                    x: pos.x,
                    y: pos.y,
                });
            }
            TabBarAction::OpenNewTabButtonContextMenu { pane_id, pos } => {
                state.dialogs.pending_native_menu =
                    Some(crate::state::PendingNativeMenu::NewTabButton {
                        pane_id,
                        x: pos.x,
                        y: pos.y,
                    });
            }
            TabBarAction::DragStart { pane_id, tab_index } => {
                state.dialogs.tab_drag = Some(crate::state::TabDragState {
                    pane_id,
                    tab_index,
                    current_x: 0.0,
                });
            }
            TabBarAction::DragUpdate { pane_id, mouse_x } => {
                if let Some(ref mut drag) = state.dialogs.tab_drag
                    && drag.pane_id == pane_id
                {
                    drag.current_x = mouse_x;
                }
            }
            TabBarAction::DragEnd { pane_id } => {
                apply_drag_end(
                    state,
                    engine,
                    panes,
                    tab_w,
                    scale_factor,
                    separator_w,
                    pane_id,
                );
            }
        }
    }
}

/// [`TabBarAction::OpenSearch`] 적용 — `apply_tab_bar_actions`의 cognitive complexity 를
/// 낮추기 위해 분리.
///
/// `keybinding.rs`/`dispatch.rs`의 `kb.find` 게이트와 동일한 이유로 focused surface 가
/// Terminal 일 때만 처리한다 — `search_bar` popup 은 `find_terminal_by_id` 로만 동작해
/// 다른 kind 에서는 항상 빈 0/0 오버레이가 된다. 이 버튼은 활성 탭의 kind 와 무관하게
/// 항상 렌더되므로(pane 마다 고정 노출), 단축키 경로만 고치고 이 경로를 놓치면 같은
/// 버그가 마우스 클릭으로 그대로 재현된다.
fn open_search_for_focused_terminal(state: &mut AppState, engine: &mut crate::core::CoreState) {
    use crate::adapters::ui::popup::PopupScope;
    use crate::intent::{OpenPopupMode, UiIntent};
    if !matches!(
        state.focused_surface_type(engine),
        crate::state::FocusedSurfaceType::Terminal
    ) {
        return;
    }
    if state.popups.is_open("search_bar") {
        state.popups.set_focused("search_bar", true);
    } else if let Some(sid) = state.focused_surface_id(engine) {
        state.search.surface_id = sid;
        state.dispatch_intent(
            UiIntent::OpenPopup {
                id: "search_bar",
                mode: OpenPopupMode::AtTopOfScope(PopupScope::Surface(sid)),
            }
            .from_user_shortcut("find"),
        );
    }
}

/// [`TabBarAction::AutoScrollToActiveTab`] 적용 — view 가 계산한 보정 오프셋을
/// 그대로 pane 에 반영한다. `apply_tab_bar_actions` 의 cognitive complexity 를
/// 낮추기 위해 분리.
fn apply_auto_scroll(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    pane_id: u32,
    offset: f32,
) {
    if let Some(pane) = state
        .active_workspace_mut(engine)
        .pane_layout_mut()
        .find_pane_mut(pane_id)
    {
        pane.tab_scroll_offset = offset;
    }
}

/// [`TabBarAction::DragEnd`] 적용 — drag 중이던 탭을 실제 drop 위치로 옮긴다.
/// `apply_tab_bar_actions` 의 cognitive complexity 를 낮추기 위해 분리.
fn apply_drag_end(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    panes: &[PaneTabBarView],
    tab_w: f32,
    scale_factor: f32,
    separator_w: f32,
    pane_id: u32,
) {
    if let Some(drag) = state.dialogs.tab_drag.take()
        && drag.pane_id == pane_id
        && let Some(pane_info) = panes.iter().find(|i| i.pane_id == pane_id)
    {
        let pane_rect = pane_info.rect;
        let pane_logical_x = pane_rect.x.to_logical(scale_factor).value().round_ui();
        let pane_logical_w = pane_rect.width.to_logical(scale_factor).value().round_ui();
        let target = compute_drop_index(
            drag.current_x,
            pane_logical_x,
            pane_info.scroll_offset,
            pane_info.tab_names.len(),
            tab_w,
            separator_w,
            pane_logical_w,
        );
        // mirror 워크스페이스는 로컬 탭 순서 변경 대신 MoveTab 을 원격으로
        // forward 한다(로컬 실행은 원격 트리와 어긋남).
        if target != drag.tab_index {
            let mirror_op = engine
                .find_pane_by_id(pane_id)
                .and_then(|p| p.tabs.get(p.active_tab))
                .and_then(|t| t.focused_surface_id())
                .map(|sid| crate::ipc::stream::StructuralOp::MoveTab {
                    anchor_surface_id: sid,
                    from_index: drag.tab_index,
                    to_index: target,
                });
            if !state.forward_mirror_structural(engine, mirror_op, Vec::new())
                && let Some(pane) = state
                    .active_workspace_mut(engine)
                    .pane_layout_mut()
                    .find_pane_mut(pane_id)
            {
                pane.move_tab(drag.tab_index, target);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_target_pane_primary_click_actions_carry_pane_id() {
        let primary = [
            TabBarAction::SwitchTab {
                pane_id: 7,
                tab_index: 0,
            },
            TabBarAction::CloseTab {
                pane_id: 7,
                tab_index: 0,
            },
            TabBarAction::AddTab { pane_id: 7 },
            TabBarAction::RequestSplit { pane_id: 7 },
            TabBarAction::OpenSearch { pane_id: 7 },
            TabBarAction::ScrollLeft { pane_id: 7 },
            TabBarAction::ScrollRight { pane_id: 7 },
            TabBarAction::FocusPane { pane_id: 7 },
            TabBarAction::DragStart {
                pane_id: 7,
                tab_index: 0,
            },
        ];
        for action in primary {
            assert_eq!(
                action.focus_target_pane(),
                Some(7),
                "{action:?} 는 그 pane 으로 focus 를 옮겨야 한다"
            );
        }
    }

    #[test]
    fn focus_target_pane_context_menu_and_drag_update_end_actions_are_none() {
        let non_focus = [
            TabBarAction::OpenContextMenu {
                pane_id: 7,
                tab_index: 0,
                pos: egui::Pos2::ZERO,
            },
            TabBarAction::OpenPaneContextMenu {
                pane_id: 7,
                pos: egui::Pos2::ZERO,
            },
            TabBarAction::OpenNewTabButtonContextMenu {
                pane_id: 7,
                pos: egui::Pos2::ZERO,
            },
            TabBarAction::DragUpdate {
                pane_id: 7,
                mouse_x: 0.0,
            },
            TabBarAction::DragEnd { pane_id: 7 },
            TabBarAction::AutoScrollToActiveTab {
                pane_id: 7,
                offset: 0.0,
            },
        ];
        for action in non_focus {
            assert_eq!(
                action.focus_target_pane(),
                None,
                "{action:?} 는 focus 를 옮기면 안 된다(우클릭은 조작 commit 이 아니고, DragUpdate/DragEnd 는 DragStart 에서 이미 focus 가 이동한 뒤의 후속 프레임)"
            );
        }
    }

    #[test]
    fn compute_drop_index_first_slot() {
        let idx = compute_drop_index(100.0, 100.0, 0.0, 3, 120.0, 1.0, 400.0);
        assert_eq!(idx, 0);
    }

    #[test]
    fn compute_drop_index_middle_slot() {
        // mouse_x=281, pane_x=100 → content_x=181 → slot=181/121 ≈ 1.496 → round 1
        let idx = compute_drop_index(281.0, 100.0, 0.0, 3, 120.0, 1.0, 400.0);
        assert_eq!(idx, 1);
    }

    #[test]
    fn compute_drop_index_last_slot_clamped() {
        let idx = compute_drop_index(10_000.0, 100.0, 0.0, 3, 120.0, 1.0, 400.0);
        assert_eq!(idx, 2);
    }

    #[test]
    fn compute_drop_index_accounts_for_scroll() {
        let idx0 = compute_drop_index(100.0, 100.0, 0.0, 5, 120.0, 1.0, 400.0);
        let idx_scroll = compute_drop_index(100.0, 100.0, 121.0, 5, 120.0, 1.0, 400.0);
        assert_eq!(idx0, 0);
        assert_eq!(idx_scroll, 1);
    }

    fn test_theme() -> Theme {
        tasty_themes::mocha_fallback()
    }

    fn run_view(panes: Vec<PaneTabBarView>, drag: Option<TabDragView>) -> PaneTabBarsOutput {
        run_view_on(&egui::Context::default(), panes, drag)
    }

    /// [`run_view`] 와 동일하되 호출자가 `egui::Context` 를 직접 제공한다 — 활성
    /// 탭 추종 스크롤은 프레임 간 상태를 `ctx` persistent memory 에 추적하므로,
    /// "여러 프레임에 걸친 변화"를 검증하려면 같은 ctx 를 재사용해 여러 번 호출해야 한다.
    fn run_view_on(
        ctx: &egui::Context,
        panes: Vec<PaneTabBarView>,
        drag: Option<TabDragView>,
    ) -> PaneTabBarsOutput {
        let theme = test_theme();
        let kb = crate::settings::KeybindingSettings::default();
        let mut out = PaneTabBarsOutput::default();
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            let props = PaneTabBarsProps {
                theme: &theme,
                kb: &kb,
                panes: &panes,
                scale_factor: 1.0,
                tab_width: 160.0,
                tab_font_size: 12.0,
                active_tab_indicator: crate::settings::ActiveTabIndicator::default(),
                drag: drag.clone(),
                switch_overlay_pane: None,
            };
            out = draw_pane_tab_bars_view(ctx, &props);
        }));
        out
    }

    fn mk_pane(pane_id: u32, names: &[&str], active: usize, focused: bool) -> PaneTabBarView {
        mk_pane_w(pane_id, names, active, focused, 800.0)
    }

    fn mk_pane_w(
        pane_id: u32,
        names: &[&str],
        active: usize,
        focused: bool,
        width: f32,
    ) -> PaneTabBarView {
        let n = names.len();
        PaneTabBarView {
            pane_id,
            rect: PhysicalRect {
                x: PhysicalPx(0.0),
                y: PhysicalPx(0.0),
                width: PhysicalPx(width),
                height: PhysicalPx(600.0),
            },
            tab_names: names.iter().map(|s| s.to_string()).collect(),
            tab_icons: vec![icons::TERM; n],
            tab_attention_kind: vec![None; n],
            tab_is_busy: vec![false; n],
            active_tab: active,
            is_focused: focused,
            scroll_offset: 0.0,
        }
    }

    /// [`TabBarAction::AutoScrollToActiveTab`] 중 주어진 pane 대상인 것의 offset.
    fn auto_scroll_offset(out: &PaneTabBarsOutput, pane_id: u32) -> Option<f32> {
        out.actions.iter().find_map(|a| match *a {
            TabBarAction::AutoScrollToActiveTab { pane_id: p, offset } if p == pane_id => {
                Some(offset)
            }
            _ => None,
        })
    }

    // 아래 스크롤 보정 테스트들의 공통 지오메트리 (tab_w=160, separator_w=1,
    // plus_w=28, right_icons_w=56, arrow_w=20 — `draw_pane_tab_bars_view` 상수와 동일):
    // pane 폭 800 · 탭 8개 → content_w=1316, avail_w=744, needs_scroll,
    // viewport_w=704, max_scroll=612.

    #[test]
    fn switching_to_offscreen_tab_scrolls_it_into_view() {
        // 탭 8개, 화면에는 앞쪽 몇 개만 보이는 좁은 pane 폭. 마지막 탭(인덱스 7)으로
        // 전환 — 현재 뷰포트(scroll=0) 밖.
        let pane = mk_pane(1, &["A", "B", "C", "D", "E", "F", "G", "H"], 7, true);
        let out = run_view(vec![pane], None);

        let tab_w = 160.0;
        let separator_w = 1.0;
        let viewport_w = 704.0;
        let tab_start = 7.0 * (tab_w + separator_w);
        let tab_end = tab_start + tab_w;

        let offset = auto_scroll_offset(&out, 1).expect("offscreen 전환은 보정을 emit 해야 한다");
        assert!(offset <= tab_start);
        assert!(offset + viewport_w >= tab_end);
    }

    #[test]
    fn switching_to_first_tab_wraps_scroll_back_into_view() {
        // 마지막 탭에서 스크롤이 오른쪽 끝까지 밀려난 상태(scroll=max_scroll)에서
        // 첫 탭(인덱스 0)으로 wrap-around 전환 — 왼쪽으로 다시 보정돼야 한다.
        let ctx = egui::Context::default();
        let mut pane = mk_pane(1, &["A", "B", "C", "D", "E", "F", "G", "H"], 7, true);
        pane.scroll_offset = 612.0; // max_scroll
        run_view_on(&ctx, vec![pane], None);

        let mut pane = mk_pane(1, &["A", "B", "C", "D", "E", "F", "G", "H"], 0, true);
        pane.scroll_offset = 612.0;
        let out = run_view_on(&ctx, vec![pane], None);

        let offset = auto_scroll_offset(&out, 1).expect("wrap-around 전환도 보정을 emit 해야 한다");
        assert_eq!(offset, 0.0, "첫 탭은 뷰포트 좌측 끝(0)에서 보여야 한다");
    }

    #[test]
    fn active_tab_unchanged_does_not_override_manual_scroll() {
        // 같은 ctx 로 두 프레임 연속 렌더 — active_tab 도 pane 지오메트리도 바뀌지
        // 않았다면, 활성 탭(0)이 사용자가 화살표로 스크롤해 가려 놓은 상태(scroll=400,
        // 탭 0 은 뷰포트 밖)라도 보정을 강제하면 안 된다.
        let ctx = egui::Context::default();
        let pane = mk_pane(1, &["A", "B", "C", "D", "E", "F", "G", "H"], 0, true);
        run_view_on(&ctx, vec![pane], None);

        let mut pane = mk_pane(1, &["A", "B", "C", "D", "E", "F", "G", "H"], 0, true);
        pane.scroll_offset = 400.0;
        let out = run_view_on(&ctx, vec![pane], None);

        assert_eq!(
            auto_scroll_offset(&out, 1),
            None,
            "활성 탭이 그대로면 수동 스크롤 상태를 덮어쓰면 안 된다"
        );
    }

    #[test]
    fn pane_resize_reveals_correction_even_without_active_change() {
        // 1프레임: 넓은 pane(2000px) — 스크롤 불필요, 탭 7 전체가 이미 보임.
        // 2프레임: 같은 ctx, 같은 active_tab(7) 이지만 pane 이 800px 로 좁아져
        // 스크롤이 필요해짐 — active_tab 은 안 바뀌었어도 지오메트리 변화로 보정돼야 한다.
        let ctx = egui::Context::default();
        let wide = mk_pane_w(
            1,
            &["A", "B", "C", "D", "E", "F", "G", "H"],
            7,
            true,
            2000.0,
        );
        run_view_on(&ctx, vec![wide], None);

        let narrow = mk_pane_w(1, &["A", "B", "C", "D", "E", "F", "G", "H"], 7, true, 800.0);
        let out = run_view_on(&ctx, vec![narrow], None);

        let tab_w = 160.0;
        let separator_w = 1.0;
        let viewport_w = 704.0;
        let tab_start = 7.0 * (tab_w + separator_w);
        let tab_end = tab_start + tab_w;
        let offset =
            auto_scroll_offset(&out, 1).expect("resize 로 out-of-view 가 된 경우도 보정해야 한다");
        assert!(offset <= tab_start);
        assert!(offset + viewport_w >= tab_end);
    }

    #[test]
    fn viewport_that_already_shows_active_tab_emits_no_correction() {
        // 첫 탭(인덱스 0)이 활성이고 scroll=0 이면 이미 뷰포트 안 — 아무 보정도
        // 필요 없다(경계값: 스크롤 화살표/"+" 버튼 폭을 뺀 뒤에도 첫 탭은 항상 보임).
        let pane = mk_pane(1, &["A", "B", "C", "D", "E", "F", "G", "H"], 0, true);
        let out = run_view(vec![pane], None);
        assert_eq!(auto_scroll_offset(&out, 1), None);
    }

    #[test]
    fn view_idle_emits_no_actions() {
        let panes = vec![mk_pane(1, &["A", "B"], 0, true)];
        let out = run_view(panes, None);
        assert!(out.actions.is_empty());
        assert!(out.measured_height_physical.is_some());
    }

    #[test]
    fn view_measures_bar_height_for_first_pane() {
        let panes = vec![
            mk_pane(1, &["A"], 0, true),
            mk_pane(2, &["X", "Y"], 0, false),
        ];
        let out = run_view(panes, None);
        assert!(out.measured_height_physical.unwrap_or_default().value() > 0.0);
    }

    #[test]
    fn view_empty_panes_returns_default_output() {
        let out = run_view(vec![], None);
        assert!(out.actions.is_empty());
        assert!(out.measured_height_physical.is_none());
    }

    #[test]
    fn view_with_drag_does_not_panic() {
        let panes = vec![mk_pane(1, &["A", "B", "C"], 1, true)];
        let drag = Some(TabDragView {
            pane_id: 1,
            tab_index: 1,
            current_x: 240.0,
        });
        let out = run_view(panes, drag);
        // drag overlay 자체는 actions 를 추가하지 않음
        assert!(out.actions.is_empty());
    }

    fn test_engine() -> crate::core::CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        crate::core::CoreState::new(80, 24, waker).expect("engine")
    }

    fn tab_with_surface(sid: crate::model::SurfaceId) -> crate::model::Tab {
        let surface: Box<dyn crate::model::Surface> =
            Box::new(crate::model::EmptySurface::new(sid));
        crate::model::Tab::new_with_surface(1, "t".to_string(), surface)
    }

    /// mirror surface(로컬 PTY 없음, `set_mirror_surface_busy` 로만 채워짐)를 담은
    /// 탭도 `compute_tab_is_busy` 가 busy 로 판정해야 한다 — `busy_surfaces` 를 직접
    /// 참조하던 예전 코드는 mirror surface 를 절대 못 봐서 dot 이 안 떴던 버그의
    /// 회귀 테스트.
    #[test]
    fn compute_tab_is_busy_true_for_mirror_only_surface() {
        let mut engine = test_engine();
        let sid = 4242;
        engine.set_mirror_surface_busy(sid, true);
        let tabs = vec![tab_with_surface(sid)];

        let result = compute_tab_is_busy(&engine, &tabs);

        assert_eq!(result, vec![true]);
    }

    #[test]
    fn compute_tab_is_busy_false_when_idle() {
        let engine = test_engine();
        let sid = 4343;
        let tabs = vec![tab_with_surface(sid)];

        let result = compute_tab_is_busy(&engine, &tabs);

        assert_eq!(result, vec![false]);
    }
}
