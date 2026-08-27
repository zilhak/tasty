//! `MultiSelect` — 다중선택 드롭다운 (디자인 `components/forms/Select` 계열).
//!
//! 닫힌 트리거는 [`crate::select`] 와 **같은 토큰**(`select_height` / `select_padding_x`
//! / `select_radius` / `select_font_size` / `select_chevron_room` / `select_fg` /
//! `select_chevron_fg`)을 쓴다 — 같은 폼에 나란히 놓였을 때 높이·보더·폰트가 어긋나면
//! 안 되기 때문이다. 다른 것은 셋뿐이다.
//!
//! 1. 팝업 본문이 라디오(`selectable_label`)가 아니라 [`crate::checkbox`] 행이다.
//! 2. close behavior 가 `CloseOnClickOutside` 다 — 항목을 **연속으로** 토글해야 하므로
//!    하나 눌렀다고 닫히면 쓸 수 없다.
//! 3. 트리거 텍스트가 값 하나가 아니라 **요약 라벨**이다(0개 / N개 / 전부 3갈래).
//!
//! **요약 문구는 이 crate 가 소유하지 않는다.** 위젯 crate 는 i18n 을 의존하지 않아
//! 번역을 가질 수 없다 — 호출자가 [`MultiSelectLabels`] 로 세 문구를 주입하고, 어느
//! 갈래인지 고르는 것만 위젯이 한다([`multi_select_summary`]).

use tasty_type_appearance::theme::Theme;

use crate::select::paint_chevron;

/// 트리거 요약 라벨 3갈래. 문구는 호출자(=i18n 을 가진 쪽)가 주입한다.
///
/// 위젯은 어느 갈래인지만 고른다 — 판정 규칙은 [`multi_select_summary`] 참고.
#[derive(Clone, Copy, Debug)]
pub struct MultiSelectLabels<'a> {
    /// 아무것도 선택되지 않았을 때. 옵션이 0 개일 때도 이 문구다.
    pub none: &'a str,
    /// 일부만 선택됐을 때. **`{}` 가 선택 개수로 치환된다**(본체 `t_fmt` 와 같은 규약).
    pub some: &'a str,
    /// 전부 선택됐을 때. 개수를 쓰지 않는 별도 문구라 치환 자리가 없다.
    pub all: &'a str,
}

/// 선택 상태 → 트리거에 그릴 요약 문구.
///
/// 갈래 판정은 **개수만** 본다: 0 개면 `none`, 전부면 `all`, 그 사이면 `some` 의 `{}`
/// 를 개수로 치환. 옵션이 0 개인 경우는 "전부 선택" 이 아니라 `none` 이다 — 빈 목록에
/// "전부" 라고 쓰면 고를 게 있다는 오해를 준다.
///
/// 트리거 밖(예: 버튼 툴팁)에서 같은 문구가 필요할 수 있어 공개한다.
pub fn multi_select_summary(labels: &MultiSelectLabels<'_>, selected: &[bool]) -> String {
    let n = selected.iter().filter(|s| **s).count();
    if n == 0 {
        labels.none.to_owned()
    } else if n == selected.len() {
        labels.all.to_owned()
    } else {
        labels.some.replacen("{}", &n.to_string(), 1)
    }
}

/// [`multi_select`] 팝업의 `egui::Id`.
///
/// 소비자가 팝업을 직접 여닫거나 열림 여부를 물어야 할 때 쓴다. **id 규약을 복제하지
/// 말고 이 헬퍼를 쓸 것** — 규약이 바뀌어도 호출부가 조용히 어긋나지 않는다.
pub fn multi_select_popup_id(ui: &egui::Ui, id_salt: &str) -> egui::Id {
    ui.make_persistent_id(("tasty_multi_select", id_salt))
}

/// 팝업 프레임이 본문 바깥에 더하는 가로 여유 — inner/outer margin 양쪽 + 보더 양쪽.
///
/// max-width 는 **메뉴 상자 전체**의 상한이므로 본문 폭 상한을 구하려면 이만큼 빼야
/// 한다(`egui::popup_below_widget` 이 `Frame::popup` 을 쓰는 것과 같은 계산).
fn popup_chrome_width(ui: &egui::Ui) -> f32 {
    let frame = egui::Frame::popup(ui.style());
    frame.total_margin().sum().x + 2.0 * frame.stroke.width
}

