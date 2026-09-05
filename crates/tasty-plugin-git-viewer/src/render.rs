//! egui-mesh 자가 렌더 — git-viewer popup 콘텐츠(디자인 `overlays/git_viewer.jsx` 전사).
//!
//! host 는 셸(scrim/bg_panel/border/Esc/outside-click)만 그리고, 이 모듈이 content_rect
//! 안쪽을 plugin egui 로 그린다. 색·폰트·간격은 모두 `Theme` 토큰에서 가져온다
//! (디자인 토큰 = host catppuccin → 의미 토큰 매핑). 상호작용(worktree 선택 / 파일→diff /
//! Back / Refresh)은 이 프레임 안에서 [`ViewerState`] 를 직접 mutate 한다 — set_context
//! 만으로 구동되므로, 갱신된 pane 이 클릭 지점보다 **뒤에** 그려지도록 순서를 잡는다.
//!
//! 색 매핑: oid·refs·main·hunk = `accent_info`(sky),
//! current·added·diff `+` = `accent_success`, locked·modified = `accent_warning`,
//! invalid·deleted·unmerged·diff `-`·error = `accent_danger`, linked·`?` = neutral.

use egui::{Align, Align2, Color32, FontId, Layout, Rect, Sense, Stroke, UiBuilder, vec2};
use tasty_plugin_sdk::Translator;
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, TagVariant, tag, tag_width};

use crate::ViewerState;
use tasty_git_core::{
    DiffData, DiffLine, DiffLineKind, FileStatus, LogEntry, StatusEntry, WorktreeEntry,
};

// ── 디자인 고정 px (git_viewer.jsx 의 화면 전용 치수 — Theme 토큰에 대응 없음) ──
/// worktree rail 고정 폭(jsx `width: 232`). 2줄 행이 어떤 프레임 폭에서도 안 넘치게 고정.
const RAIL_W: f32 = 232.0;
/// 섹션 헤더 strip 높이(jsx `gvHeadStrip height: 28`).
const SECTION_H: f32 = 28.0;
/// context strip 높이(jsx `height: 30`).
const CTX_H: f32 = 30.0;
/// diff 툴바 높이(jsx `height: 32`).
const DIFF_TOOLBAR_H: f32 = 32.0;
/// Changes 행 높이(jsx `ChRow height: 26`).
const CH_ROW_H: f32 = 26.0;
/// Commits 행 높이(jsx `CmRow height: 28`).
const CM_ROW_H: f32 = 28.0;
/// 상태 pill 고정 폭(jsx `GBadge width: 18`).
const STATUS_BADGE_W: f32 = 18.0;
/// diff old/new 라인번호 거터 폭(jsx `width: 34`).
const DIFF_GUTTER_W: f32 = 34.0;
/// diff `+`/`-`/context 부호 컬럼 폭(jsx `width: 14`).
const DIFF_SIGN_W: f32 = 14.0;

fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}
fn prop(size: f32) -> FontId {
    FontId::proportional(size)
}

/// popup 본문 진입점 — CentralPanel(bg_panel) 위에 header + context strip + body.
pub fn draw(ctx: &egui::Context, theme: &Theme, state: &mut ViewerState, tr: &Translator) {
    let frame = egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .inner_margin(egui::Margin::ZERO);
    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        header(ui, theme, state, tr);
        context_strip(ui, theme, state, tr);
        if let Some(err) = state.error.clone() {
            error_line(ui, theme, &err);
        }
        if state.repo_path.is_none() {
            if state.loading {
                loading(ui, theme, tr);
            } else {
                nonrepo(ui, theme, tr);
            }
            return;
        }
        body(ui, theme, state, tr);
    });
}

/// 단일 인스턴스 가드용 — 두 번째 popup 인스턴스가 보여줄 "이미 열림" 중앙 메시지.
pub fn draw_busy(ctx: &egui::Context, theme: &Theme, tr: &Translator) {
    let frame = egui::Frame::new().fill(theme.bg_panel().to_egui());
    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        let h = ui.available_height().max(1.0);
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), h),
            Layout::centered_and_justified(egui::Direction::TopDown),
            |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(tr.t("git_viewer.already_open"))
                            .size(theme.font_size_body.value())
                            .strong()
                            .color(theme.text_secondary().to_egui()),
                    );
                    ui.add_space(theme.spacing_xs.value());
                    ui.label(
                        egui::RichText::new(tr.t("git_viewer.already_open_hint"))
                            .size(theme.font_size_caption.value())
                            .color(theme.text_muted().to_egui()),
                    );
                });
            },
        );
    });
}

