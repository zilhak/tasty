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
//!
//! 메뉴 최상단의 일괄 토글 행([`MultiSelectAllToggle`])은 **opt-in** 이다 — 기본은
//! 없음이고, 켠 호출부만 "전부 선택 / 전부 해제" 행 하나와 구분선을 얻는다. 문구는
//! 같은 이유로 호출자가 주입한다.
//!
//! 마우스 없이도 끝까지 조작된다 — 닫힌 트리거에서 `↓`/`Enter`/`Space` 로 열고,
//! `↑`/`↓`/`Home`/`End` 로 active 행을 옮기고(비활성 행은 건너뛴다), `Space`/`Enter`
//! 로 **닫지 않고** 토글하고, `Esc` 로 닫는다(포커스는 트리거에 남는다). `Tab` 은
//! 닫고 다음 위젯으로 간다. active 행은 hover 워시가 아니라 `surface_active` 배경으로
//! 표시된다 — 키보드 커서 전용 신호다.

use tasty_type_appearance::theme::Theme;

use crate::keyboard_cursor::{edge_enabled, row_enabled, step_active};
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

/// 메뉴 최상단 일괄 토글 행의 문구 2 갈래. 디자인 `MultiSelect` 의 `allToggle` +
/// `allLabel` / `clearLabel` 세 prop 을 Rust 로 옮긴 것이다.
///
/// jsx 는 `allToggle: bool` 과 기본값이 붙은 문구 prop 을 따로 두지만, Rust 에는 기본
/// 인자가 없어 bool 로 켜면 호출부가 늘 문구 2 개를 함께 넘겨야 한다 — 끄는 쪽(현재
/// 실 소비처 전부)이 쓰지도 않을 더미 문자열을 지어내게 된다. 그래서 "켬 + 문구" 를
/// 한 값으로 묶어 [`multi_select`] 가 `Option` 으로 받는다: `None` 이 곧 off 이고,
/// `Some` 이면 문구가 반드시 있다(둘이 어긋날 수 없다).
///
/// 어느 문구를 쓸지 고르는 판정만 위젯이 한다 — [`MultiSelectLabels`] 와 같은 규약.
#[derive(Clone, Copy, Debug)]
pub struct MultiSelectAllToggle<'a> {
    /// 아직 전부 켜지지 않았을 때. 누르면 (토글 가능한) 모든 옵션이 켜진다.
    pub select_all: &'a str,
    /// 이미 전부 켜져 있을 때. 누르면 (토글 가능한) 모든 옵션이 꺼진다.
    pub clear_all: &'a str,
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

/// 키보드 커서(active 행)의 저장 키 — 팝업 id 에서 파생해 인스턴스마다 독립이다.
///
/// 위젯 함수는 상태를 소유하지 않으므로 값은 `Memory::data` 에 둔다(팝업 열림 여부를
/// `Memory` 에 두는 것과 같은 이유).
fn active_row_id(popup_id: egui::Id) -> egui::Id {
    popup_id.with("active_row")
}

/// 열린 팝업이 이번 프레임에 가져가는 키.
#[derive(Default, Clone, Copy)]
struct NavKeys {
    up: bool,
    down: bool,
    home: bool,
    end: bool,
    /// `Space` / `Enter` — active 행 토글. 팝업은 닫지 않는다.
    toggle: bool,
    /// `Esc` — 닫기.
    close: bool,
    /// `Tab` — 닫고 다음 위젯으로.
    tab: bool,
}

/// 열린 팝업이 쓸 키를 입력 큐에서 **걷어낸다**. 소비가 핵심이다.
///
/// 1. egui 는 포커스된 clickable 위젯의 `Space`/`Enter` 를 클릭으로 승격시킨다
///    (fake primary click). 남겨두면 "active 행 토글" 이 트리거 클릭 = "팝업 닫기" 로
///    둔갑하므로, 트리거를 allocate 하기 **전에** 불러야 한다.
/// 2. `Esc` 는 상위 팝업·모달도 듣고 있다. 여기서 먹지 않으면 드롭다운만 닫히는 대신
///    부모 창까지 함께 닫힌다.
///
/// `Tab` 만 예외로 소비하지 않는다 — 팝업은 닫되 포커스 이동은 egui 기본 동작 몫이다.
fn take_nav_keys(ui: &egui::Ui) -> NavKeys {
    let none = egui::Modifiers::NONE;
    ui.input_mut(|i| NavKeys {
        up: i.consume_key(none, egui::Key::ArrowUp),
        down: i.consume_key(none, egui::Key::ArrowDown),
        home: i.consume_key(none, egui::Key::Home),
        end: i.consume_key(none, egui::Key::End),
        // `|` 는 단락 평가가 없다 — 둘 다 반드시 소비해야 한 쪽이 남아 클릭으로 새지 않는다.
        toggle: i.consume_key(none, egui::Key::Space) | i.consume_key(none, egui::Key::Enter),
        close: i.consume_key(none, egui::Key::Escape),
        tab: i.key_pressed(egui::Key::Tab),
    })
}

