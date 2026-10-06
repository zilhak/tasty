//! 탐색기 내용 영역의 상태 화면 — 빈 폴더 · 권한 거부 · 불러오는 중 · 읽기 오류.
//! 시안 `ExpState`(갤러리 Spec "Empty / permission / loading / read error")를 따른다: 가운데 정렬한
//! 글리프 · 제목(body) · 선택 보조 줄(caption, text-muted) · 선택 이유 줄(mono caption, text-muted) ·
//! 선택 버튼 줄을 space-sm 간격으로 쌓고, 버튼 줄은 space-xs 를 더 띄운다.
//! 권한 거부는 글리프와 제목을 accent-warning, 읽기 오류는 explorer-error-fg 로 칠하고, 불러오는 중은
//! 글리프 자리에 Spinner 를 둔다. 읽기 오류는 OS 이유 문구를 번역하지 않고 보이며 Retry(같은 경로를
//! 다시 읽음)와 Go up(상위 폴더, 루트에서는 숨김)을 둔다.
//! 시안의 패널 배경·테두리는 갤러리 전시 칸이고 본체에서는 내용 영역 자체가 그 자리다.

use std::path::Path;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, Spinner};

use super::ExplorerAction;
use super::view::{ExplorerView, LoadState};
use crate::adapters::ui::icons::{self, Icon};
use crate::i18n::t;

/// 시안 글리프 확대 비율(`transform: scale(1.6)`). 대응 토큰이 없다.
const GLYPH_SCALE: f32 = 1.6;
/// 시안 보조 줄·이유 줄 최대 폭(`maxWidth: 200`). Theme 역할에 연결하지 않은 화면 전용 고정 치수다(ADR-0035).
const SUB_MAX_W: LogicalPx = LogicalPx(200.0);
/// 시안 버튼 크기(`size="sm"`).
const ACTION_SIZE: ControlSize = ControlSize::Sm;

/// 상태 화면의 글리프 — 아이콘 또는 Spinner.
#[derive(Clone, Copy)]
enum StateGlyph {
    Icon(Icon),
    Spinner,
}

/// 글리프와 제목의 색조. 중립은 글리프 text-muted · 제목 text-secondary 다.
#[derive(Clone, Copy)]
enum Tone {
    Neutral,
    Warning,
    Error,
}

/// 상태 화면 하나의 내용.
struct StateScreen<'a> {
    glyph: StateGlyph,
    tone: Tone,
    title: &'a str,
    sub: Option<&'a str>,
    /// 번역하지 않은 OS 이유 문구(mono).
    reason: Option<&'a str>,
    /// 읽기 오류의 Retry · Go up 버튼. Go up 은 상위 폴더가 있을 때만 있다.
    actions: Option<ReadErrorActions>,
}

#[derive(Clone, Copy)]
struct ReadErrorActions {
    go_up: bool,
}

/// 목록 대신 상태 화면을 그려야 하면 받은 영역에 그리고 true 를 돌려준다.
/// 읽기 오류 화면의 버튼을 누르면 `action` 에 Refresh 또는 GoUp 을 넣는다.
pub(super) fn show_for(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &ExplorerView,
    root: &Path,
    action: &mut Option<ExplorerAction>,
) -> bool {
    let screen = match &view.state {
        LoadState::NoPermission => StateScreen {
            glyph: StateGlyph::Icon(icons::LOCK),
            tone: Tone::Warning,
            title: t("explorer.state.no_permission"),
            sub: Some(t("explorer.state.no_permission_sub")),
            reason: None,
            actions: None,
        },
        // 읽을 수 없음을 빈 폴더로 떨어뜨리지 않는다(시안 ssh config Note 의 원칙).
        LoadState::Error(msg) => StateScreen {
            glyph: StateGlyph::Icon(icons::ALERT_TRIANGLE),
            tone: Tone::Error,
            title: t("explorer.state.read_error"),
            sub: None,
            reason: Some(msg),
            actions: Some(ReadErrorActions {
                go_up: root.parent().is_some(),
            }),
        },
        LoadState::Loading => StateScreen {
            glyph: StateGlyph::Spinner,
            tone: Tone::Neutral,
            title: t("explorer.state.loading"),
            sub: None,
            reason: None,
            actions: None,
        },
        LoadState::Ok if view.entries.is_empty() => StateScreen {
            glyph: StateGlyph::Icon(icons::FOLDER_OPEN),
            tone: Tone::Neutral,
            title: t("explorer.state.empty"),
            sub: None,
            reason: None,
            actions: None,
        },
        LoadState::Ok => return false,
    };
    if let Some(clicked) = show(ui, theme, &screen)
        && action.is_none()
    {
        *action = Some(clicked);
    }
    true
}