// ── header (row 1): Git 타이틀 + Refresh(secondary) ──
fn header(ui: &mut egui::Ui, theme: &Theme, state: &mut ViewerState, tr: &Translator) {
    let full_w = ui.available_width();
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_md.value();
    let btn_h = ControlSize::Sm.height(theme);
    let h = pad_y * 2.0 + btn_h;
    let (rect, _) = ui.allocate_exact_size(vec2(full_w, h), Sense::hover());

    // 타이틀(아이콘 생략 — host UiNode 경로와 동일, primitive 매핑에 icon 없음).
    ui.painter().text(
        egui::pos2(rect.left() + pad_x, rect.center().y),
        Align2::LEFT_CENTER,
        tr.t("git_viewer.heading"),
        prop(theme.font_size_max.value()),
        theme.text_primary().to_egui(),
    );

    // Refresh 버튼 — 우측 정렬(right_to_left child).
    let ctrl_rect = Rect::from_min_max(
        egui::pos2(rect.left(), rect.top() + pad_y),
        egui::pos2(rect.right() - pad_x, rect.top() + pad_y + btn_h),
    );
    let mut cui = ui.new_child(
        UiBuilder::new()
            .max_rect(ctrl_rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    if Button::new(&tr.t("git_viewer.refresh"))
        .variant(ButtonVariant::Secondary)
        .size(ControlSize::Sm)
        .show(&mut cui, theme)
        .clicked()
    {
        state.refresh();
    }
    bottom_separator(ui, theme, rect);
}

// ── header (row 2): context strip — worktree · branch · oid · repo path ──
fn context_strip(ui: &mut egui::Ui, theme: &Theme, state: &ViewerState, tr: &Translator) {
    let full_w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(full_w, CTX_H), Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());
    bottom_separator(ui, theme, rect);

    let active = state.worktrees.get(state.active_worktree);
    let name = active
        .map(|w| w.name.clone())
        .unwrap_or_else(|| "—".to_string());
    let branch = active
        .and_then(|w| w.branch.clone())
        .unwrap_or_else(|| tr.t("git_viewer.detached").to_string());
    let oid = active.and_then(|w| w.oid.clone());

    let pad_x = theme.spacing_md.value();
    let content = rect.shrink2(vec2(pad_x, 0.0));
    let mut cui = ui.new_child(
        UiBuilder::new()
            .max_rect(content)
            .layout(Layout::left_to_right(Align::Center)),
    );
    cui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    cui.label(
        egui::RichText::new(name)
            .font(mono(theme.font_size_term_sm.value()))
            .color(theme.text_primary().to_egui()),
    );
    cui.label(
        egui::RichText::new("·")
            .size(theme.font_size_term_sm.value())
            .color(theme.text_disabled().to_egui()),
    );
    cui.label(
        egui::RichText::new(branch)
            .font(mono(theme.font_size_term_sm.value()))
            .color(theme.text_secondary().to_egui()),
    );
    if let Some(oid) = oid {
        tag(&mut cui, theme, &oid, TagVariant::Info, false);
    }

    // repo path — 우측, mono caption, muted, 우측 영역에 clip.
    if let Some(path) = state
        .repo_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
    {
        let clip = Rect::from_min_max(
            egui::pos2(content.left() + content.width() * 0.4, rect.top()),
            egui::pos2(content.right(), rect.bottom()),
        );
        ui.painter().with_clip_rect(clip).text(
            egui::pos2(content.right(), rect.center().y),
            Align2::RIGHT_CENTER,
            path,
            mono(theme.font_size_caption.value()),
            theme.text_muted().to_egui(),
        );
    }
}

fn error_line(ui: &mut egui::Ui, theme: &Theme, err: &str) {
    let full_w = ui.available_width();
    let pad_x = theme.spacing_md.value();
    let h = theme.spacing_sm.value() * 2.0 + theme.font_size_caption.value();
    let (rect, _) = ui.allocate_exact_size(vec2(full_w, h), Sense::hover());
    let danger = theme.accent_danger().to_egui();
    // 오류 요약 줄 배경. 대응 토큰 없음 — 값에 이름만 둔다.
    const ERROR_ROW_BG_OPACITY: f32 = 0.1;
    ui.painter()
        .rect_filled(rect, 0.0, danger.gamma_multiply(ERROR_ROW_BG_OPACITY));
    bottom_separator(ui, theme, rect);
    ui.painter()
        .with_clip_rect(rect.shrink2(vec2(pad_x, 0.0)))
        .text(
            egui::pos2(rect.left() + pad_x, rect.center().y),
            Align2::LEFT_CENTER,
            err,
            prop(theme.font_size_caption.value()),
            danger,
        );
}

/// (ADR-0056) mirror popup 의 최초 원격 스냅샷 왕복이 아직 안 왔을 때.
fn loading(ui: &mut egui::Ui, theme: &Theme, tr: &Translator) {
    let h = ui.available_height().max(1.0);
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), h),
        Layout::centered_and_justified(egui::Direction::TopDown),
        |ui| {
            ui.label(
                egui::RichText::new(tr.t("git_viewer.loading"))
                    .size(theme.font_size_body.value())
                    .color(theme.text_muted().to_egui()),
            );
        },
    );
}

fn nonrepo(ui: &mut egui::Ui, theme: &Theme, tr: &Translator) {
    let h = ui.available_height().max(1.0);
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), h),
        Layout::centered_and_justified(egui::Direction::TopDown),
        |ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(tr.t("git_viewer.no_repo_title"))
                        .size(theme.font_size_body.value())
                        .strong()
                        .color(theme.text_secondary().to_egui()),
                );
                ui.add_space(theme.spacing_sm.value());
                ui.label(
                    egui::RichText::new(tr.t("git_viewer.no_repo"))
                        .size(theme.font_size_caption.value())
                        .color(theme.text_muted().to_egui()),
                );
            });
        },
    );
}

// ── body: rail(232) | right column ──
fn body(ui: &mut egui::Ui, theme: &Theme, state: &mut ViewerState, tr: &Translator) {
    let avail = ui.available_rect_before_wrap();
    let rail_rect = Rect::from_min_size(avail.min, vec2(RAIL_W, avail.height()));
    let right_rect = Rect::from_min_max(egui::pos2(avail.min.x + RAIL_W, avail.min.y), avail.max);

    // rail 우측 경계선.
    ui.painter().vline(
        avail.min.x + RAIL_W,
        rail_rect.y_range(),
        Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );

    draw_rail(ui, theme, state, tr, rail_rect);
    draw_right(ui, theme, state, tr, right_rect);
    ui.allocate_rect(avail, Sense::hover());
}

