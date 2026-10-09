//! explorer 칸의 파일 작업 표시: 상태줄 진행, 대기열 팝오버, 결과 카드.
//! 작업 실행과 충돌 질문은 App 의 job 이 맡고, 이 모듈은 그 상태를 그리고 사용자 조작을 돌려준다.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use tasty_type_appearance::theme::Theme;
use tasty_type_appearance::toast_kind::ToastKind;
use tasty_ui_widgets::{
    ButtonVariant, OpStatusProps, QueueRowProps, ResultAction, ResultCardProps, ResultLine,
    op_queue_popover, op_status_line, result_card,
};

use super::ExplorerView;
use crate::adapters::ui::icons;
use crate::app::explorer_files::job::{OpKind, Reason, Report, Shared, UNKNOWN_BYTES, UndoStep};
use crate::core::fs_list::human_size;
use crate::i18n::{t, t_args, t_fmt, t_fmt2};

/// 결과 카드에 경로를 펼쳐 보이는 최대 수. 나머지는 "and n more" 로 줄인다.
const MAX_RESULT_LINES: usize = 3;

/// 이 칸이 시작해 지금 실행 중인 작업.
pub(crate) struct Running {
    pub(crate) shared: Arc<Shared>,
    pub(crate) kind: OpKind,
    pub(crate) dest: Option<PathBuf>,
    pub(crate) total: usize,
}

/// 이 칸이 요청해 차례를 기다리는 작업.
pub(crate) struct Queued {
    pub(crate) id: u64,
    pub(crate) kind: OpKind,
    pub(crate) count: usize,
    pub(crate) dest: Option<PathBuf>,
}

struct ResultCard {
    id: u64,
    report: Report,
    /// 되돌리기 결과면 되돌린 작업의 종류.
    undo_of: Option<OpKind>,
    /// 표준 시간 뒤 사라지는 카드. 실패가 있는 카드는 None 이라 닫을 때까지 남는다.
    expires: Option<Instant>,
}

#[derive(Default)]
pub(crate) struct OpsState {
    pub(crate) running: Option<Running>,
    pub(crate) queued: Vec<Queued>,
    results: Vec<ResultCard>,
    queue_open: bool,
    next_card: u64,
    /// 충돌 카드의 "남은 충돌에도 적용" 체크 상태.
    pub(crate) conflict_apply_all: bool,
    /// 직전 프레임에 잰 충돌 카드 높이. popup sizer 가 읽는다.
    pub(crate) conflict_height: f32,
    /// 충돌 카드에서 답하고 닫았다. 닫힘 정리가 다음 질문을 취소하지 않게 한다.
    pub(crate) conflict_answered: bool,
}

/// 렌더 뒤 호스트가 처리할 파일 작업 조작.
#[derive(Clone, Debug)]
pub enum OpsAction {
    /// 대기열에서 아직 시작하지 않은 작업을 뺀다.
    RemoveQueued(u64),
    /// 건너뛰었거나 실패한 항목을 같은 작업으로 다시 요청한다.
    Retry {
        kind: OpKind,
        paths: Vec<PathBuf>,
        dest: Option<PathBuf>,
    },
    /// 끝난 복사·이동을 되돌린다.
    Undo(Vec<UndoStep>),
    /// 기다리는 충돌 질문을 이 칸에 띄운다.
    ShowConflict,
}

/// 결과 카드의 수명 정책: 모두 끝났거나 취소한 카드만 표준 시간 뒤 사라진다.
pub(crate) fn result_is_timed(report: &Report) -> bool {
    hard_failures(report) == 0 && report.skipped.is_empty() && !report.trash_unavailable
}

