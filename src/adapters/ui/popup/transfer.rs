//! (09) 원격 파일 전송 피드백 팝업 2종 — 진행(progress) + 실패(error).
//!
//! 06 bulk 전송(ADR-0054) + 08 이미지 paste 업로드가 실제 전송을 담당하고, 이 모듈은
//! 그 전송에 대한 사용자 피드백 UI 를 PopupDef 로 제공한다(egui::Window 직접 사용 금지).
//! 갤러리 specimen `crates/tasty-gallery/.../components/transfer.rs` 의 본체 대응이다
//! (gallery-first — 갤러리에서 시각 확정 후 여기 반영).
//!
//! 디자인 canonical: `gallery/overlays-shared.jsx` `TransferProgressFrame`(09a) /
//! `TransferErrorFrame`(09b). scrim 중앙 headless 모달. 진행은 **시스템 최초 determinate
//! progress bar**(recessed 4px track = bg-app + accent fill, 0ms 무애니 — 바이트 수신
//! 시에만 fill 폭 이동).
//!
//! - progress(`transfer_progress`): `close_on_outside_click=false`, 모든 행 완료 시 self-close.
//! - error(`transfer_error`): 기본 dismiss(Esc/scrim), 전송 중 실패만 Retry.
//!
//! 상태 공급: `AppState.dialogs.transfer_progress`(진행 행 Vec) / `transfer_error`(실패 큐).
//! 08 워커가 진행 이벤트로 행을 갱신하고, 완료/실패 시 App 이 팝업을 open/close/승격한다.

use tasty_ui_widgets::{Button, ButtonVariant, ControlSize};

use crate::adapters::ui::icons;
use crate::adapters::ui::popup::PopupAction;
use crate::core::CoreState;
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;

pub const TRANSFER_PROGRESS_POPUP_ID: &str = "transfer_progress";
pub const TRANSFER_ERROR_POPUP_ID: &str = "transfer_error";

// ── 프레임 고정 치수 (디자인 raw px — 화면 전용 popup 좌표, token-policy §c) ──
/// `--tasty-transfer-popup-width` (size-400).
const FRAME_W: f32 = 400.0;
/// 헤더/푸터 가로 패딩 (디자인 14 — space 스텝 밖 raw).
const PAD_X: f32 = 14.0;
/// 헤더 세로 패딩 (디자인 12 = space-md).
const HEADER_PAD_Y: f32 = 12.0;
/// 바디 패딩 (디자인 14 — raw).
const BODY_PAD: f32 = 14.0;
/// 푸터 세로 패딩 (디자인 10 — raw).
const FOOTER_PAD_Y: f32 = 10.0;
/// 바디 내부 요소 gap (디자인 10 — raw).
const BODY_GAP: f32 = 10.0;
/// 헤더/푸터 콘텐츠 높이 근사(glyph 16 / 제목 14 line ≈ 20).
const HEADER_CONTENT_H: f32 = 20.0;

/// 진행 팝업 상태 — 진행 중인 파일 행들. 08 워커 진행 이벤트가 `row_by_id` 로 갱신한다.
#[derive(Debug, Default, Clone)]
pub struct TransferProgress {
    pub rows: Vec<TransferRow>,
}

impl TransferProgress {
    /// id 로 행을 찾아 가변 참조 반환(진행 이벤트 적용용).
    pub fn row_by_id(&mut self, id: u64) -> Option<&mut TransferRow> {
        self.rows.iter_mut().find(|r| r.id == id)
    }
}

/// 한 파일의 진행 상태.
#[derive(Debug, Clone)]
pub struct TransferRow {
    /// UI 상관 id(08 이 발급, 진행 이벤트가 이 id 로 행을 지목).
    pub id: u64,
    /// 표시 파일명(mono 말줄임).
    pub name: String,
    /// 지금까지 전송한 바이트.
    pub sent: u64,
    /// 총 바이트.
    pub total: u64,
    /// 표시용 전송 속도 문자열(예 "2.1 MiB/s"). 워커가 계산해 넣는다.
    pub rate: String,
}