fn draw_rail(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut ViewerState,
    tr: &Translator,
    area: Rect,
) {
    let mut pane = ui.new_child(
        UiBuilder::new()
            .max_rect(area)
            .layout(Layout::top_down(Align::Min)),
    );
    pane.spacing_mut().item_spacing = vec2(0.0, 0.0);
    pane_head(
        &mut pane,
        theme,
        &format!(
            "{} ({})",
            tr.t("git_viewer.worktrees_heading"),
            state.worktrees.len()
        ),
    );
    if state.worktrees.is_empty() {
        empty_line(&mut pane, theme, &tr.t("git_viewer.no_worktrees"));
        return;
    }
    let mut clicked: Option<usize> = None;
    // virtualization — 보이는 행만 레이아웃한다. `row_range` 는 **전체 목록 기준**
    // 인덱스라 `select_worktree(idx)` 에 그대로 넘길 수 있다.
    egui::ScrollArea::vertical()
        .id_salt("gv_rail")
        .auto_shrink([false, false])
        .show_rows(
            &mut pane,
            wt_row_h(theme),
            state.worktrees.len(),
            |ui, row_range| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                for idx in row_range {
                    let Some(wt) = state.worktrees.get(idx) else {
                        continue;
                    };
                    let selected = idx == state.active_worktree;
                    if ui
                        .push_id(idx, |ui| wt_row(ui, theme, tr, wt, selected))
                        .inner
                    {
                        clicked = Some(idx);
                    }
                }
            },
        );
    // 클릭 → worktree 재바인딩. right column 은 이 뒤에 그려져 같은 프레임에 반영된다.
    if let Some(idx) = clicked {
        state.select_worktree(idx);
    }
}

/// worktree 행 높이 — 2줄 + 상하 padding. theme 파생이지만 한 프레임 안에서 모든 행이
/// 같은 값이라 `ScrollArea::show_rows` 의 균일 높이 전제를 만족한다.
fn wt_row_h(theme: &Theme) -> f32 {
    let pad_y = theme.spacing_sm.value();
    let line_gap = theme.spacing_xs.value();
    let l1_h = theme.font_size_term_sm.value() + 4.0;
    let l2_h = theme.font_size_caption.value() + 4.0;
    pad_y * 2.0 + l1_h + line_gap + l2_h
}

/// 2줄 worktree 행: line1 = name + type pill, line2 = short oid + state pill.
fn wt_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    tr: &Translator,
    wt: &WorktreeEntry,
    selected: bool,
) -> bool {
    let full_w = ui.available_width();
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_sm.value();
    let line_gap = theme.spacing_xs.value();
    let l1_h = theme.font_size_term_sm.value() + 4.0;
    let l2_h = theme.font_size_caption.value() + 4.0;
    let h = wt_row_h(theme);
    let (rect, resp) = ui.allocate_exact_size(vec2(full_w, h), Sense::click());

    if selected {
        ui.painter()
            .rect_filled(rect, 0.0, theme.surface_active().to_egui());
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, theme.overlay_hover().to_egui_premultiplied());
    }

    let name_color = if !wt.is_valid {
        theme.text_disabled()
    } else if selected {
        theme.text_primary()
    } else {
        theme.text_secondary()
    };

    // line 1: name (left, clipped) + type pill (right).
    let l1 = Rect::from_min_size(
        egui::pos2(rect.left() + pad_x, rect.top() + pad_y),
        vec2(full_w - pad_x * 2.0, l1_h),
    );
    let (type_key, type_variant) = if wt.is_main {
        ("git_viewer.wt_main", TagVariant::Info)
    } else {
        ("git_viewer.wt_linked", TagVariant::Default)
    };
    let mut t1 = ui.new_child(
        UiBuilder::new()
            .max_rect(l1)
            .layout(Layout::right_to_left(Align::Center)),
    );
    let type_resp = tag(&mut t1, theme, &tr.t(type_key), type_variant, false);
    let name_right = type_resp.rect.left() - theme.spacing_sm.value();
    let name_clip = Rect::from_min_max(l1.min, egui::pos2(name_right, l1.max.y));
    ui.painter().with_clip_rect(name_clip).text(
        egui::pos2(l1.left(), l1.center().y),
        Align2::LEFT_CENTER,
        &wt.name,
        mono(theme.font_size_term_sm.value()),
        name_color.to_egui(),
    );

    // line 2: oid (left, accent_info) + state pill (right).
    let l2 = Rect::from_min_size(
        egui::pos2(rect.left() + pad_x, l1.max.y + line_gap),
        vec2(full_w - pad_x * 2.0, l2_h),
    );
    if let Some((label_key, variant)) = wt_state_pill(wt) {
        let mut t2 = ui.new_child(
            UiBuilder::new()
                .max_rect(l2)
                .layout(Layout::right_to_left(Align::Center)),
        );
        let resp = tag(&mut t2, theme, &tr.t(label_key), variant, true);
        // locked/invalid 사유를 hover tooltip 으로(jsx `title={wt.reason}`).
        if let Some(reason) = &wt.lock_reason
            && !reason.is_empty()
        {
            resp.on_hover_text(reason);
        }
    }
    if let Some(oid) = &wt.oid {
        ui.painter().text(
            egui::pos2(l2.left(), l2.center().y),
            Align2::LEFT_CENTER,
            oid,
            mono(theme.font_size_caption.value()),
            theme.accent_info().to_egui(),
        );
    }

    // 선택 행 좌측 2px inset accent bar.
    if selected {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
            0.0,
            theme.accent_primary().to_egui(),
        );
    }
    bottom_separator(ui, theme, rect);
    resp.clicked() && wt.is_valid
}