impl OpsState {
    /// 끝난 작업의 결과 카드를 더한다. `expires` 가 None 이면 닫을 때까지 남는다.
    pub(crate) fn push_result(
        &mut self,
        report: Report,
        undo_of: Option<OpKind>,
        expires: Option<Instant>,
    ) {
        self.next_card += 1;
        self.results.push(ResultCard {
            id: self.next_card,
            report,
            undo_of,
            expires,
        });
    }
    /// 수명이 지난 카드를 지운다. 지운 카드가 있으면 true.
    pub(crate) fn expire(&mut self, now: Instant) -> bool {
        let before = self.results.len();
        self.results.retain(|c| c.expires.is_none_or(|at| at > now));
        before != self.results.len()
    }
    /// 다음으로 사라질 카드의 시각.
    pub(crate) fn next_expiry(&self) -> Option<Instant> {
        self.results.iter().filter_map(|c| c.expires).min()
    }
    #[cfg(test)]
    pub(crate) fn result_count(&self) -> usize {
        self.results.len()
    }
}

fn hard_failures(report: &Report) -> usize {
    report
        .failed
        .iter()
        .filter(|f| !matches!(f.reason, Reason::SourceNotRemoved(_)))
        .count()
}

fn name_of(path: Option<&Path>) -> String {
    path.map(|p| {
        p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.display().to_string())
    })
    .unwrap_or_default()
}

/// 홈 아래 경로를 `~` 로 줄여 보인다.
pub(crate) fn home_tilde(path: &Path) -> String {
    let home = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf());
    match home.as_deref().and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

fn glyph(icon: icons::Icon) -> impl Fn(&mut egui::Ui, egui::Rect, egui::Color32) {
    move |ui, rect, color| icon.image(rect.height(), color).paint_at(ui, rect)
}

// ── 상태줄 ─────────────────────────────────────────────────

/// 칸의 상태줄을 그린다. 이 칸의 작업이 돌고 있으면 진행을, 아니면 `normal` 을 그린다.
/// 결과 카드와 대기열 팝오버도 여기서 상태줄 위에 띄운다.
pub(crate) fn footer(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &mut ExplorerView,
    body: egui::Rect,
    action: &mut Option<super::super::ExplorerAction>,
    normal: fn(&mut egui::Ui, &Theme, &ExplorerView),
) {
    // 자세히 보기의 넓은 열이 본문 ui 를 칸보다 넓힐 수 있다. 칸에서 보이는 부분만 쓴다.
    let status_h = theme.item_height_interactive.value();
    let cell = egui::Rect::from_min_max(body.min, egui::pos2(body.max.x, body.max.y + status_h))
        .intersect(ui.clip_rect());
    if view.ops.running.is_none() {
        view.ops.queue_open = false;
        normal(ui, theme, view);
    } else {
        progress_line(ui, theme, &mut view.ops, cell.width(), action);
    }
    let now = Instant::now();
    view.ops.expire(now);
    if let Some(at) = view.ops.next_expiry() {
        ui.ctx()
            .request_repaint_after(at.saturating_duration_since(now));
    }
    let status_top = body.bottom();
    results(ui, theme, &mut view.ops, cell, status_top, action);
    if view.ops.queue_open {
        queue(ui, theme, &mut view.ops, cell, status_top, action);
    }
}

fn progress_line(
    ui: &mut egui::Ui,
    theme: &Theme,
    ops: &mut OpsState,
    width: f32,
    action: &mut Option<super::super::ExplorerAction>,
) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, theme.item_height_interactive.value()),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());
    let Some(running) = &ops.running else {
        return;
    };
    let snap = running.shared.snapshot();
    let ask = running.shared.pending_ask();
    let bytes_known = snap.bytes_total != UNKNOWN_BYTES && snap.bytes_total > 0;
    let fraction = bytes_known.then(|| snap.bytes_done as f32 / snap.bytes_total as f32);
    let text = match &ask {
        Some(ask) => t_fmt("explorer.op.waiting", &(ask.remaining + 1).to_string()),
        None => progress_text(running.kind, &snap),
    };
    let bytes = bytes_known.then(|| {
        t_fmt2(
            "explorer.op.bytes",
            &human_size(false, snap.bytes_done),
            &human_size(false, snap.bytes_total),
        )
    });
    let queued = (!ops.queued.is_empty())
        .then(|| t_fmt("explorer.op.queued", &ops.queued.len().to_string()));
    let close = glyph(icons::CLOSE);
    let resp = op_status_line(
        ui,
        theme,
        rect,
        &OpStatusProps {
            fraction,
            waiting: ask.is_some(),
            text: &text,
            bytes: bytes.as_deref(),
            queued: queued.as_deref(),
            show_label: t("explorer.op.show"),
            cancel_tip: t("explorer.op.cancel"),
        },
        &close,
    );
    if resp.cancel {
        running.shared.cancel();
    }
    if resp.show && action.is_none() {
        *action = Some(super::super::ExplorerAction::Ops(OpsAction::ShowConflict));
    }
    if resp.open_queue {
        ops.queue_open = !ops.queue_open;
    }
}