/// 실패 팝업 큐 항목.
pub struct TransferError {
    /// 실패한 파일명.
    pub name: String,
    /// 실패 사유(원격 reason 또는 전송/프로토콜 에러 메시지).
    pub reason: String,
    /// `Some` = 전송 중 실패(재시도 가능) → Retry 버튼 + 재전송 페이로드. `None` = 원격
    /// 거부(07 capacity 등, 재시도 무의미) → Dismiss 단독. 판정은 08 이 [`BULK_REJECT_PREFIX`]
    /// 로 한다.
    ///
    /// [`BULK_REJECT_PREFIX`]: crate::app::attach_client::BULK_REJECT_PREFIX
    pub retry: Option<crate::core::PendingImageUpload>,
}

// ── PopupDef sizer (headless — title 미표시, title_fn 불필요) ──────────────

/// 진행 팝업 높이 = header + body(행 N개) + footer. 행 수에 맞춰 딱 맞게(빈 하단 방지).
pub fn transfer_progress_sizer(state: &AppState, _e: &CoreState) -> egui::Vec2 {
    let n = state
        .dialogs
        .transfer_progress
        .as_ref()
        .map(|p| p.rows.len())
        .unwrap_or(1)
        .max(1);
    let header_h = HEADER_PAD_Y * 2.0 + HEADER_CONTENT_H;
    let footer_h = FOOTER_PAD_Y * 2.0 + ControlSize::Sm.height(&theme::theme());
    // 행 하나: fileRow(~18) + gap + bar(4) + gap + statsRow(~15).
    let row_h = 18.0 + BODY_GAP + 4.0 + BODY_GAP + 15.0;
    let body_h = BODY_PAD * 2.0 + row_h * n as f32 + BODY_GAP * (n.saturating_sub(1)) as f32;
    egui::vec2(FRAME_W, header_h + body_h + footer_h)
}

/// 실패 팝업 높이 = header + body(prose + reason well) + footer. reason 길이로 well 줄수 추정.
pub fn transfer_error_sizer(state: &AppState, _e: &CoreState) -> egui::Vec2 {
    let th = theme::theme();
    let header_h = HEADER_PAD_Y * 2.0 + HEADER_CONTENT_H;
    let footer_h = FOOTER_PAD_Y * 2.0 + ControlSize::Sm.height(&th);
    let (name_len, reason_len) = state
        .dialogs
        .transfer_error
        .front()
        .map(|e| (e.name.chars().count(), e.reason.chars().count()))
        .unwrap_or((0, 0));
    // 본문 폭 372(400 − 2×14). 대략 문자당 ~7px → prose ~53자/줄, well ~50자/줄.
    let prose_lines = (((name_len + 22) as f32) / 53.0).ceil().max(1.0);
    let prose_h = prose_lines * th.font_size_body.value() * 1.5;
    let well_lines = ((reason_len as f32) / 50.0).ceil().max(1.0);
    // well: 패딩 8+8 + 텍스트 줄.
    let well_h = 16.0 + well_lines * th.font_size_caption.value() * 1.4;
    let body_h = BODY_PAD * 2.0 + prose_h + BODY_GAP + well_h;
    egui::vec2(FRAME_W, header_h + body_h + footer_h)
}

// ── draw_fn ────────────────────────────────────────────────────────────────

/// PopupDef::on_close entry point — 진행 상태 정리(backstop; Cancel/완료 경로가
/// 이미 비웠어도 무해). `src/app/image_upload.rs` 가 같은 정리를 직접 하는 중복이
/// 있는데, 훅으로 옮겨도 둘 다 `None` 으로 만들 뿐이라 무해하다 — 그 중복 제거는
/// 별건(open-time defensive reset 제거)이 담당한다.
pub fn on_close_transfer_progress(
    _ctx: &egui::Context,
    state: &mut AppState,
    _engine: &mut CoreState,
) {
    state.dialogs.transfer_progress = None;
}