/// worktree 상태 pill(current/locked/invalid). 없으면 None. (색 dot 포함)
fn wt_state_pill(wt: &WorktreeEntry) -> Option<(&'static str, TagVariant)> {
    if !wt.is_valid {
        Some(("git_viewer.wt_invalid", TagVariant::Danger))
    } else if wt.locked {
        Some(("git_viewer.wt_locked", TagVariant::Warning))
    } else if wt.is_current {
        Some(("git_viewer.wt_current", TagVariant::Success))
    } else {
        None
    }
}

fn draw_right(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut ViewerState,
    tr: &Translator,
    area: Rect,
) {
    let half = (area.height() * 0.5).round();
    let top = Rect::from_min_size(area.min, vec2(area.width(), half));
    let bottom = Rect::from_min_max(egui::pos2(area.left(), area.top() + half), area.max);

    // 상단 Changes | 하단 Commits↔Diff 사이 separator.
    ui.painter().hline(
        area.x_range(),
        area.top() + half,
        Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );

    draw_changes(ui, theme, state, tr, top);
    draw_bottom(ui, theme, state, tr, bottom);
}

fn draw_changes(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut ViewerState,
    tr: &Translator,
    area: Rect,
) {
    let mut pane = ui.new_child(
        UiBuilder::new()
            .max_rect(area)
            .layout(Layout::top_down(Align::Min)),
    );
    pane.spacing_mut().item_spacing = vec2(0.0, 0.0);
    pane_head(
        &mut pane,
        theme,
        &format!(
            "{} ({})",
            tr.t("git_viewer.status_heading"),
            state.status_entries.len()
        ),
    );
    if state.status_entries.is_empty() {
        empty_line(&mut pane, theme, &tr.t("git_viewer.no_changes"));
        return;
    }
    let mut clicked: Option<usize> = None;
    // virtualization — `row_range` 는 **전체 목록 기준** 인덱스라 `load_diff(idx)` 가
    // 받는 값의 의미가 바뀌지 않는다.
    egui::ScrollArea::vertical()
        .id_salt("gv_changes")
        .auto_shrink([false, false])
        .show_rows(
            &mut pane,
            CH_ROW_H,
            state.status_entries.len(),
            |ui, row_range| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                for idx in row_range {
                    let Some(entry) = state.status_entries.get(idx) else {
                        continue;
                    };
                    let selected = state.selected_file == Some(idx);
                    if ui
                        .push_id(idx, |ui| ch_row(ui, theme, entry, selected))
                        .inner
                    {
                        clicked = Some(idx);
                    }
                }
            },
        );
    if let Some(idx) = clicked {
        state.load_diff(idx);
    }
}

/// Changes 행: 고정폭 상태 pill + 경로(dir muted / file primary, clip).
fn ch_row(ui: &mut egui::Ui, theme: &Theme, entry: &StatusEntry, selected: bool) -> bool {
    let full_w = ui.available_width();
    let pad_x = theme.spacing_md.value();
    let (rect, resp) = ui.allocate_exact_size(vec2(full_w, CH_ROW_H), Sense::click());
    if selected {
        ui.painter()
            .rect_filled(rect, 0.0, theme.surface_active().to_egui());
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, theme.overlay_hover().to_egui_premultiplied());
    }

    let (glyph, variant) = status_pill(entry.status);
    let content = Rect::from_min_size(
        egui::pos2(rect.left() + pad_x, rect.top()),
        vec2(full_w - pad_x * 2.0, CH_ROW_H),
    );
    let mut cui = ui.new_child(
        UiBuilder::new()
            .max_rect(content)
            .layout(Layout::left_to_right(Align::Center)),
    );
    cui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    // 고정폭 상태 pill 자리.
    cui.allocate_ui_with_layout(
        vec2(STATUS_BADGE_W, CH_ROW_H),
        Layout::left_to_right(Align::Center),
        |ui| {
            tag(ui, theme, glyph, variant, false);
        },
    );
    // 경로 — dir(muted) + file(primary).
    let (dir, file) = split_path(&entry.path);
    let path_clip = Rect::from_min_max(
        egui::pos2(cui.cursor().left(), rect.top()),
        egui::pos2(content.right(), rect.bottom()),
    );
    let p = ui.painter().with_clip_rect(path_clip);
    let cy = rect.center().y;
    let mut x = cui.cursor().left();
    if !dir.is_empty() {
        let g = p.layout_no_wrap(
            dir,
            mono(theme.font_size_term_sm.value()),
            theme.text_muted().to_egui(),
        );
        p.galley(
            egui::pos2(x, cy - g.rect.height() * 0.5),
            g.clone(),
            theme.text_muted().to_egui(),
        );
        x += g.rect.width();
    }
    let gf = p.layout_no_wrap(
        file,
        mono(theme.font_size_term_sm.value()),
        theme.text_primary().to_egui(),
    );
    p.galley(
        egui::pos2(x, cy - gf.rect.height() * 0.5),
        gf,
        theme.text_primary().to_egui(),
    );

    if selected {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
            0.0,
            theme.accent_primary().to_egui(),
        );
    }
    resp.clicked()
}

fn draw_bottom(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut ViewerState,
    tr: &Translator,
    area: Rect,
) {
    // diff 표시 중이면 툴바(Back) 먼저 — Back 클릭 시 close_diff 후 아래에서 commits 로 전환.
    let showing_diff = state.selected_file.is_some() && state.diff_content.is_some();
    let toolbar_h = if showing_diff { DIFF_TOOLBAR_H } else { 0.0 };
    if showing_diff {
        let toolbar = Rect::from_min_size(area.min, vec2(area.width(), DIFF_TOOLBAR_H));
        if diff_toolbar(ui, theme, tr, toolbar, state.diff_content.as_ref()) {
            state.close_diff();
        }
    }
    let content = Rect::from_min_max(egui::pos2(area.left(), area.top() + toolbar_h), area.max);
    if state.selected_file.is_some()
        && let Some(diff) = state.diff_content.as_ref()
    {
        draw_diff(ui, theme, tr, diff, &mut state.diff_width, content);
    } else {
        draw_commits(ui, theme, tr, &state.log_entries, content);
    }
}