fn progress_text(kind: OpKind, snap: &crate::app::explorer_files::job::Snapshot) -> String {
    let key = match kind {
        OpKind::Copy => "explorer.op.copying",
        OpKind::Move => "explorer.op.moving",
        OpKind::Trash => "explorer.op.trashing",
        OpKind::Undo => "explorer.op.undoing",
    };
    let current = (snap.items_done + 1).min(snap.items_total.max(1));
    t_args(
        key,
        &[
            &current.to_string(),
            &snap.items_total.to_string(),
            &snap.current,
        ],
    )
}

// ── 대기열 팝오버 ──────────────────────────────────────────

fn queue_title(kind: OpKind, count: usize, dest: Option<&Path>) -> String {
    let n = count.to_string();
    match kind {
        OpKind::Copy => t_fmt2("explorer.op.queue_copy", &n, &name_of(dest)),
        OpKind::Move => t_fmt2("explorer.op.queue_move", &n, &name_of(dest)),
        OpKind::Trash => t_fmt("explorer.op.queue_trash", &n),
        OpKind::Undo => t_fmt("explorer.op.queue_undo", &n),
    }
}

fn queue(
    ui: &mut egui::Ui,
    theme: &Theme,
    ops: &mut OpsState,
    cell: egui::Rect,
    status_top: f32,
    action: &mut Option<super::super::ExplorerAction>,
) {
    let Some(running) = &ops.running else {
        return;
    };
    let snap = running.shared.snapshot();
    let mut titles = vec![queue_title(
        running.kind,
        running.total,
        running.dest.as_deref(),
    )];
    let mut sub = t_fmt2(
        "explorer.op.queue_progress",
        &(snap.items_done + 1)
            .min(snap.items_total.max(1))
            .to_string(),
        &snap.items_total.to_string(),
    );
    if snap.bytes_total != UNKNOWN_BYTES && snap.bytes_total > 0 {
        let bytes = t_fmt2(
            "explorer.op.bytes",
            &human_size(false, snap.bytes_done),
            &human_size(false, snap.bytes_total),
        );
        sub = format!("{sub} · {bytes}");
    }
    let mut subs = vec![sub];
    for q in &ops.queued {
        titles.push(queue_title(q.kind, q.count, q.dest.as_deref()));
        subs.push(t("explorer.op.queue_waiting").to_owned());
    }
    let rows: Vec<QueueRowProps<'_>> = titles
        .iter()
        .zip(&subs)
        .enumerate()
        .map(|(i, (title, sub))| QueueRowProps {
            title,
            sub,
            running: i == 0,
            remove_tip: if i == 0 {
                t("explorer.op.cancel")
            } else {
                t("explorer.op.remove_from_queue")
            },
        })
        .collect();
    let close = glyph(icons::CLOSE);
    let layers = glyph(icons::LAYERS);
    let removed = in_cell_area(ui, cell, "explorer_ops_queue", |ui| {
        op_queue_popover(
            ui,
            theme,
            ui.id().with("explorer_ops_queue_height"),
            egui::pos2(
                cell.right() - theme.spacing_sm.value(),
                status_top - theme.spacing_xs.value(),
            ),
            &rows,
            &close,
            &layers,
        )
        .1
    });
    match removed {
        Some(0) => running.shared.cancel(),
        Some(i) => {
            if let Some(q) = ops.queued.get(i - 1)
                && action.is_none()
            {
                *action = Some(super::super::ExplorerAction::Ops(OpsAction::RemoveQueued(
                    q.id,
                )));
            }
        }
        None => {}
    }
}