/// 다중선택 드롭다운. `selected` 는 `options` 와 **같은 길이**의 on/off 배열이다
/// (짧으면 남는 옵션은 그려지지 않는다 — 호출자가 길이를 맞춰야 한다).
///
/// 선택이 하나라도 바뀌면 `true` 를 반환한다([`crate::select`] 와 대칭).
#[allow(clippy::too_many_arguments)] // reason: select 와 대칭인 시그니처 + labels 주입. 인위적 그룹핑 불필요
pub fn multi_select(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: &str,
    selected: &mut [bool],
    options: &[&str],
    labels: &MultiSelectLabels<'_>,
    width: f32,
    enabled: bool,
) -> bool {
    let height = theme.select_height().value();
    let pad_x = theme.select_padding_x().value();
    let radius = theme.select_radius().value();
    let bw = theme.border_width.value();
    let body = theme.select_font_size().value();
    let chevron_room = theme.select_chevron_room().value();

    let popup_id = multi_select_popup_id(ui, id_salt);
    let open = ui.memory(|m| m.is_popup_open(popup_id));

    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), sense);
    let dim = |c: egui::Color32| {
        if enabled {
            c
        } else {
            c.gamma_multiply(theme.opacity_disabled())
        }
    };

    // 트리거 박스 — select 와 동일. 열려 있는 동안은 focus 보더로 "이 컨트롤이 지금
    // 활성" 을 유지한다(팝업이 트리거를 덮지 않으므로 신호가 남아야 한다).
    let border = if !enabled {
        theme.select_border()
    } else if open {
        theme.select_border_focus()
    } else if resp.hovered() {
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

    // 요약 라벨 — 가용 폭(좌 padding ~ chevron 앞) 초과 시 말줄임으로 border/chevron
    // 침범 방지(select 와 동일 규칙). 0 개 선택은 placeholder 톤으로 "아직 안 골랐다" 를
    // 값과 구분한다.
    let summary = multi_select_summary(labels, selected);
    let none_selected = !selected.iter().any(|s| *s);
    let fg = if none_selected {
        theme.text_placeholder()
    } else {
        theme.select_fg()
    };
    let text_max_width = (rect.right() - chevron_room - (rect.left() + pad_x)).max(0.0);
    let mut job = egui::text::LayoutJob::simple_singleline(
        summary,
        egui::FontId::proportional(body),
        egui::Color32::PLACEHOLDER,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(text_max_width);
    let galley = ui.fonts(|f| f.layout_job(job));
    let text_pos = egui::pos2(
        rect.left() + pad_x,
        rect.center().y - galley.rect.height() * 0.5,
    );
    ui.painter().galley(text_pos, galley, dim(fg.to_egui()));

    // chevron — 열려 있으면 위를 향한다(단일 select 는 네이티브 미러라 항상 아래).
    let cx = rect.right() - chevron_room * 0.5;
    let ch = dim(theme.select_chevron_fg().to_egui());
    paint_chevron(ui.painter(), egui::pos2(cx, rect.center().y), ch, open);

    if enabled && resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }

    // 메뉴 폭은 팝업을 그리기 **전에** 확정한다. egui 팝업 본문은 justified 세로
    // 레이아웃이라 행이 언제나 가용 폭을 채우고, 그 가용 폭은 다시 지난 프레임의 내용
    // 폭에서 온다 — 여기에 말줄임(가용 폭 기준)까지 얹히면 서로를 물어 폭이 트리거에
    // 고정되거나 max 에 붙어버린다. 가장 넓은 행을 직접 재서 min/max 를 같은 값으로
    // 못박아 그 되먹임을 끊는다.
    //
    // 규칙은 디자인 판정 그대로 — min = 트리거 폭, 내용에 맞춰 max-width 까지 확장.
    // CSS 처럼 min 이 max 를 이긴다(트리거가 max 보다 넓으면 트리거를 따른다).
    let widest_row = options
        .iter()
        .take(selected.len())
        .map(|opt| crate::checkbox_width(ui, theme, opt))
        .fold(0.0_f32, f32::max);
    let menu_chrome = popup_chrome_width(ui);
    let menu_min = width;
    let menu_max = (theme.multiselect_menu_max_width().value() - menu_chrome).max(menu_min);
    let menu_width = widest_row.clamp(menu_min, menu_max);

    let mut changed = false;
    egui::popup_below_widget(
        ui,
        popup_id,
        &resp,
        // 다중선택의 핵심 — 항목 하나를 눌러도 닫히지 않아야 연속 토글이 된다.
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(menu_width);
            ui.set_max_width(menu_width);
            // 행 사이 간격은 메뉴 행 리듬(menu_item)이 아니라 checkbox 목록 리듬을
            // 따른다 — 행 자체가 checkbox 라 spacing_xs 가 디자인 Checkbox 그룹과 같다.
            ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
            // 옵션이 많으면 max-height 에서 멈추고 내부 스크롤(AutoComplete 드롭다운과
            // 같은 규칙·같은 높이). 적으면 shrink-to-fit 이라 짧은 목록은 그대로다.
            egui::ScrollArea::vertical()
                .id_salt(("tasty_multi_select_list", id_salt))
                .max_height(theme.multiselect_menu_max_height().value())
                .auto_shrink([true, true])
                .show(ui, |ui| {
                    for (i, opt) in options.iter().enumerate() {
                        let Some(flag) = selected.get_mut(i) else {
                            break;
                        };
                        if crate::checkbox(ui, theme, flag, opt, true).changed() {
                            changed = true;
                        }
                    }
                });
        },
    );
    changed
}