/// diff 툴바: Back(ghost) + 파일 경로(mono muted). Back 클릭 여부 반환.
fn diff_toolbar(
    ui: &mut egui::Ui,
    theme: &Theme,
    tr: &Translator,
    area: Rect,
    diff: Option<&DiffData>,
) -> bool {
    ui.painter()
        .rect_filled(area, 0.0, theme.bg_sidebar().to_egui());
    bottom_separator(ui, theme, area);
    let pad_x = theme.spacing_sm.value();
    let mut cui = ui.new_child(
        UiBuilder::new()
            .max_rect(area.shrink2(vec2(pad_x, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    cui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    let back = Button::new(&tr.t("git_viewer.back_to_log"))
        .variant(ButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .show(&mut cui, theme)
        .clicked();
    if let Some(diff) = diff {
        cui.label(
            egui::RichText::new(&diff.file_path)
                .font(mono(theme.font_size_caption.value()))
                .color(theme.text_muted().to_egui()),
        );
    }
    back
}

fn draw_commits(ui: &mut egui::Ui, theme: &Theme, tr: &Translator, log: &[LogEntry], area: Rect) {
    let mut pane = ui.new_child(
        UiBuilder::new()
            .max_rect(area)
            .layout(Layout::top_down(Align::Min)),
    );
    pane.spacing_mut().item_spacing = vec2(0.0, 0.0);
    pane_head(
        &mut pane,
        theme,
        &format!("{} ({})", tr.t("git_viewer.log_heading"), log.len()),
    );
    if log.is_empty() {
        empty_line(&mut pane, theme, &tr.t("git_viewer.no_commits"));
        return;
    }
    // virtualization — `LOG_LIMIT` 만큼 쌓여도 보이는 행만 레이아웃한다. 커밋 행은
    // 클릭 대상이 아니라 인덱스 매핑 부담이 없다.
    egui::ScrollArea::vertical()
        .id_salt("gv_commits")
        .auto_shrink([false, false])
        .show_rows(&mut pane, CM_ROW_H, log.len(), |ui, row_range| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            for idx in row_range {
                let Some(entry) = log.get(idx) else {
                    continue;
                };
                ui.push_id(idx, |ui| cm_row(ui, theme, tr, entry));
            }
        });
}

/// 커밋 summary 표시 문자열. `tasty-git-core` 는 값이 없으면 빈 문자열을 주고 자연어를
/// 만들지 않는다(호출자 주입, ADR-0106 결정 4) — 빈 값의 문구는 여기서 plugin 자기
/// lang 으로 고른다. 원격 mirror 조회도 같은 wire 라 로컬 언어로 표시된다.
fn summary_text<'a>(tr: &'a Translator, entry: &'a LogEntry) -> &'a str {
    if entry.summary.is_empty() {
        tr.t("git_viewer.no_message")
    } else {
        &entry.summary
    }
}

/// 커밋 author 표시 문자열 — `summary_text` 와 같은 규약.
fn author_text<'a>(tr: &'a Translator, entry: &'a LogEntry) -> &'a str {
    if entry.author.is_empty() {
        tr.t("git_viewer.unknown_author")
    } else {
        &entry.author
    }
}

/// Commits 행: oid(info) + refs pills(info) + summary(flex) + author + time.
fn cm_row(ui: &mut egui::Ui, theme: &Theme, tr: &Translator, entry: &LogEntry) {
    let full_w = ui.available_width();
    let pad_x = theme.spacing_md.value();
    let (rect, resp) = ui.allocate_exact_size(vec2(full_w, CM_ROW_H), Sense::hover());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, theme.overlay_hover().to_egui_premultiplied());
    }
    let content = Rect::from_min_size(
        egui::pos2(rect.left() + pad_x, rect.top()),
        vec2(full_w - pad_x * 2.0, CM_ROW_H),
    );
    let gap = theme.spacing_sm.value();

    // oid(info)는 항상 그려지는 고정 요소라 폭을 먼저 측정해 둔다 — 우 cluster가
    // (author 가 아무리 길어도) 이 영역을 절대 침범하지 못하게 clip 상한으로 쓴다.
    let oid_w = ui
        .painter()
        .layout_no_wrap(
            entry.oid_short.clone(),
            mono(theme.font_size_caption.value()),
            Color32::PLACEHOLDER,
        )
        .rect
        .width();
    let right_clip_left = content.left() + oid_w + gap;

    // 우 cluster: time + author (우측 정렬). author 가 길면 egui Label 은 자체적으로
    // wrap/clip 하지 않고 폭 제한 없이 그려지므로(참고: `ch_row` 의 path clip 과 동일
    // 이유), oid 영역을 덮어쓰지 않도록 명시적 상한을 건다 — 넘치는 부분은
    // summary/pill 과 동일하게 픽셀 단위로 잘릴 뿐 ellipsis 는 없다.
    //
    // clip 은 반드시 `shrink_clip_rect`(부모 clip 과 교집합)로 좁힌다 —
    // `set_clip_rect` 는 부모 clip 을 덮어쓴다. 이 행은 ScrollArea 안에서 그려지고
    // `rect` 는 스크롤된 가상 콘텐츠 좌표라, 덮어쓰면 뷰포트 밖으로 밀려난 행의
    // 라벨이 pane 경계를 넘어 그려진다.
    let mut right = ui.new_child(
        UiBuilder::new()
            .max_rect(content)
            .layout(Layout::right_to_left(Align::Center)),
    );
    let right_clip = Rect::from_min_max(
        egui::pos2(right_clip_left, rect.top()),
        egui::pos2(content.right(), rect.bottom()),
    );
    right.shrink_clip_rect(right_clip);
    right.spacing_mut().item_spacing.x = gap;
    right.label(
        egui::RichText::new(&entry.time)
            .font(mono(theme.font_size_caption.value()))
            .color(theme.text_muted().to_egui()),
    );
    right.label(
        egui::RichText::new(author_text(tr, entry))
            .size(theme.font_size_term_sm.value())
            .color(theme.text_muted().to_egui()),
    );
    // `min_rect()`는 clip 과 무관하게 라벨의 논리적(안 잘린) 전체 폭을 반영해 author 가
    // 길면 content 밖(심지어 음수)까지 나갈 수 있다 — pill 상한은 실제로 화면에 보이는
    // (=clip 된) 시작점을 써야 한다. author 가 짧아 clip 이 아예 걸리지 않는 보통
    // 케이스는 natural 값이 이미 right_clip_left 보다 오른쪽이라 이 max 는 no-op —
    // 8-refs 케이스 등 기존 동작에 영향 없다. author 가 길어 clip 이 걸리는 케이스는
    // right_clip_left(=oid 우측 끝)로 클램프돼, pill 은 (남는 자리가 거의 없으므로)
    // 대부분 "+N" 로도 다 못 그려질 수 있다 — oid 를 침범하지 않는 것이 우선이라 정상.
    let right_start = right.min_rect().left().max(right_clip_left);

    // 좌 cluster: oid(info) + refs pills(info). pill 누적 폭이 right cluster를
    // 침범하기 전까지만 그리고, 넘치면 남은 개수를 "+N" pill로 축약한다.
    let mut left = ui.new_child(
        UiBuilder::new()
            .max_rect(content)
            .layout(Layout::left_to_right(Align::Center)),
    );
    left.spacing_mut().item_spacing.x = gap;
    left.label(
        egui::RichText::new(&entry.oid_short)
            .font(mono(theme.font_size_caption.value()))
            .color(theme.accent_info().to_egui()),
    );
    let pill_limit = (right_start - gap).max(left.min_rect().right());
    let mut cursor_x = left.min_rect().right();
    let mut shown = 0usize;
    for r in &entry.refs {
        let remaining_after = entry.refs.len() - shown - 1;
        // 이 pill을 그린 뒤에도 남는 게 있으면 "+N" pill 자리를 반드시 남겨둔다.
        let reserve = if remaining_after > 0 {
            gap + tag_width(&left, theme, &format!("+{remaining_after}"))
        } else {
            0.0
        };
        let w = tag_width(&left, theme, r);
        if cursor_x + gap + w + reserve > pill_limit {
            break;
        }
        tag(&mut left, theme, r, TagVariant::Info, false);
        cursor_x += gap + w;
        shown += 1;
    }
    if shown < entry.refs.len() {
        let remaining = entry.refs.len() - shown;
        tag(
            &mut left,
            theme,
            &format!("+{remaining}"),
            TagVariant::Info,
            false,
        );
    }
    let left_end = left.min_rect().right();

    // summary — 가운데 남는 폭에 clip(truncate).
    let sx = left_end + gap;
    let clip = Rect::from_min_max(
        egui::pos2(sx, rect.top()),
        egui::pos2((right_start - gap).max(sx), rect.bottom()),
    );
    ui.painter().with_clip_rect(clip).text(
        egui::pos2(sx, rect.center().y),
        Align2::LEFT_CENTER,
        summary_text(tr, entry),
        prop(theme.font_size_body.value()),
        theme.text_primary().to_egui(),
    );
}

fn draw_diff(
    ui: &mut egui::Ui,
    theme: &Theme,
    tr: &Translator,
    diff: &DiffData,
    width_cache: &mut Option<(f32, f32)>,
    area: Rect,
) {
    // recessed bg-app well.
    ui.painter()
        .rect_filled(area, 0.0, theme.bg_app().to_egui());
    let mut pane = ui.new_child(
        UiBuilder::new()
            .max_rect(area)
            .layout(Layout::top_down(Align::Min)),
    );
    pane.spacing_mut().item_spacing = vec2(0.0, 0.0);
    if diff.hunks.is_empty() {
        empty_line(&mut pane, theme, &tr.t("git_viewer.no_changes"));
        return;
    }
    // (hunk 헤더 + 라인) 평탄화 — `starts[i]` = i 번째 hunk 헤더의 flat 행 인덱스.
    // hunk 수만큼만 도는 prefix sum 이라 라인 수와 무관하게 싸다.
    let mut starts: Vec<usize> = Vec::with_capacity(diff.hunks.len());
    let mut total_rows = 0usize;
    for hunk in &diff.hunks {
        starts.push(total_rows);
        total_rows += 1 + hunk.lines.len();
    }

    // 가로 폭은 전 라인의 최장 폭으로 **한 번만** 재서 캐시한다. 보이는 라인만 재면
    // 스크롤할 때마다 콘텐츠 폭이 바뀌어 가로 스크롤이 출렁인다. 캐시 키는 폰트 크기
    // (theme 변경 시 자동 재측정), 무효화는 `ViewerState::set_diff` 가 맡는다.
    let sz = theme.font_size_caption.value();
    let row_w = match *width_cache {
        Some((cached_sz, w)) if cached_sz == sz => w,
        _ => {
            let w = diff_content_w(&pane, theme, diff, sz);
            *width_cache = Some((sz, w));
            w
        }
    };

    egui::ScrollArea::both()
        .id_salt("gv_diff")
        .auto_shrink([false, false])
        .show_rows(&mut pane, diff_row_h(theme), total_rows, |ui, row_range| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            for row in row_range {
                // flat 행 → (hunk, 헤더 | 라인). `starts` 는 오름차순이라 이분 탐색.
                let h = starts
                    .partition_point(|&start| start <= row)
                    .wrapping_sub(1);
                let (Some(hunk), Some(start)) = (diff.hunks.get(h), starts.get(h)) else {
                    continue;
                };
                let off = row - start;
                ui.push_id(row, |ui| {
                    if off == 0 {
                        diff_line(ui, theme, DiffRow::hunk(&hunk.header), row_w);
                    } else if let Some(line) = hunk.lines.get(off - 1) {
                        diff_line(ui, theme, DiffRow::line(line), row_w);
                    }
                });
            }
        });
}

/// diff 한 줄의 높이 — hunk 헤더와 일반 라인이 같은 값이라 `show_rows` 의 균일 높이
/// 전제를 만족한다.
fn diff_row_h(theme: &Theme) -> f32 {
    (theme.font_size_caption.value() * 1.65).round()
}

/// diff 콘텐츠의 가로 폭 — 전 라인(hunk 헤더 포함) 중 최장 텍스트 기준. 캐시 미스일
/// 때만 부르는 O(라인 수) 경로다.
fn diff_content_w(ui: &egui::Ui, theme: &Theme, diff: &DiffData, sz: f32) -> f32 {
    let p = ui.painter();
    let mut w = 0.0f32;
    for hunk in &diff.hunks {
        w = w.max(diff_min_w(p, theme, &hunk.header, sz));
        for line in &hunk.lines {
            w = w.max(diff_min_w(p, theme, &line.content, sz));
        }
    }
    w
}

/// diff 한 줄이 자기 텍스트를 다 담는 데 필요한 최소 폭 — 거터 2칸 + 부호 컬럼 +
/// 텍스트 + 우측 여백. 콘텐츠 폭(전 라인 최댓값)과 행별 tint 밴드 폭이 같은 식을
/// 쓰도록 한 곳에 둔다.
fn diff_min_w(p: &egui::Painter, theme: &Theme, text: &str, sz: f32) -> f32 {
    DIFF_GUTTER_W * 2.0
        + DIFF_SIGN_W
        + p.layout_no_wrap(text.to_owned(), mono(sz), Color32::PLACEHOLDER)
            .rect
            .width()
        + theme.spacing_md.value()
}

/// diff 한 줄의 렌더 입력 — hunk 헤더와 일반 라인을 한 모양으로 다룬다.
struct DiffRow<'a> {
    kind: DiffLineKind,
    is_hunk: bool,
    old_no: Option<u32>,
    new_no: Option<u32>,
    text: &'a str,
}