/// 칸 안에 묶인 위층에 그린다. 아래 목록이 같은 자리의 클릭을 받지 않는다.
fn in_cell_area<R>(
    ui: &egui::Ui,
    cell: egui::Rect,
    salt: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Area::new(ui.id().with(salt))
        .order(egui::Order::Foreground)
        .fixed_pos(cell.min)
        .constrain_to(cell)
        .show(ui.ctx(), |ui| {
            ui.set_clip_rect(cell);
            ui.set_max_size(cell.size());
            add(ui)
        })
        .inner
}

// ── 결과 카드 ──────────────────────────────────────────────

/// 결과 카드에 보일 문구.
struct CardText {
    kind: ToastKind,
    title: String,
    lines: Vec<(String, String)>,
    more: Option<String>,
    retry: Vec<PathBuf>,
    undo: bool,
}

fn reason_text(reason: &Reason) -> String {
    match reason {
        Reason::Os(e) => e.clone(),
        Reason::IntoItself => t("explorer.result.into_itself").to_owned(),
        Reason::SourceNotRemoved(e) => t_fmt("explorer.result.source_not_removed", e),
        Reason::NewerThere => t("explorer.result.newer_there").to_owned(),
        Reason::Gone => t("explorer.result.gone").to_owned(),
        Reason::Replaced => t("explorer.result.replaced").to_owned(),
    }
}

fn card_lines(report: &Report) -> (Vec<(String, String)>, Option<String>) {
    let all: Vec<(String, String)> = report
        .failed
        .iter()
        .map(|f| (home_tilde(&f.path), reason_text(&f.reason)))
        .chain(
            report
                .skipped
                .iter()
                .map(|p| (home_tilde(p), t("explorer.result.skipped").to_owned())),
        )
        .collect();
    let more = (all.len() > MAX_RESULT_LINES).then(|| {
        t_fmt(
            "explorer.result.more",
            &(all.len() - MAX_RESULT_LINES).to_string(),
        )
    });
    (all.into_iter().take(MAX_RESULT_LINES).collect(), more)
}

fn kind_key(
    kind: OpKind,
    copy: &'static str,
    moved: &'static str,
    trash: &'static str,
) -> &'static str {
    match kind {
        OpKind::Copy => copy,
        OpKind::Move | OpKind::Undo => moved,
        OpKind::Trash => trash,
    }
}

fn card_title(report: &Report) -> (ToastKind, String) {
    let (done, total) = (report.done.to_string(), report.total.to_string());
    let hard = hard_failures(report);
    let k = report.kind;
    if report.trash_unavailable {
        return (ToastKind::Error, t("explorer.trash.unavailable").to_owned());
    }
    if report.cancelled {
        let key = match k {
            OpKind::Copy => "explorer.result.cancelled_copy",
            OpKind::Move => "explorer.result.cancelled_move",
            OpKind::Trash => "explorer.result.cancelled_trash",
            OpKind::Undo => "explorer.result.cancelled_undo",
        };
        return (ToastKind::Info, t_fmt2(key, &done, &total));
    }
    if hard > 0 && report.done == 0 {
        let key = kind_key(
            k,
            "explorer.result.failed_copy",
            "explorer.result.failed_move",
            "explorer.result.failed_trash",
        );
        return (ToastKind::Error, t_fmt(key, &hard.to_string()));
    }
    if hard > 0 {
        let key = kind_key(
            k,
            "explorer.result.partial_copy",
            "explorer.result.partial_move",
            "explorer.result.partial_trash",
        );
        return (
            ToastKind::Warning,
            t_args(key, &[&done, &total, &hard.to_string()]),
        );
    }
    if !report.skipped.is_empty() {
        let key = kind_key(
            k,
            "explorer.result.skipped_copy",
            "explorer.result.skipped_move",
            "explorer.result.skipped_move",
        );
        let skipped = report.skipped.len().to_string();
        return (ToastKind::Warning, t_args(key, &[&done, &total, &skipped]));
    }
    let dest = name_of(report.dest.as_deref());
    let title = match k {
        OpKind::Copy => t_fmt2("explorer.result.copied", &done, &dest),
        OpKind::Trash => t_fmt("explorer.result.trashed", &done),
        OpKind::Move | OpKind::Undo => t_fmt2("explorer.result.moved", &done, &dest),
    };
    // 원본을 지우지 못한 이동은 사본이 남았으므로 경고로 알린다.
    let kind = if report.failed.is_empty() {
        ToastKind::Success
    } else {
        ToastKind::Warning
    };
    (kind, title)
}