/// 진행 팝업 draw_fn. 헤더(download + "Receiving file" + pct) → 파일 행들(파일명 +
/// determinate bar + done/total·rate) → ghost Cancel. 행이 없으면 self-close.
pub fn draw_transfer_progress(
    ui: &mut egui::Ui,
    state: &mut AppState,
    _engine: &mut CoreState,
) -> PopupAction {
    let th = theme::theme();
    let Some(progress) = state.dialogs.transfer_progress.as_ref() else {
        return PopupAction::Close;
    };
    if progress.rows.is_empty() {
        return PopupAction::Close;
    }
    // 헤더 pct = 첫 행 기준(단일 파일이 표준; 다중은 행별 pct 를 각 bar 가 보여줌).
    let head_pct = progress.rows.first().map(row_pct).unwrap_or(0);
    let rows = progress.rows.clone();

    let mut cancel = false;
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    ui.vertical(|ui| {
        ui.set_width(FRAME_W);
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        header_band(
            ui,
            &th,
            icons::DOWNLOAD,
            th.text_muted().into(),
            t("transfer.progress.title"),
            Some(&format!("{head_pct}%")),
        );
        body_region(ui, |ui| {
            for (i, row) in rows.iter().enumerate() {
                if i > 0 {
                    ui.add_space(BODY_GAP);
                }
                progress_row(ui, &th, row);
            }
        });
        cancel = footer_buttons(ui, &th, |ui| {
            Button::new(t("transfer.progress.cancel"))
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .show(ui, &th)
                .clicked()
        });
    });

    if cancel {
        // 진행 관망 중단(실제 전송은 abort 하지 않음 — 동기 워커라 중단 불가). 진행 상태를
        // 비워 완료 이벤트가 팝업을 재오픈하지 않게 한다. Ok 완료는 여전히 경로를 삽입한다.
        state.dialogs.transfer_progress = None;
        return PopupAction::Close;
    }
    PopupAction::None
}

/// PopupDef::on_close entry point — draw_fn 의 Dismiss/Retry 분기는 스스로
/// `pop_front()` 한 뒤 큐가 비었을 때만 Close 를 반환하므로(그 경우 여기 도달
/// 시점엔 이미 빈 큐), 이 훅에서 할 일이 남는 경우는 **scrim/외부 클릭처럼
/// draw_fn 을 거치지 않고 닫힌 경우뿐**이다 — 그때는 큐가 아직 안 비어 있으므로
/// head 를 여기서 대신 dismiss 하고, 남은 실패가 있으면 팝업을 다시 연다.
pub fn on_close_transfer_error(
    _ctx: &egui::Context,
    state: &mut AppState,
    _engine: &mut CoreState,
) {
    if state.dialogs.transfer_error.is_empty() {
        return;
    }
    state.dialogs.transfer_error.pop_front();
    if !state.dialogs.transfer_error.is_empty() {
        state.popups.open_centered_focused(TRANSFER_ERROR_POPUP_ID);
    }
}

/// 실패 팝업 draw_fn. 헤더(warn + "Transfer failed") → prose + reason well → Dismiss
/// /(전송 중 실패만)Retry. Esc/scrim = Dismiss. 큐가 비면 self-close.
pub fn draw_transfer_error(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut CoreState,
) -> PopupAction {
    let th = theme::theme();
    let Some(head) = state.dialogs.transfer_error.front() else {
        return PopupAction::Close;
    };
    let name = head.name.clone();
    let reason = head.reason.clone();
    let retryable = head.retry.is_some();

    let esc = ui
        .ctx()
        .input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter));

    let mut dismiss = esc;
    let mut retry = false;
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    ui.vertical(|ui| {
        ui.set_width(FRAME_W);
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        header_band(
            ui,
            &th,
            icons::ALERT_TRIANGLE,
            th.accent_danger().into(),
            t("transfer.error.title"),
            None,
        );
        body_region(ui, |ui| {
            // "<b>{name}</b> could not be received." — mono bold name + 산문.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label(
                    egui::RichText::new(&name)
                        .monospace()
                        .strong()
                        .size(th.font_size_body.value())
                        .color(th.text_primary()),
                );
                ui.label(
                    egui::RichText::new(t("transfer.error.body_suffix"))
                        .size(th.font_size_body.value())
                        .color(th.text_secondary()),
                );
            });
            ui.add_space(BODY_GAP);
            reason_well(ui, &th, &reason);
        });
        // danger-fill 금지 — ghost/secondary 만.
        footer_buttons(ui, &th, |ui| {
            if retryable {
                retry = Button::new(t("transfer.error.retry"))
                    .variant(ButtonVariant::Secondary)
                    .size(ControlSize::Sm)
                    .show(ui, &th)
                    .clicked();
                if Button::new(t("transfer.error.dismiss"))
                    .variant(ButtonVariant::Ghost)
                    .size(ControlSize::Sm)
                    .show(ui, &th)
                    .clicked()
                {
                    dismiss = true;
                }
            } else if Button::new(t("transfer.error.dismiss"))
                .variant(ButtonVariant::Secondary)
                .size(ControlSize::Sm)
                .show(ui, &th)
                .clicked()
            {
                dismiss = true;
            }
        });
    });

    if retry {
        // 전송 중 실패 재시도 — 저장해둔 페이로드를 기존 업로드 트리거 큐에 재투입한다
        // (clipboard.rs 가 쓰는 그 큐 — 새 트리거 경로가 아니라 재사용). 08 워커가 다시
        // 진행 팝업을 띄운다.
        if let Some(err) = state.dialogs.transfer_error.pop_front()
            && let Some(payload) = err.retry
        {
            engine.pending_image_uploads.push(payload);
        }
        return if state.dialogs.transfer_error.is_empty() {
            PopupAction::Close
        } else {
            PopupAction::None
        };
    }
    if dismiss {
        state.dialogs.transfer_error.pop_front();
        return if state.dialogs.transfer_error.is_empty() {
            PopupAction::Close
        } else {
            PopupAction::None
        };
    }
    PopupAction::None
}