/// 이번 프레임에 팝업 상자 **안쪽**에서 포인터 press 가 났는가.
///
/// egui 는 포커스된 위젯 밖에서 press 가 나면 그 포커스를 회수한다 — 행을 마우스로
/// 누르는 순간 트리거가 키보드 포커스를 잃어 이후 키 조작이 죽는다. 팝업이 열린 채면
/// 되찾아 주기 위한 판정이라 행·구분선·스크롤바를 가리지 않는다.
fn pressed_inside_popup(ui: &egui::Ui, popup_id: egui::Id) -> bool {
    let Some(rect) = ui.memory(|m| m.area_rect(popup_id)) else {
        return false;
    };
    ui.input(|i| {
        i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| rect.contains(p))
    })
}

/// 실제로 그려지는 행 개수 — `options` 와 `selected` 중 짧은 쪽이 목록을 끊는다.
fn row_count(selected: &[bool], options: &[&str]) -> usize {
    options.len().min(selected.len())
}

/// 일괄 토글 행이 지금 "전부 해제"(=이미 전부 켜짐) 인가.
///
/// **토글 가능한 행만** 센다. jsx 는 disabled 행까지 포함해 판정하지만, 그러면 끌 수
/// 없는 disabled 행 하나가 꺼져 있는 것만으로 라벨이 영원히 "전부 선택" 에 묶여
/// 눌러도 아무 일이 없는 상태가 된다. 여기서는 "누르면 실제로 바뀌는 것들" 만 보므로
/// 라벨이 늘 다음 동작을 정확히 예고한다. 마스크가 없는 보통의 호출부에서는 jsx 와
/// 결과가 같다.
fn all_rows_on(selected: &[bool], options: &[&str], disabled: Option<&[bool]>) -> bool {
    let n = row_count(selected, options);
    let mut any = false;
    for (i, on) in selected.iter().enumerate().take(n) {
        if !row_enabled(disabled, i) {
            continue;
        }
        any = true;
        if !on {
            return false;
        }
    }
    any
}

/// 메뉴 최상단 accent 액션 행. 옵션 행(checkbox)과 **같은 행 높이·같은 폰트**를 써
/// 리듬을 맞추고, 클릭 어포던스는 hover wash 로 준다(디자인 `.tasty-mselect__all`).
///
/// checkbox 가 아니다 — 부분 선택을 나타낼 indeterminate 상태가 이 디자인 시스템에
/// 없기 때문이다(디자인 판정).
fn all_toggle_row(ui: &mut egui::Ui, theme: &Theme, label: &str) -> egui::Response {
    let body = theme.font_size_body.value();
    let width = ui.available_width();
    // 옵션 행과 같은 규칙으로 말줄임 — 메뉴 폭이 max-width 에 걸려도 보더를 넘지 않는다.
    let mut job = egui::text::LayoutJob::simple_singleline(
        label.to_owned(),
        egui::FontId::proportional(body),
        egui::Color32::PLACEHOLDER,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    let galley = ui.fonts(|f| f.layout_job(job));
    // checkbox 행의 높이 계산과 같다(박스 16 과 글자 중 큰 쪽) — 두 행이 한 리듬으로
    // 읽혀야 하므로 값을 새로 만들지 않고 같은 식을 쓴다.
    let height = theme.checkbox_size().value().max(galley.rect.height());
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            theme.menu_item_radius().value(),
            theme.menu_item_bg_hover().to_egui_premultiplied(),
        );
    }
    let pos = egui::pos2(rect.left(), rect.center().y - galley.rect.height() * 0.5);
    ui.painter()
        .galley(pos, galley, theme.accent_primary().to_egui());
    resp
}

