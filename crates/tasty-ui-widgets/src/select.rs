//! `Select` — 드롭다운 (디자인 `components/forms/Select`).
//!
//! 닫힌 트리거(height 28, surface-raised, border-default, 우측 chevron)는 디자인
//! 토큰 그대로. 열린 메뉴는 egui popup 으로 옵션을 나열한다(메뉴 항목 스타일은
//! 근사 — MenuItem 위젯과 통합 여지). `selected` 변경 시 `true` 반환.

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
    let height = theme.select_height().value();
    let pad_x = theme.select_padding_x().value();
    let radius = theme.select_radius().value();
    let bw = theme.border_width.value();
    let body = theme.select_font_size().value();
    let chevron_room = theme.select_chevron_room().value();

    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), sense);
    // disabled 디밍은 `opacity_disabled`(0.5) 공통 톤이다 — 이 위젯만의 값이 아니다.
    let dim = |c: egui::Color32| {
        if enabled {
            c
        } else {
            c.gamma_multiply(theme.opacity_disabled())
        }
    };

    // 트리거 박스. hover border 는 대응 select component 토큰 없어 semantic 유지.
    let border = if enabled && resp.hovered() {
        theme.border_strong()
    } else {
        theme.select_border()
    };
    ui.painter().rect(
        rect,
        radius,
        dim(theme.select_bg().to_egui()),
        egui::Stroke::new(bw, dim(border.to_egui())),
        egui::StrokeKind::Inside,
    );
    // 현재 값 — 가용 폭(좌 padding ~ chevron 앞) 초과 시 말줄임(truncate_at_width)으로
    // border/chevron 침범 방지.
    let label = options.get(*selected).copied().unwrap_or("");
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
        .galley(text_pos, galley, dim(theme.select_fg().to_egui()));
    // chevron (▾) — 우측.
    let cx = rect.right() - chevron_room * 0.5;
    let ch = dim(theme.select_chevron_fg().to_egui());
    paint_chevron(ui.painter(), egui::pos2(cx, rect.center().y), ch, false);

    let popup_id = ui.make_persistent_id(("tasty_select", id_salt));
    if enabled && resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }

    let mut changed = false;
    egui::popup_below_widget(
        ui,
        popup_id,
        &resp,
        egui::PopupCloseBehavior::CloseOnClick,
        |ui| {
            ui.set_min_width(width);
            for (i, opt) in options.iter().enumerate() {
                if ui.selectable_label(i == *selected, *opt).clicked() && i != *selected {
                    *selected = i;
                    changed = true;
                }
            }
        },
    );
    changed
}

/// Select 계열 트리거의 chevron 글리프 — 꺾은선 2 segment.
///
/// 대응 chevron component 토큰이 없어(`select_chevron_room`/`-offset` 은 *자리*,
/// `select_chevron_fg` 는 *색*만 준다) 글리프 자체의 반폭·깊이·선굵기는 이 함수가
/// 소유하는 명명 상수다. [`select`] 와 [`crate::multi_select`] 가 공유해, 같은
/// 계열 트리거의 chevron 이 한 곳에서만 정의되게 한다.
///
/// `up = true` 면 위를 향한다(다중선택의 열린 상태 표시). [`select`] 는 네이티브
/// `<select>` 미러라 항상 아래를 향한다.
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