impl<'a> DiffRow<'a> {
    fn hunk(header: &'a str) -> Self {
        Self {
            kind: DiffLineKind::Context,
            is_hunk: true,
            old_no: None,
            new_no: None,
            text: header,
        }
    }

    fn line(line: &'a DiffLine) -> Self {
        Self {
            kind: line.kind,
            is_hunk: false,
            old_no: line.old_lineno,
            new_no: line.new_lineno,
            text: &line.content,
        }
    }
}

/// diff 한 줄: gutter(old/new) + sign + text. hunk 헤더 밴드 / ± 라인 tint.
fn diff_line(ui: &mut egui::Ui, theme: &Theme, row: DiffRow<'_>, row_w: f32) {
    let DiffRow {
        kind,
        is_hunk,
        old_no,
        new_no,
        text,
    } = row;
    let sz = theme.font_size_caption.value();
    let h = diff_row_h(theme);
    // **할당** 폭은 호출자가 준 캐시값(전 라인 최장) — 모든 행이 같은 폭을 할당해야
    // 보이는 라인만 그려도 콘텐츠 폭(=가로 스크롤 범위)이 스크롤 위치에 따라 출렁이지
    // 않는다. 반면 아래 tint 밴드는 **행 자신의 폭**까지만 칠한다(할당 폭과 분리).
    let avail = ui.available_width();
    let full_w = avail.max(row_w);
    let (rect, _) = ui.allocate_exact_size(vec2(full_w, h), Sense::hover());
    // diff 줄 배경 톤. hunk 머리만 한 단계 더 옅다. 대응 토큰 없음.
    const DIFF_HUNK_BG_OPACITY: f32 = 0.09;
    const DIFF_LINE_BG_OPACITY: f32 = 0.10;

    let (fg, bg) = match (is_hunk, kind) {
        (true, _) => (
            theme.accent_info().to_egui(),
            theme
                .accent_info()
                .to_egui()
                .gamma_multiply(DIFF_HUNK_BG_OPACITY),
        ),
        (false, DiffLineKind::Addition) => (
            theme.accent_success().to_egui(),
            theme
                .accent_success()
                .to_egui()
                .gamma_multiply(DIFF_LINE_BG_OPACITY),
        ),
        (false, DiffLineKind::Deletion) => (
            theme.accent_danger().to_egui(),
            theme
                .accent_danger()
                .to_egui()
                .gamma_multiply(DIFF_LINE_BG_OPACITY),
        ),
        (false, DiffLineKind::Context) => (theme.text_primary().to_egui(), Color32::TRANSPARENT),
    };
    if bg != Color32::TRANSPARENT {
        // 밴드 폭은 행 자신의 텍스트 기준 — 할당 폭(전 라인 최장)으로 칠하면 짧은
        // ±/hunk 행의 밴드가 콘텐츠 끝까지 늘어나 기존 시각과 달라진다. 이 측정은
        // 보이는 행에서만 일어나므로 virtualization 이 줄인 비용을 되돌리지 않는다.
        let band_w = avail.max(diff_min_w(ui.painter(), theme, text, sz));
        ui.painter()
            .rect_filled(Rect::from_min_size(rect.min, vec2(band_w, h)), 0.0, bg);
    }
    let p = ui.painter();
    let cy = rect.center().y;
    let disabled = theme.text_disabled().to_egui();
    if !is_hunk {
        let old_s = old_no.map(|n| n.to_string()).unwrap_or_default();
        let new_s = new_no.map(|n| n.to_string()).unwrap_or_default();
        p.text(
            egui::pos2(rect.left() + DIFF_GUTTER_W - 6.0, cy),
            Align2::RIGHT_CENTER,
            old_s,
            mono(sz),
            disabled,
        );
        p.text(
            egui::pos2(rect.left() + DIFF_GUTTER_W * 2.0 - 8.0, cy),
            Align2::RIGHT_CENTER,
            new_s,
            mono(sz),
            disabled,
        );
        let sign = match kind {
            DiffLineKind::Addition => "+",
            DiffLineKind::Deletion => "-",
            DiffLineKind::Context => "",
        };
        // 부호 글리프는 jsx `opacity: 0.8`(hunk band border 패턴과 동일 gamma_multiply).
        // +/- 부호는 본문 전경보다 한 단계 물러난다. 대응 토큰 없음.
        const DIFF_SIGN_OPACITY: f32 = 0.8;
        p.text(
            egui::pos2(rect.left() + DIFF_GUTTER_W * 2.0 + DIFF_SIGN_W * 0.5, cy),
            Align2::CENTER_CENTER,
            sign,
            mono(sz),
            fg.gamma_multiply(DIFF_SIGN_OPACITY),
        );
    }
    let text_x = rect.left() + DIFF_GUTTER_W * 2.0 + DIFF_SIGN_W;
    p.text(
        egui::pos2(text_x, cy),
        Align2::LEFT_CENTER,
        text,
        mono(sz),
        fg,
    );
}

