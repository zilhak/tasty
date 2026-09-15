//! Painting and interaction for one tab slot.

use super::{PaneTabBarView, PaneTabBarsOutput, PaneTabBarsProps, TabBarAction};
use crate::adapters::ui::{icons, zoomed_px};
use crate::core::AttentionKind;
use tasty_type_geometry::length::LogicalPx;

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

/// busy 점과 탭 라벨 사이 여백. 종전에는 `let dot_pad: f32 = 6.0;` 인라인 리터럴이었다 —
/// 이름이 없으면 이 값이 점 지름(6)과 **같은 값이라는 사실**도, 그것이 우연이라는 사실도
/// 소스에서 안 읽힌다. 선언이 아니라 `let` 이라 선언 축 가드에도 안 걸렸다.
const TAB_BUSY_DOT_PAD: LogicalPx = LogicalPx(6.0);

/// Inputs shared by the tab slots in one clipped pane strip.
pub(super) struct TabRenderContext<'a, 'props> {
    pub props: &'a PaneTabBarsProps<'props>,
    pub info: &'a PaneTabBarView,
    pub painter: &'a egui::Painter,
    pub clip_rect: egui::Rect,
    pub bg: tasty_type_appearance::color::HexColor,
    pub separator_w: LogicalPx,
}

pub(super) fn draw_tab(
    ui: &mut egui::Ui,
    context: &TabRenderContext<'_, '_>,
    output: &mut PaneTabBarsOutput,
    i: usize,
    start_x: LogicalPx,
) -> LogicalPx {
    let TabRenderContext {
        props,
        info,
        painter,
        clip_rect,
        bg,
        separator_w,
    } = *context;
    let th = props.theme;
    let name = &info.tab_names[i];
    let mut x = start_x.value();
    let separator_w = separator_w.value();
    let tab_w = props.tab_width;
    let bar_h = th.tab_bar_height.value();
    let label_font_size = props.tab_font_size;
    let h_padding: f32 = 8.0;
    let active_indicator_h = th.tab_indicator_width.value();
    // 점 치수와 그 옆 여백도 배율을 탄다 — 같은 탭 안의 라벨 폰트와 탭바 높이가
    // `Theme` 에서 와서 이미 타므로, 점만 고정이면 1.2 에서 점이 상대적으로 쪼그라든다
    // (ADR-0126 "그릇과 내용은 같은 편이어야 한다"). 값 자체를 토큰으로 스냅하는 것은
    // 별개 물음이고 그쪽은 같은 ADR 이 스냅하지 말라고 정해 두었다.
    let dot_radius = zoomed_px(th, TAB_BUSY_DOT_SIZE).scaled(0.5);
    // 라벨이 점에 내주는 폭 = 지름 + 여백. 논리 길이로 더하고 여기서 한 번만 벗긴다.
    let dot_reserve = (zoomed_px(th, TAB_BUSY_DOT_SIZE) + zoomed_px(th, TAB_BUSY_DOT_PAD)).value();
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
    let tab_bg =
        if is_active && props.active_tab_indicator == crate::settings::ActiveTabIndicator::Fill {
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

    let tab_rect =
        egui::Rect::from_min_size(egui::pos2(x, clip_rect.min.y), egui::vec2(tab_w, bar_h));

    painter.rect_filled(tab_rect, 0.0, tab_bg);

    if is_active {
        use crate::settings::ActiveTabIndicator;
        match props.active_tab_indicator {
            ActiveTabIndicator::Underline => {
                let line_rect = egui::Rect::from_min_size(
                    egui::pos2(tab_rect.min.x, tab_rect.min.y),
                    egui::vec2(tab_w, active_indicator_h),
                );
                painter.rect_filled(line_rect, 0.0, th.accent_primary());
            }
            // Fill: 배경은 위에서 이미 bg_panel() 로 채움 — 추가 마커 없음.
            ActiveTabIndicator::Fill => {}
            ActiveTabIndicator::Dot => {
                // 탭 상단 중앙의 accent 점 마커.
                let r = zoomed_px(th, TAB_ACTIVE_DOT_SIZE).value() * 0.5;
                let center = egui::pos2(tab_rect.center().x, tab_rect.min.y + r * 2.0);
                painter.circle_filled(center, r, th.accent_primary());
            }
        }
    }

    // close 버튼 슬롯(우측 h_padding + 14px)을 비워두고 dot 은
    // 그 왼쪽에 둔다 (close 와 겹치지 않게).
    let dot_right = tab_rect.max.x - h_padding - 14.0;
    if is_busy {
        let dot_center = egui::pos2(dot_right - dot_radius.value(), tab_rect.center().y);
        let color: egui::Color32 = th.accent_success().into();
        painter.circle_filled(dot_center, dot_radius.value(), color);
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
    let switch_digit = crate::adapters::ui::switch_overlay::tab_keycap_for(
        props.kb,
        props.switch_overlay_pane,
        info.pane_id,
        i,
    );
    // 등장 페이드(90ms, motion-ui-fast) — 이 pane 의 오버레이
    // 활성 여부로 매 프레임 구동(키캡 미표시 프레임 포함 priming).
    let overlay_active = props.switch_overlay_pane == Some(info.pane_id);
    let fade = crate::adapters::ui::switch_overlay::appear_fade(
        ui.ctx(),
        th,
        info.pane_id,
        overlay_active,
    );
    if let Some(digit) = switch_digit {
        crate::adapters::ui::switch_overlay::paint_keycap(
            painter,
            th,
            icon_rect.center(),
            digit,
            is_active,
            fade,
        );
    } else {
        let icon = info.tab_icons.get(i).copied().unwrap_or(icons::FILE);
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
        text_right -= dot_reserve;
    }
    let available_w = (text_right - text_x).max(0.0);
    let font_id = egui::FontId::proportional(label_font_size);
    let final_galley = layout_tab_label(painter, name, font_id, text_color, LogicalPx(available_w));
    let text_y = tab_rect.center().y - final_galley.size().y / 2.0;
    painter.galley(egui::pos2(text_x, text_y), final_galley, text_color.into());

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
                egui::pos2(tab_rect.max.x - h_padding - cs / 2.0, tab_rect.center().y),
                egui::vec2(cs, cs),
            );
            let cr = ui.interact(
                close_rect,
                egui::Id::new(("tabclose", info.pane_id, i)),
                egui::Sense::click(),
            );
            if cr.hovered() {
                painter.rect_filled(close_rect, 2.0, th.active_overlay.to_egui_premultiplied());
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
                egui::Stroke::new(th.focus_ring_width.value(), th.accent_success()),
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
    LogicalPx(x)
}

/// Fit the label into the space left by the leading icon, busy dot and close slot.
fn layout_tab_label(
    painter: &egui::Painter,
    name: &str,
    font_id: egui::FontId,
    text_color: tasty_type_appearance::color::HexColor,
    available_width: LogicalPx,
) -> std::sync::Arc<egui::Galley> {
    let available_w = available_width.value();
    let galley = painter.layout_no_wrap(name.to_owned(), font_id.clone(), text_color.into());
    if galley.size().x > available_w {
        let mut truncated = name.to_owned();
        loop {
            truncated.pop();
            let candidate = format!("{truncated}…");
            let g = painter.layout_no_wrap(candidate.clone(), font_id.clone(), text_color.into());
            if g.size().x <= available_w || truncated.is_empty() {
                break g;
            }
        }
    } else {
        galley
    }
}