// ── 그리기 헬퍼 (갤러리 specimen 과 동일 전사) ─────────────────────────────

/// 헤더 띠 — glyph + 제목(+ 우측 trailing mono). 하단 separator.
fn header_band(
    ui: &mut egui::Ui,
    th: &theme::Theme,
    glyph: icons::Icon,
    glyph_color: egui::Color32,
    title: &str,
    trailing: Option<&str>,
) {
    let band_h = HEADER_PAD_Y * 2.0 + HEADER_CONTENT_H;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(FRAME_W, band_h), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(th.border_width.value(), th.separator.to_egui()),
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + PAD_X, rect.top() + HEADER_PAD_Y),
        egui::pos2(rect.right() - PAD_X, rect.bottom() - HEADER_PAD_Y),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = th.spacing_sm.value();
    let gsz = th.icon_glyph_size_md.value();
    let (grect, _) = child.allocate_exact_size(egui::vec2(gsz, gsz), egui::Sense::hover());
    glyph.image(gsz, glyph_color).paint_at(&child, grect);
    child.label(
        egui::RichText::new(title)
            .size(th.font_size_max.value())
            .strong()
            .color(th.text_primary()),
    );
    if let Some(pct) = trailing {
        child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(pct)
                    .monospace()
                    .size(th.font_size_caption.value())
                    .color(th.text_muted()),
            );
        });
    }
}

/// 바디 region (padding 14, 전체폭).
fn body_region(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .inner_margin(egui::Margin::same(BODY_PAD as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui);
        });
}

/// 한 파일 진행 행 — 파일명 → determinate bar → done/total · rate.
fn progress_row(ui: &mut egui::Ui, th: &theme::Theme, row: &TransferRow) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        let gsz = th.icon_glyph_size_md.value();
        let (grect, _) = ui.allocate_exact_size(egui::vec2(gsz, gsz), egui::Sense::hover());
        icons::FILE
            .image(gsz, th.text_muted().into())
            .paint_at(ui, grect);
        // glyph 뒤 남은 가용폭에 맞춰 mono 말줄임(specimen elide_mono 와 동일 거동).
        let avail = ui.available_width();
        let name = elide_mono(ui, th, &row.name, avail);
        ui.label(
            egui::RichText::new(name)
                .monospace()
                .size(th.font_size_body.value())
                .color(th.text_primary()),
        );
    });
    ui.add_space(BODY_GAP);
    progress_bar(ui, th, row_pct(row));
    ui.add_space(BODY_GAP);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!(
                "{} / {}",
                format_mib(row.sent),
                format_mib(row.total)
            ))
            .monospace()
            .size(th.font_size_caption.value())
            .color(th.text_muted()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(&row.rate)
                    .monospace()
                    .size(th.font_size_caption.value())
                    .color(th.text_muted()),
            );
        });
    });
}