/// 일괄 토글 행이 라벨을 자르지 않고 그리는 데 필요한 폭 — 앞에 박스가 없어 글자
/// 폭 그대로다(옵션 행의 [`crate::checkbox_width`] 에 대응).
fn all_toggle_width(ui: &egui::Ui, theme: &Theme, label: &str) -> f32 {
    ui.fonts(|f| {
        f.layout_no_wrap(
            label.to_owned(),
            egui::FontId::proportional(theme.font_size_body.value()),
            egui::Color32::PLACEHOLDER,
        )
    })
    .rect
    .width()
}

/// 일괄 토글 행 아래 구분선 — 1px, 위아래 여백은 목록 행 간격(`spacing_xs`)이 준다
/// (디자인 `.tasty-mselect__sep` 의 `margin: space-xs 0` 과 같은 결과).
fn all_toggle_separator(ui: &mut egui::Ui, theme: &Theme) {
    let bw = theme.border_width.value();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), bw), egui::Sense::hover());
    // separator 는 도출 overlay(알파 ~8%)라 배경 위에 얹어야 보인다 — 다른 divider
    // 소비처(`listctrl` / `drilldown`)와 같이 premultiplied 로 칠한다.
    ui.painter()
        .rect_filled(rect, 0.0, theme.separator.to_egui_premultiplied());
}

