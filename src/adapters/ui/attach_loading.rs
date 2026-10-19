//! Attach drafts and nonblocking response wait. No timer owns a hidden View.
use crate::model::{BinaryTree, LogicalPx, PhysicalRect};
use crate::runtime::engine_read::EngineRead;
use crate::state::MainViewState;
use crate::state::attach_pending::waits::{Wait, by_kind};
use std::time::{Duration, Instant};

pub(crate) fn draw(
    ctx: &egui::Context,
    state: &mut MainViewState,
    engine: &EngineRead<'_>,
    rect: PhysicalRect,
    scale: f32,
    notice_column_occupied: Option<egui::Rect>,
) {
    let theme = crate::theme::theme();
    let now = Instant::now();
    let delay = Duration::from_secs_f64(theme.loading_overlay_delay().to_secs_f64());
    for (id, mut physical, pane) in state.pending_regions(engine, rect, scale) {
        if pane {
            physical.y += state.tab_bar_height;
            physical.height -= state.tab_bar_height;
        }
        let bounds = super::to_egui_rect(physical, scale);
        let Some(entry) = state
            .attach_pending
            .entries
            .iter()
            .find(|entry| entry.id == id)
        else {
            continue;
        };
        if let Some(
            crate::state::attach_pending::Creation::Surface(direction)
            | crate::state::attach_pending::Creation::Pane(direction),
        ) = entry.creation
        {
            let gap = if pane {
                crate::model::PaneNode::border_width(scale)
            } else {
                crate::model::SurfaceLayout::border_width(scale)
            }
            .to_logical(scale);
            let divider = match direction {
                crate::model::SplitDirection::Vertical => egui::Rect::from_min_max(
                    egui::pos2((LogicalPx(bounds.left()) - gap).value(), bounds.top()),
                    egui::pos2(bounds.left(), bounds.bottom()),
                ),
                crate::model::SplitDirection::Horizontal => egui::Rect::from_min_max(
                    egui::pos2(bounds.left(), (LogicalPx(bounds.top()) - gap).value()),
                    egui::pos2(bounds.right(), bounds.top()),
                ),
            };
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new(("pending_divider", id.0)),
            ))
            .rect_filled(divider, 0.0, theme.separator.to_egui_premultiplied());
        }
        let elapsed = now.duration_since(entry.receipt.admitted);
        let output = egui::Area::new(egui::Id::new(("attach_draft", id.0)))
            .fixed_pos(bounds.min)
            .order(egui::Order::Middle)
            .show(ctx, |ui| {
                let (tile, response) = ui.allocate_exact_size(bounds.size(), egui::Sense::click());
                ui.painter().rect_filled(tile, 0.0, theme.bg_panel());
                if elapsed >= delay {
                    tasty_ui_widgets::paint_loading_overlay(
                        ui,
                        &theme,
                        tile,
                        tasty_ui_widgets::LoadingPlacement::Surface,
                        theme.bg_panel().to_egui(),
                    );
                } else {
                    ctx.request_repaint_after(delay - elapsed);
                }
                response.clicked()
            });
        if output.inner {
            state.attach_pending.select(id, &state.navigation);
        }
    }
    let workspace = state.active_workspace(engine).id;
    let mut times = state
        .attach_waits
        .get(&workspace)
        .cloned()
        .unwrap_or_default();
    times.extend(
        state
            .attach_pending
            .entries
            .iter()
            .filter(|entry| {
                entry.workspace == workspace && entry.epoch.is_active() && !entry.receipt.finished()
            })
            .map(|entry| Wait::new(entry.wait_kind, entry.receipt.admitted)),
    );
    let wait_delay = Duration::from_secs_f64(theme.pending_indicator_delay().to_secs_f64());
    let Some(summary) = tasty_remote::pending_request::WaitSummary::at(
        times.iter().map(|wait| wait.admitted),
        now,
        wait_delay,
    ) else {
        return;
    };
    ctx.request_repaint_after(summary.next_update);
    if !summary.visible {
        return;
    }
    let bounds = super::to_egui_rect(rect, scale);
    let inset = theme.notice_column_inset();
    let width = theme
        .notice_column_width()
        .min((LogicalPx(bounds.width()) - inset * 2.0).max(LogicalPx(0.0)));
    let label = if summary.count == 1 {
        crate::i18n::t("attach.pending.one").to_string()
    } else {
        crate::i18n::t_fmt("attach.pending.many", &summary.count.to_string())
    };
    let time = elapsed_label(summary.elapsed.as_secs());
    let details = by_kind(&times)
        .into_iter()
        .map(|(kind, since)| {
            format!(
                "{} · {}",
                crate::i18n::t(kind.label_key()),
                elapsed_label(now.saturating_duration_since(since).as_secs())
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let base_top = LogicalPx(bounds.top()) + state.tab_bar_height.to_logical(scale) + inset;
    let top = notice_column_occupied.map_or(base_top, |occupied| {
        base_top.max(LogicalPx(occupied.bottom()) + theme.notice_column_gap())
    });
    // Does not hide native child windows or register a keyboard/pointer target.
    egui::Area::new(egui::Id::new("attach_response_wait"))
        .order(egui::Order::Foreground)
        .interactable(false)
        .fixed_pos(egui::pos2(
            (LogicalPx(bounds.right()) - inset - width).value(),
            top.value(),
        ))
        .show(ctx, |ui| {
            ui.set_width(width.value());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                tasty_ui_widgets::PendingIndicator {
                    label: &label,
                    time: &time,
                    compact: false,
                    details: &details,
                }
                .show(ui, &theme);
            });
        });
}
fn elapsed_label(seconds: u64) -> String {
    let part = |key, n: u64| crate::i18n::t_fmt(key, &n.to_string());
    let padded = |key, n: u64| crate::i18n::t_fmt(key, &format!("{n:02}"));
    if seconds < 60 {
        part("attach.pending.seconds", seconds)
    } else if seconds < 3600 {
        format!(
            "{} {}",
            part("attach.pending.minutes", seconds / 60),
            padded("attach.pending.seconds", seconds % 60)
        )
    } else {
        format!(
            "{} {}",
            part("attach.pending.hours", seconds / 3600),
            padded("attach.pending.minutes", seconds % 3600 / 60)
        )
    }
}

/// 지연 시간을 넘겨 로딩 중인 사이드바 항목에 overlay를 그린다. 아직이면 그 시각에 다시 그리게 한다.
pub(crate) fn paint_sidebar_loading(
    ui: &mut egui::Ui,
    theme: &tasty_type_appearance::theme::Theme,
    since: Option<Instant>,
    rect: egui::Rect,
    placement: tasty_ui_widgets::LoadingPlacement,
) {
    if loading_visible(ui, theme, since) {
        let fill = theme.bg_sidebar().to_egui();
        tasty_ui_widgets::paint_loading_overlay(ui, theme, rect, placement, fill);
    }
}

fn loading_visible(
    ui: &egui::Ui,
    theme: &tasty_type_appearance::theme::Theme,
    since: Option<std::time::Instant>,
) -> bool {
    let Some(since) = since else {
        return false;
    };
    let elapsed = since.elapsed();
    let delay = std::time::Duration::from_secs_f64(theme.loading_overlay_delay().to_secs_f64());
    if elapsed < delay {
        ui.ctx().request_repaint_after(delay - elapsed);
        false
    } else {
        true
    }
}
