//! 단일 선택 드롭다운. 트리거는 Theme로 그리고 열린 항목 목록은 egui 팝업 안의 공용 옵션 행이다.

use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;

/// 드롭다운. `selected` 는 `options` 인덱스. 선택이 바뀌면 `true`.
pub fn select(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: &str,
    selected: &mut usize,
    options: &[&str],
    width: f32,
    enabled: bool,
) -> bool {
    select_rect(ui, theme, id_salt, selected, options, width, enabled).0
}

/// [`select`] 와 같고 트리거가 차지한 rect 도 돌려준다. 행 안 컨트롤의 높이·위치를 맞추는 view 가 쓴다.
pub(crate) fn select_rect(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: &str,
    selected: &mut usize,
    options: &[&str],
    width: f32,
    enabled: bool,
) -> (bool, egui::Rect) {
    let (picked, rect) = select_impl(
        ui,
        theme,
        id_salt,
        Some(*selected),
        options,
        None,
        width,
        enabled,
    );
    match picked {
        Some(i) => {
            *selected = i;
            (true, rect)
        }
        None => (false, rect),
    }
}

/// 미선택 상태에서는 placeholder 색과 문구를 사용한다.
/// 메뉴의 안내 행을 눌러도 값을 선택한 것으로 보지 않으며 실제 선택이 바뀌면 true를 반환한다.
// reason: `select` 와 같은 인자 모양에 placeholder 하나를 더한 것이다 — 묶으면 두 드롭다운의
// 호출 모양이 갈린다.
#[allow(clippy::too_many_arguments)]
pub fn select_or_placeholder(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: &str,
    selected: &mut Option<usize>,
    options: &[&str],
    placeholder: &str,
    width: f32,
    enabled: bool,
) -> bool {
    match select_impl(
        ui,
        theme,
        id_salt,
        *selected,
        options,
        Some(placeholder),
        width,
        enabled,
    )
    .0
    {
        Some(i) => {
            *selected = Some(i);
            true
        }
        None => false,
    }
}

/// 두 공개 드롭다운이 공유하는 구현. 새로 선택한 인덱스(있으면)와 트리거 rect 를 반환한다.
// reason: 공개 함수 둘의 인자를 그대로 받는 내부 원문이다.
#[allow(clippy::too_many_arguments)]
fn select_impl(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: &str,
    selected: Option<usize>,
    options: &[&str],
    placeholder: Option<&str>,
    width: f32,
    enabled: bool,
) -> (Option<usize>, egui::Rect) {
    let pad_x = theme.select_padding_x().value();
    let body = theme.select_font_size().value();
    let chevron_room = theme.select_chevron_room().value();

    let (rect, resp) = alloc_trigger(ui, theme, width, enabled);
    // disabled 글자·글리프는 opacity 없이 disabled ink를 쓴다.
    let dim = |c: egui::Color32| {
        if enabled {
            c
        } else {
            theme.state_disabled_fg().to_egui()
        }
    };

    // 직접 키를 처리하지 않으므로 MultiSelect와 같은 포커스 테두리는 제공하지 않는다.
    let border = if enabled && resp.hovered() {
        theme.border_strong()
    } else {
        theme.select_border()
    };
    paint_trigger_box(ui.painter(), theme, rect, border, enabled);
    // 문구가 테두리와 화살표를 넘지 않도록 줄인다.
    let (label, label_fg) = match (selected, placeholder) {
        (None, Some(p)) => (p, theme.text_placeholder()),
        (s, _) => (
            s.and_then(|i| options.get(i)).copied().unwrap_or(""),
            theme.select_fg(),
        ),
    };
    let text_max_width = (rect.right() - chevron_room - (rect.left() + pad_x)).max(0.0);
    let mut job = egui::text::LayoutJob::simple_singleline(
        label.to_owned(),
        egui::FontId::proportional(body),
        egui::Color32::PLACEHOLDER,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(text_max_width);
    let galley = ui.fonts(|f| f.layout_job(job));
    let text_pos = egui::pos2(
        rect.left() + pad_x,
        rect.center().y - galley.rect.height() * 0.5,
    );
    ui.painter()
        .galley(text_pos, galley, dim(label_fg.to_egui()));
    let cx = rect.right() - chevron_room * 0.5;
    let ch = dim(theme.select_chevron_fg().to_egui());
    paint_chevron(ui.painter(), egui::pos2(cx, rect.center().y), ch, false);

    let popup_id = ui.make_persistent_id(("tasty_select", id_salt));
    if enabled && resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }

    let mut picked = None;
    tasty_egui_theme::with_popover_frame(ui, theme, |ui| {
        egui::popup_below_widget(
            ui,
            popup_id,
            &resp,
            egui::PopupCloseBehavior::CloseOnClick,
            |ui| {
                ui.set_min_width(width);
                // 옵션은 공용 MenuItem 옵션 행이다. 현재 값만 selected(글자 + 체크, 채움 없음)이고
                // 행은 다른 메뉴처럼 붙인다.
                ui.spacing_mut().item_spacing.y = 0.0;
                if let (None, Some(p)) = (selected, placeholder) {
                    // sentinel — 값이 아니므로 selected 로 그리지 않는다. 누르면 메뉴만 닫힌다.
                    let _sentinel = crate::menu_option(ui, theme, p, false);
                }
                for (i, opt) in options.iter().enumerate() {
                    if crate::menu_option(ui, theme, opt, selected == Some(i)).clicked()
                        && selected != Some(i)
                    {
                        picked = Some(i);
                    }
                }
            },
        )
    });
    (picked, rect)
}

/// 단일·다중 선택 필드가 같은 높이와 입력 영역 계산을 사용한다.
pub(crate) fn alloc_trigger(
    ui: &mut egui::Ui,
    theme: &Theme,
    width: f32,
    enabled: bool,
) -> (egui::Rect, egui::Response) {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    ui.allocate_exact_size(egui::vec2(width, theme.select_height().value()), sense)
}

/// 단일·다중 선택 필드가 공유하는 배경·반경·선 굵기. 상태별 테두리색은 호출자가 정한다.
pub(crate) fn paint_trigger_box(
    painter: &egui::Painter,
    theme: &Theme,
    rect: egui::Rect,
    border: HexColor,
    enabled: bool,
) {
    // disabled 트리거는 상태별 테두리 대신 disabled 상자 role을 쓴다.
    let (fill, edge) = if enabled {
        (theme.select_bg(), border)
    } else {
        (theme.state_disabled_fill(), theme.state_disabled_border())
    };
    painter.rect(
        rect,
        theme.select_radius().value(),
        fill.to_egui(),
        egui::Stroke::new(theme.border_width.value(), edge.to_egui()),
        egui::StrokeKind::Inside,
    );
}

pub(crate) fn paint_chevron(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: egui::Color32,
    up: bool,
) {
    /// chevron 반폭(px).
    const HALF_W: f32 = 4.0;
    /// 꼭짓점이 중심선에서 내려가는 깊이(px).
    const DEPTH: f32 = 2.5;
    /// 양 끝이 중심선에서 올라가는 높이(px).
    const RISE: f32 = 2.0;
    /// chevron 선 굵기(px).
    const STROKE_W: f32 = 1.5;

    let dir = if up { -1.0 } else { 1.0 };
    painter.add(egui::Shape::line(
        vec![
            egui::pos2(center.x - HALF_W, center.y - RISE * dir),
            egui::pos2(center.x, center.y + DEPTH * dir),
            egui::pos2(center.x + HALF_W, center.y - RISE * dir),
        ],
        egui::Stroke::new(STROKE_W, color),
    ));
}