fn undo_title(report: &Report, undo_of: OpKind) -> (ToastKind, String) {
    let failed = report.failed.len();
    let moved = undo_of == OpKind::Move;
    if report.cancelled {
        let (done, total) = (report.done.to_string(), report.total.to_string());
        return (
            ToastKind::Info,
            t_fmt2("explorer.result.cancelled_undo", &done, &total),
        );
    }
    if failed == 0 {
        let key = if moved {
            "explorer.result.undo_done_move"
        } else {
            "explorer.result.undo_done_copy"
        };
        return (ToastKind::Success, t(key).to_owned());
    }
    let key = if moved {
        "explorer.result.undo_partial_move"
    } else {
        "explorer.result.undo_partial_copy"
    };
    (ToastKind::Warning, t_fmt(key, &failed.to_string()))
}

fn card_text(card: &ResultCard) -> CardText {
    let report = &card.report;
    let (kind, title) = match card.undo_of {
        Some(of) => undo_title(report, of),
        None => card_title(report),
    };
    let (lines, more) = card_lines(report);
    let retry = if card.undo_of.is_some() || report.cancelled {
        Vec::new()
    } else {
        report.retryable()
    };
    let undo = card.undo_of.is_none()
        && !report.cancelled
        && matches!(report.kind, OpKind::Copy | OpKind::Move)
        && hard_failures(report) == 0
        && report.skipped.is_empty()
        && !report.undo.is_empty();
    CardText {
        kind,
        title,
        lines,
        more,
        retry,
        undo,
    }
}

/// 카드에서 고른 일.
enum CardPick {
    Retry,
    CopyPaths,
    Undo,
}

fn results(
    ui: &mut egui::Ui,
    theme: &Theme,
    ops: &mut OpsState,
    cell: egui::Rect,
    status_top: f32,
    action: &mut Option<super::super::ExplorerAction>,
) {
    if ops.results.is_empty() {
        return;
    }
    let width = theme.toast_max_width().value().min(cell.width());
    let texts: Vec<CardText> = ops.results.iter().map(card_text).collect();
    let picks = in_cell_area(ui, cell, "explorer_ops_results", |ui| {
        draw_cards(ui, theme, &texts, &ops.results, width, cell, status_top)
    });
    for (index, pick) in picks {
        apply_pick(ui, ops, &texts, index, pick, action);
    }
}

/// 새 카드를 아래에 두고 위로 쌓는다. 고른 일을 (카드 번호, 일) 로 돌려준다.
#[allow(clippy::too_many_arguments)]
fn draw_cards(
    ui: &mut egui::Ui,
    theme: &Theme,
    texts: &[CardText],
    cards: &[ResultCard],
    width: f32,
    cell: egui::Rect,
    status_top: f32,
) -> Vec<(usize, Option<CardPick>)> {
    let sm = theme.spacing_sm.value();
    let close = glyph(icons::CLOSE);
    let mut bottom = status_top - sm;
    let mut picks = Vec::new();
    for (index, text) in texts.iter().enumerate().rev() {
        let id = ui.id().with(("explorer_ops_card", cards[index].id));
        let h = ui.ctx().data(|d| d.get_temp::<f32>(id)).unwrap_or(0.0);
        let min = egui::pos2(cell.right() - sm - width, bottom - h);
        let mut child = ui.new_child(egui::UiBuilder::new().id_salt(id).max_rect(
            egui::Rect::from_min_size(min, egui::vec2(width, h.max(1.0))),
        ));
        let (rect, resp) = draw_card(&mut child, theme, text, width, &close);
        if (rect.height() - h).abs() > f32::EPSILON {
            ui.ctx().data_mut(|d| d.insert_temp(id, rect.height()));
            ui.ctx().request_repaint();
        }
        bottom -= rect.height() + sm;
        if resp.dismiss {
            picks.push((index, None));
        } else if let Some(i) = resp.action {
            picks.push((index, card_pick(text, i)));
        }
    }
    picks
}