/// 받은 영역 전체를 차지하고 그 가운데에 상태 블록을 그린다. 누른 버튼의 액션을 돌려준다.
fn show(ui: &mut egui::Ui, theme: &Theme, s: &StateScreen<'_>) -> Option<ExplorerAction> {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ui.available_height()),
        egui::Sense::hover(),
    );
    let muted = theme.text_muted().to_egui();
    let (glyph_fg, title_fg) = match s.tone {
        Tone::Neutral => (muted, theme.text_secondary().to_egui()),
        Tone::Warning => {
            let c = theme.accent_warning().to_egui();
            (c, c)
        }
        Tone::Error => {
            let c = theme.explorer_error_fg().to_egui();
            (c, c)
        }
    };
    // transform: scale 은 배치에 영향이 없다. 배치는 원래 글리프 크기로 하고 그림만 확대한다.
    let glyph_box = theme.icon_glyph_size_md.value();
    let glyph = glyph_box * GLYPH_SCALE;
    let gap = theme.spacing_sm.value();
    let inner_w = (rect.width() - theme.spacing_lg.value() * 2.0).max(0.0);
    let center = |text: &str, font: egui::FontId, color: egui::Color32, max_w: f32| {
        let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, color, max_w);
        job.halign = egui::Align::Center;
        ui.painter().layout_job(job)
    };
    let caption = theme.font_size_caption.value();
    let narrow = SUB_MAX_W.value().min(inner_w);
    let title = center(
        s.title,
        egui::FontId::proportional(theme.font_size_body.value()),
        title_fg,
        inner_w,
    );
    let sub = s
        .sub
        .map(|t| center(t, egui::FontId::proportional(caption), muted, narrow));
    let reason = s
        .reason
        .map(|t| center(t, egui::FontId::monospace(caption), muted, narrow));
    let action_h = ACTION_SIZE.height(theme);
    let mut block_h = glyph_box + gap + title.rect.height();
    for g in [&sub, &reason].into_iter().flatten() {
        block_h += gap + g.rect.height();
    }
    if s.actions.is_some() {
        block_h += gap + theme.spacing_xs.value() + action_h;
    }
    let top = rect.center().y - block_h * 0.5;
    let glyph_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, top + glyph_box * 0.5),
        egui::vec2(glyph, glyph),
    );
    match s.glyph {
        StateGlyph::Icon(icon) => {
            icon.image(glyph, glyph_fg).paint_at(ui, glyph_rect);
        }
        StateGlyph::Spinner => {
            let mut slot = ui.new_child(egui::UiBuilder::new().max_rect(glyph_rect));
            Spinner::new().size(glyph).show(&mut slot, theme);
        }
    }
    // 가운데 정렬 job 의 원점은 줄 가운데다.
    let mut y = top + glyph_box + gap;
    let title_h = title.rect.height();
    ui.painter()
        .galley(egui::pos2(rect.center().x, y), title, title_fg);
    y += title_h;
    for g in [sub, reason].into_iter().flatten() {
        y += gap;
        let h = g.rect.height();
        ui.painter()
            .galley(egui::pos2(rect.center().x, y), g, muted);
        y += h;
    }
    let actions = s.actions?;
    y += gap + theme.spacing_xs.value();
    action_row(ui, theme, rect, y, actions)
}

/// 시안 버튼 줄(`display: flex; gap: space-sm`)을 가운데에 놓는다. Retry 는 Secondary, Go up 은 Ghost.
fn action_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    top: f32,
    actions: ReadErrorActions,
) -> Option<ExplorerAction> {
    let retry = t("explorer.state.read_error_retry");
    let go_up = t("explorer.state.read_error_go_up");
    let labels: &[&str] = if actions.go_up {
        &[retry, go_up]
    } else {
        &[retry]
    };
    let font = egui::FontId::proportional(ACTION_SIZE.font_size(theme));
    let pad = ACTION_SIZE.pad_x(theme) * 2.0;
    let gap = theme.spacing_sm.value();
    let row_w = labels
        .iter()
        .map(|l| {
            ui.painter()
                .layout_no_wrap((*l).to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
                .rect
                .width()
                + pad
        })
        .sum::<f32>()
        + gap * (labels.len() as f32 - 1.0);
    let row = egui::Rect::from_min_size(
        egui::pos2(rect.center().x - row_w * 0.5, top),
        egui::vec2(row_w, ACTION_SIZE.height(theme)),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(row)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = gap;
    let mut clicked = None;
    if Button::new(retry)
        .variant(ButtonVariant::Secondary)
        .size(ACTION_SIZE)
        .show(&mut child, theme)
        .clicked()
    {
        clicked = Some(ExplorerAction::Refresh);
    }
    if actions.go_up
        && Button::new(go_up)
            .variant(ButtonVariant::Ghost)
            .size(ACTION_SIZE)
            .show(&mut child, theme)
            .clicked()
    {
        clicked = Some(ExplorerAction::GoUp);
    }
    clicked
}