// ── 공용 헬퍼 ──

/// 섹션 헤더 strip — bg-sidebar + 하단 separator + uppercase mono micro muted 라벨.
fn pane_head(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    let full_w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(full_w, SECTION_H), Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());
    bottom_separator(ui, theme, rect);
    ui.painter().text(
        egui::pos2(rect.left() + theme.spacing_md.value(), rect.center().y),
        Align2::LEFT_CENTER,
        text.to_uppercase(),
        mono(theme.font_size_micro.value()),
        theme.text_muted().to_egui(),
    );
}

/// 빈 pane 한 줄 — 중앙 italic muted.
fn empty_line(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    let full_w = ui.available_width();
    let h = ui
        .available_height()
        .max(theme.item_height_interactive.value());
    let (rect, _) = ui.allocate_exact_size(vec2(full_w, h), Sense::hover());
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        prop(theme.font_size_term_sm.value()),
        theme.text_muted().to_egui(),
    );
}

/// rect 하단에 1px separator.
fn bottom_separator(ui: &mut egui::Ui, theme: &Theme, rect: Rect) {
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - theme.border_width.value() * 0.5,
        Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
}

/// 경로를 (dir, file) 로 분리 — 마지막 `/` 기준. dir 은 trailing `/` 포함.
fn split_path(path: &str) -> (String, String) {
    match path.rfind('/') {
        Some(i) => (path[..=i].to_string(), path[i + 1..].to_string()),
        None => (String::new(), path.to_string()),
    }
}