/// mono 문자열을 폭에 맞게 앞은 두고 뒤를 `…` 로 자른다(specimen elide_mono 와 동일).
fn elide_mono(ui: &egui::Ui, th: &theme::Theme, s: &str, max_w: f32) -> String {
    let font = egui::FontId::monospace(th.font_size_body.value());
    let w = |t: &str| {
        ui.fonts(|f| {
            f.layout_no_wrap(t.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
                .rect
                .width()
        })
    };
    if w(s) <= max_w {
        return s.to_owned();
    }
    let mut cut = s.chars().collect::<Vec<_>>();
    while !cut.is_empty() {
        cut.pop();
        let candidate: String = cut.iter().collect::<String>() + "…";
        if w(&candidate) <= max_w {
            return candidate;
        }
    }
    "…".to_owned()
}

/// determinate 4px progress bar — recessed track(bg-app) + accent fill(accent-primary),
/// 0ms 무애니(fill 폭 = pct). 토큰: height=size-4(spacing_xs) · radius=radius-sm.
fn progress_bar(ui: &mut egui::Ui, th: &theme::Theme, pct: u32) {
    let h = th.spacing_xs.value(); // progress-height = size-4 = 4
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let r = th.corner_radius_sm.value();
    ui.painter().rect_filled(rect, r, th.bg_app());
    let frac = (pct.min(100) as f32) / 100.0;
    if frac > 0.0 {
        let fill = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * frac, h));
        ui.painter().rect_filled(fill, r, th.accent_primary());
    }
}

/// command-well 패턴 — bg-app + 1px separator + radius, mono danger 텍스트.
fn reason_well(ui: &mut egui::Ui, th: &theme::Theme, reason: &str) {
    egui::Frame::new()
        .fill(th.bg_app().into())
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            th.separator.to_egui(),
        ))
        .corner_radius(th.corner_radius.value())
        .inner_margin(egui::Margin {
            left: 10,
            right: 10,
            top: 8,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                egui::RichText::new(reason)
                    .monospace()
                    .size(th.font_size_caption.value())
                    .color(th.accent_danger()),
            );
        });
}

/// 푸터 (padding 10/14, borderTop separator, 우측정렬). `add` 는 우→좌 순서로 위젯을
/// 넣고 원하는 클릭 결과(bool)를 반환한다.
fn footer_buttons<R>(
    ui: &mut egui::Ui,
    th: &theme::Theme,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let btn_h = ControlSize::Sm.height(th);
    let band_h = FOOTER_PAD_Y * 2.0 + btn_h;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(FRAME_W, band_h), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(th.border_width.value(), th.separator.to_egui()),
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + PAD_X, rect.top() + FOOTER_PAD_Y),
        egui::pos2(rect.right() - PAD_X, rect.bottom() - FOOTER_PAD_Y),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = th.spacing_sm.value();
    add(&mut child)
}

/// 행 진행률 % (총 0 이면 완료 취급 100).
fn row_pct(row: &TransferRow) -> u32 {
    if row.total == 0 {
        return 100;
    }
    ((row.sent.min(row.total) as f64 / row.total as f64) * 100.0).round() as u32
}

/// 바이트 → "12.3 MiB" 형식(1024 기반, 디자인 MiB 표기). B/KiB/MiB/GiB.
fn format_mib(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(sent: u64, total: u64) -> TransferRow {
        TransferRow {
            id: 1,
            name: "x".into(),
            sent,
            total,
            rate: String::new(),
        }
    }

    #[test]
    fn row_pct_zero_total_is_complete() {
        // 빈 파일(총 0) → 100%(0/0 나눗셈 회피).
        assert_eq!(row_pct(&row(0, 0)), 100);
    }

    #[test]
    fn row_pct_partial_and_clamped() {
        assert_eq!(row_pct(&row(0, 100)), 0);
        assert_eq!(row_pct(&row(27, 100)), 27);
        assert_eq!(row_pct(&row(100, 100)), 100);
        // sent > total 방어 → 100 클램프.
        assert_eq!(row_pct(&row(200, 100)), 100);
    }

    #[test]
    fn format_mib_units() {
        assert_eq!(format_mib(0), "0 B");
        assert_eq!(format_mib(512), "512 B");
        assert_eq!(format_mib(1024), "1.0 KiB");
        assert_eq!(format_mib(1024 * 1024), "1.0 MiB");
        assert_eq!(format_mib(1024 * 1024 * 1024), "1.0 GiB");
    }
}