/// 다중선택 드롭다운. `selected` 는 `options` 와 **같은 길이**의 on/off 배열이다
/// (짧으면 남는 옵션은 그려지지 않는다 — 호출자가 길이를 맞춰야 한다).
///
/// `disabled` 는 **행 단위** 비활성 마스크다. `disabled[i]` 가 `true` 면 그 옵션 행은
/// 보이되 흐려지고(`opacity_disabled`) 클릭해도 토글되지 않는다 — 목록에서 아예
/// 빼버리는 것과 다르다(고를 수 없다는 사실 자체를 보여준다). `None` 이거나 길이가
/// 모자라 인덱스가 비면 그 행은 활성이다.
///
/// 컨트롤 전체를 끄는 `enabled` 와는 층이 다르다 — `enabled=false` 면 트리거부터
/// 죽어 팝업이 열리지 않으므로 `disabled` 마스크는 애초에 관측되지 않는다.
///
/// `all_toggle` 은 메뉴 최상단 일괄 토글 행의 **opt-in** 이다(디자인 `allToggle`,
/// 기본 off). `None` 이면 액션 행도 구분선도 그려지지 않아 렌더가 예전과 동일하고,
/// `Some` 이면 accent 액션 행 1 개 + 구분선이 옵션 목록 **위**에 붙는다(목록이
/// 스크롤돼도 따라 움직이지 않는다). 옵션이 적을 때(디자인 권고: 8 개 미만) 끄는
/// 판단은 호출자 몫이다 — 위젯은 개수로 스스로 억제하지 않는다.
///
/// 일괄 토글은 `disabled` 마스크를 **양방향으로 존중한다** — 켤 때도 끌 때도 비활성
/// 행은 건드리지 않는다. 클릭해도 안 바뀌는 행이라는 계약이 경로에 따라 깨지면 안 된다.
///
/// 키보드로도 완전히 조작된다 — 트리거가 포커스를 가진 채 `↓`/`Enter`/`Space` 면
/// 열리고, `↑`/`↓`/`Home`/`End` 가 active 행을 옮기며(`disabled` 행은 건너뛴다),
/// `Space`/`Enter` 가 그 행을 **닫지 않고** 토글하고, `Esc` 가 닫는다(포커스는 트리거에
/// 남는다). `Tab` 은 닫고 다음 위젯으로 넘어간다. active 행은 열 때마다 초기화된다 —
/// 목록이 바뀐 뒤 엉뚱한 행이 짚힌 채 열리지 않도록.
///
/// 선택이 하나라도 바뀌면 `true` 를 반환한다([`crate::select`] 와 대칭).
#[allow(clippy::too_many_arguments)] // reason: select 와 대칭인 시그니처 + labels 주입. 인위적 그룹핑 불필요
pub fn multi_select(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: &str,
    selected: &mut [bool],
    options: &[&str],
    disabled: Option<&[bool]>,
    labels: &MultiSelectLabels<'_>,
    all_toggle: Option<MultiSelectAllToggle<'_>>,
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
    let mut open = ui.memory(|m| m.is_popup_open(popup_id));

    // 열려 있는 동안의 키는 이 팝업 것이다 — 트리거를 allocate 하기 **전에** 걷어낸다
    // ([`take_nav_keys`] 의 두 이유). 팝업은 egui `Memory` 상 한 번에 하나만 열리므로
    // `open` 이 곧 "이 위젯이 지금 키보드 주인" 이라는 뜻이다.
    let nav = if open {
        take_nav_keys(ui)
    } else {
        NavKeys::default()
    };

    let active_id = active_row_id(popup_id);
    let mut active: Option<usize> = ui
        .data(|d| d.get_temp::<Option<usize>>(active_id))
        .flatten();
    let mut changed = false;
    // active 가 스크롤 뷰 밖으로 나가면 따라가야 한다 — 키로 움직인 프레임에만 켠다
    // (마우스 hover 로 목록을 굴리는 동안 화면이 튀면 안 된다).
    let mut scroll_to_active = false;
    if open {
        let n = row_count(selected, options);
        // 목록·마스크는 프레임마다 바뀔 수 있다 — 갈 곳을 잃은 커서는 버린다.
        active = active.filter(|i| *i < n && row_enabled(disabled, *i));
        for (pressed, forward) in [(nav.down, true), (nav.up, false)] {
            if pressed {
                active = step_active(active, n, disabled, forward);
                scroll_to_active = true;
            }
        }
        if nav.home || nav.end {
            active = edge_enabled(n, disabled, nav.home);
            scroll_to_active = true;
        }
        // 토글은 트리거를 그리기 **전에** 반영한다 — 요약 라벨이 같은 프레임에 갱신된다.
        if nav.toggle
            && let Some(flag) = active.and_then(|i| selected.get_mut(i))
        {
            *flag = !*flag;
            changed = true;
        }
        if nav.close || nav.tab {
            // `Tab` 은 여기서 닫기만 한다 — 포커스 이동은 소비하지 않은 그 키로
            // egui 가 이어서 처리한다.
            ui.memory_mut(|m| m.close_popup());
            open = false;
        }
    }

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
    //
    // 닫혀 있어도 키보드 포커스를 가졌으면 같은 보더다 — `Tab` 으로 짚은 컨트롤이
    // idle 과 구별되지 않으면 다음 `↓`/`Enter` 가 어디로 갈지 알 수 없다.
    let border = if !enabled {
        theme.select_border()
    } else if open || resp.has_focus() {
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

    // 닫힌 트리거에서 `↓` 는 "열기" 다. `Enter`/`Space` 는 egui 가 클릭으로 승격시켜
    // (fake primary click) 아래 `resp.clicked()` 로 들어오므로 따로 읽지 않는다.
    let open_by_key = enabled
        && !open
        && resp.has_focus()
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));

    if (enabled && resp.clicked()) || open_by_key {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
        if ui.memory(|m| m.is_popup_open(popup_id)) {
            open = true;
            // 열 때마다 커서를 초기화한다 — 지난 위치를 기억하면 목록이 바뀐 뒤
            // 엉뚱한 행이 짚힌 채 열린다.
            active = None;
            // 클릭으로 열었어도 이어서 키보드로 조작할 수 있어야 한다. egui 는 일반
            // 위젯에 클릭 포커스를 주지 않으므로 여기서 직접 가져온다.
            resp.request_focus();
        } else {
            open = false;
        }
    }

    // 포커스가 있는 동안 `↑`/`↓` 는 포커스 이동이 아니라 이 위젯의 뜻이다(닫혔으면
    // "열기", 열렸으면 "행 이동"). `Esc` 는 **열려 있을 때만** 가져간다 — 닫힌 상태의
    // `Esc` 는 상위 화면 것이고, 여기서 잠그면 포커스도 안 풀린다.
    //
    // 필터는 **이번 프레임의 최종 열림 상태**로 건다. 이 자리가 여닫기 처리보다 앞서면
    // 여는 프레임에 `escape: false` 가 박혀, 곧이어 들어온 `Esc` 가 팝업을 닫으면서
    // 포커스까지 걷어간다(그러면 다음 `↓` 로 다시 열 수 없다). 필터는 다음 프레임
    // 입력부터 효력이 있으므로 늦게 걸어도 손해가 없다.
    if resp.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                resp.id,
                egui::EventFilter {
                    // `Tab` 은 넘긴다 — 팝업만 닫고 포커스는 다음 위젯으로 가야 한다.
                    tab: false,
                    horizontal_arrows: false,
                    vertical_arrows: true,
                    escape: open,
                },
            );
        });
    }

    // 메뉴 폭은 팝업을 그리기 **전에** 확정한다. egui 팝업 본문은 justified 세로
    // 레이아웃이라 행이 언제나 가용 폭을 채우고, 그 가용 폭은 다시 지난 프레임의 내용
    // 폭에서 온다 — 여기에 말줄임(가용 폭 기준)까지 얹히면 서로를 물어 폭이 트리거에
    // 고정되거나 max 에 붙어버린다. 가장 넓은 행을 직접 재서 min/max 를 같은 값으로
    // 못박아 그 되먹임을 끊는다.
    //
    // 규칙은 디자인 판정 그대로 — min = 트리거 폭, 내용에 맞춰 max-width 까지 확장.
    // CSS 처럼 min 이 max 를 이긴다(트리거가 max 보다 넓으면 트리거를 따른다).
    let widest_option = options
        .iter()
        .take(selected.len())
        .map(|opt| crate::checkbox_width(ui, theme, opt))
        .fold(0.0_f32, f32::max);
    // 액션 행도 메뉴의 내용이다 — 두 문구 중 넓은 쪽까지 재야 라벨이 전환될 때 폭이
    // 흔들리거나 잘리지 않는다(끄면 0 이라 기존 폭 계산과 완전히 같다).
    let widest_all_toggle = all_toggle.map_or(0.0, |t| {
        all_toggle_width(ui, theme, t.select_all).max(all_toggle_width(ui, theme, t.clear_all))
    });
    let widest_row = widest_option.max(widest_all_toggle);
    let menu_chrome = popup_chrome_width(ui);
    let menu_min = width;
    let menu_max = (theme.multiselect_menu_max_width().value() - menu_chrome).max(menu_min);
    let menu_width = widest_row.clamp(menu_min, menu_max);

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
            // 일괄 토글 행은 스크롤 영역 **밖**에 둔다 — 최상단에 고정돼야 목록을
            // 굴려도 사라지지 않는다(디자인에서도 list 의 형제 노드다).
            if let Some(t) = all_toggle {
                let all_on = all_rows_on(selected, options, disabled);
                let label = if all_on { t.clear_all } else { t.select_all };
                if all_toggle_row(ui, theme, label).clicked() {
                    // 전부 켜져 있으면 끄고, 아니면 켠다. 비활성 행은 어느 쪽이든 그대로.
                    let n = row_count(selected, options);
                    for (i, flag) in selected.iter_mut().enumerate().take(n) {
                        if row_enabled(disabled, i) && *flag == all_on {
                            *flag = !all_on;
                            changed = true;
                        }
                    }
                }
                all_toggle_separator(ui, theme);
            }
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
                        // 키보드 커서 배경은 체크박스 **뒤**에 깔린다. checkbox 가 자기
                        // rect 를 직접 할당하므로 자리를 먼저 예약해 두고 그린 뒤 채운다
                        // (행 폭도 checkbox 의 내용 폭이 아니라 메뉴 폭이어야 한다).
                        let cursor = (active == Some(i))
                            .then(|| (ui.painter().add(egui::Shape::Noop), ui.available_width()));
                        let resp = crate::checkbox(ui, theme, flag, opt, row_enabled(disabled, i));
                        if resp.changed() {
                            changed = true;
                        }
                        if let Some((slot, row_width)) = cursor {
                            let row = egui::Rect::from_min_size(
                                resp.rect.left_top(),
                                egui::vec2(row_width, resp.rect.height()),
                            );
                            ui.painter().set(
                                slot,
                                egui::Shape::rect_filled(
                                    row,
                                    theme.menu_item_radius().value(),
                                    theme.surface_active().to_egui(),
                                ),
                            );
                            if scroll_to_active {
                                ui.scroll_to_rect(row, None);
                            }
                        }
                    }
                });
        },
    );

    // 팝업 안을 마우스로 누르면 egui 가 트리거의 포커스를 회수한다. 팝업은 열린 채라
    // 키보드 조작이 이어져야 하므로 되찾아 준다.
    if open && pressed_inside_popup(ui, popup_id) {
        resp.request_focus();
    }
    ui.data_mut(|d| d.insert_temp(active_id, active));
    changed
}