/// status → (표시 글리프, Tag variant). (jsx `ST` 매핑)
fn status_pill(s: FileStatus) -> (&'static str, TagVariant) {
    match s {
        FileStatus::Modified => ("M", TagVariant::Warning),
        FileStatus::Added => ("A", TagVariant::Success),
        FileStatus::Deleted => ("D", TagVariant::Danger),
        FileStatus::Renamed => ("R", TagVariant::Accent),
        FileStatus::Untracked => ("?", TagVariant::Default),
        FileStatus::Conflicted => ("U", TagVariant::Danger),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(summary: &str, author: &str) -> LogEntry {
        LogEntry {
            oid_short: "0123abc".to_string(),
            summary: summary.to_string(),
            author: author.to_string(),
            time: String::new(),
            refs: Vec::new(),
        }
    }

    fn translator(locale: &str) -> Translator {
        let lang_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("lang");
        Translator::load(&lang_dir, locale)
    }

    /// git-core 가 준 빈 값은 plugin 의 lang 키로 대체되고, 값이 있으면 그대로 쓴다.
    #[test]
    fn empty_summary_and_author_fall_back_to_lang_keys() {
        let tr = translator("en");
        let e = entry("", "");
        assert_eq!(summary_text(&tr, &e), "(no message)");
        assert_eq!(author_text(&tr, &e), "(unknown)");
        let e = entry("feat: x", "alice");
        assert_eq!(summary_text(&tr, &e), "feat: x");
        assert_eq!(author_text(&tr, &e), "alice");
    }

    /// 폴백 문구는 plugin 로케일을 따른다 — host 언어가 아니라 plugin 이 받은 `TASTY_LOCALE`.
    #[test]
    fn fallback_follows_plugin_locale() {
        let e = entry("", "");
        for locale in ["ko", "ja"] {
            let tr = translator(locale);
            let summary = summary_text(&tr, &e);
            let author = author_text(&tr, &e);
            assert_ne!(summary, "(no message)", "{locale}: summary 가 번역돼야 함");
            assert_ne!(author, "(unknown)", "{locale}: author 가 번역돼야 함");
            assert_ne!(summary, "git_viewer.no_message", "{locale}: 키 누락");
            assert_ne!(author, "git_viewer.unknown_author", "{locale}: 키 누락");
        }
    }
}