/// 카드의 동작 버튼 순서: Retry, Copy paths, Undo 중 있는 것만.
fn card_actions(text: &CardText) -> Vec<(String, ButtonVariant, CardPick)> {
    let mut out = Vec::new();
    if !text.retry.is_empty() {
        out.push((
            t_fmt("explorer.result.retry", &text.retry.len().to_string()),
            ButtonVariant::Secondary,
            CardPick::Retry,
        ));
    }
    if !text.lines.is_empty() {
        out.push((
            t("explorer.result.copy_paths").to_owned(),
            ButtonVariant::Ghost,
            CardPick::CopyPaths,
        ));
    }
    if text.undo {
        out.push((
            t("explorer.result.undo").to_owned(),
            ButtonVariant::Ghost,
            CardPick::Undo,
        ));
    }
    out
}

fn card_pick(text: &CardText, index: usize) -> Option<CardPick> {
    card_actions(text).into_iter().nth(index).map(|(_, _, p)| p)
}

fn draw_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    text: &CardText,
    width: f32,
    close: &dyn Fn(&mut egui::Ui, egui::Rect, egui::Color32),
) -> (egui::Rect, tasty_ui_widgets::ResultCardResponse) {
    let lines: Vec<ResultLine<'_>> = text
        .lines
        .iter()
        .map(|(path, reason)| ResultLine { path, reason })
        .collect();
    let actions = card_actions(text);
    let actions: Vec<ResultAction<'_>> = actions
        .iter()
        .map(|(label, variant, _)| ResultAction {
            label,
            variant: *variant,
        })
        .collect();
    result_card(
        ui,
        theme,
        width,
        &ResultCardProps {
            kind: text.kind,
            title: &text.title,
            lines: &lines,
            more: text.more.as_deref(),
            actions: &actions,
            dismiss_tip: t("explorer.result.dismiss"),
        },
        close,
    )
}

fn apply_pick(
    ui: &egui::Ui,
    ops: &mut OpsState,
    texts: &[CardText],
    index: usize,
    pick: Option<CardPick>,
    action: &mut Option<super::super::ExplorerAction>,
) {
    let Some(card) = ops.results.get(index) else {
        return;
    };
    let id = card.id;
    match pick {
        None => ops.results.retain(|c| c.id != id),
        Some(CardPick::CopyPaths) => ui.ctx().copy_text(all_paths(&card.report)),
        Some(CardPick::Retry) if action.is_none() => {
            *action = Some(super::super::ExplorerAction::Ops(OpsAction::Retry {
                kind: card.report.kind,
                paths: texts[index].retry.clone(),
                dest: card.report.dest.clone(),
            }));
            ops.results.retain(|c| c.id != id);
        }
        Some(CardPick::Undo) if action.is_none() => {
            *action = Some(super::super::ExplorerAction::Ops(OpsAction::Undo(
                card.report.undo.clone(),
            )));
            ops.results.retain(|c| c.id != id);
        }
        Some(_) => {}
    }
}

/// 실패·건너뛴 항목의 전체 경로를 한 줄에 하나씩.
fn all_paths(report: &Report) -> String {
    report
        .failed
        .iter()
        .map(|f| &f.path)
        .chain(&report.skipped)
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
