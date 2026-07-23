//! General › Remote transfer — 원격(mirror) 파일 전송 채널의 수신측 저장 정책
//! 편집. `RemoteTransferSettings{dir, max_mb}`(06/07 백엔드, 이미 merge)를 두 행으로
//! 편집한다.
//!
//! 디자인 구조 전사: `gallery/overlays-shared.jsx` `SettingsRemoteTransferFrame`
//! (design-request `design-request/remote-transfer-ui.md`). 콘텐츠 컬럼 =
//! mono uppercase 섹션 헤딩("Received files") + 150px 라벨 grid 2행(Save folder /
//! Maximum size), 각 행 아래 muted 설명 + 행 사이 separator. 갤러리 spec:
//! `gallery/overlays-windows.jsx` "Settings · General › Remote transfer".
//! Browse…/numeric input 페어링은 Scripts(`misc.rs`)·plugin number(`appearance.rs`)
//! 선례와 동형(rfd folder picker · mono text Input + 정수 파싱).

use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, Input, vspace};

use crate::adapters::ui::icons;
use crate::i18n::t;
use crate::settings::Settings;

/// 디자인 settings-row 라벨 컬럼 폭(`gridTemplateColumns: "150px 1fr"`). 4px 그리드
/// 밖 화면 전용 고정 치수(token-policy §c) — 대응 Theme 필드 없음.
const LABEL_COL_WIDTH: LogicalPx = LogicalPx(150.0);

pub fn draw_remote_transfer_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    // 섹션 헤딩 "Received files" — mono micro uppercase text-muted (misc new_card 관례).
    ui.label(
        egui::RichText::new(t("settings.remote_transfer.section").to_uppercase())
            .size(th.font_size_micro.value())
            .monospace()
            .color(th.text_muted()),
    );
    vspace(ui, th.spacing_sm);

    // ── 행 1: Save folder — mono path Input + Browse…(secondary, folder 아이콘) ──
    settings_row(ui, &th, t("settings.remote_transfer.dir"), |ui| {
        // 디자인: [Input flex:1][Browse flex:none], gap 8. right_to_left 로 Browse 를
        // 먼저(우측) 배치하고 Input 이 남은 폭을 채운다(misc add_card 선례).
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
            if Button::new(t("settings.remote_transfer.browse"))
                .variant(ButtonVariant::Secondary)
                .size(ControlSize::Sm)
                .leading_icon(&|ui, rect, c| {
                    icons::FOLDER.image(rect.width(), c).paint_at(ui, rect);
                })
                .show(ui, &th)
                .clicked()
                && let Some(path) = rfd::FileDialog::new().pick_folder()
            {
                settings.remote_transfer.dir = path.to_string_lossy().into_owned();
            }
            Input::new()
                .mono(true)
                .placeholder(t("settings.remote_transfer.dir_placeholder"))
                .show(ui, &th, &mut settings.remote_transfer.dir);
        });
    });
    row_desc(ui, &th, t("settings.remote_transfer.dir_desc"));
    row_separator(ui, &th);

    // ── 행 2: Maximum size — mono numeric Input + 필드 밖 정적 mono "MiB" suffix ──
    // 디자인 text Input 을 tasty numeric 으로 재현: 프레임 간 편집 버퍼를 egui 메모리에
    // 두고 유효 정수만 max_mb(u64)로 clamp 저장(draw_plugin_number 선례). "MiB" 는
    // Toast 의 " s" 처럼 필드 밖 정적 단위 리터럴(i18n 예외 — 단위 기호).
    let cur_mb = settings.remote_transfer.max_mb;
    let buf_id = egui::Id::new("remote_transfer_max_mb_buf");
    let mut buf = ui
        .data_mut(|d| d.get_temp::<String>(buf_id))
        .unwrap_or_else(|| cur_mb.to_string());
    settings_row(ui, &th, t("settings.remote_transfer.max_capacity"), |ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
            let resp = Input::new()
                .mono(true)
                .width(th.field_width_xs.value())
                .show(ui, &th, &mut buf);
            ui.label(
                egui::RichText::new("MiB")
                    .monospace()
                    .size(th.font_size_caption.value())
                    .color(th.text_muted()),
            );
            if !resp.has_focus() {
                // 편집 중이 아니면 버퍼를 저장값으로 동기화(초기 표시 + 포커스 아웃 정규화).
                let synced = cur_mb.to_string();
                if buf != synced {
                    buf = synced;
                }
            } else if resp.changed() {
                // 유효 정수만 저장(최소 1 MiB). 빈/무효 입력은 마지막 유효값 유지.
                if let Ok(parsed) = buf.trim().parse::<u64>() {
                    let clamped = parsed.max(1);
                    if clamped != cur_mb {
                        settings.remote_transfer.max_mb = clamped;
                    }
                }
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(buf_id, buf));
    row_desc(ui, &th, t("settings.remote_transfer.max_capacity_desc"));
}

/// settings-row 한 행: 150px 좌측 라벨 컬럼(수직 중앙) + `spacing_md`(12) gap +
/// 컨트롤. 행 높이는 `settings_row_min_height`(32) 하한.
fn settings_row(
    ui: &mut egui::Ui,
    th: &tasty_type_appearance::theme::Theme,
    label: &str,
    control: impl FnOnce(&mut egui::Ui),
) {
    let min_h = th.settings_row_min_height().value();
    ui.horizontal(|ui| {
        ui.set_min_height(min_h);
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.allocate_ui_with_layout(
            egui::vec2(LABEL_COL_WIDTH.value(), min_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(
                    egui::RichText::new(label)
                        .size(th.font_size_body.value())
                        .color(th.text_primary()),
                );
            },
        );
        ui.add_space(th.spacing_md.value());
        control(ui);
    });
}

/// 행 아래 muted 설명줄(caption · text-muted, 가용폭 wrap).
fn row_desc(ui: &mut egui::Ui, th: &tasty_type_appearance::theme::Theme, text: &str) {
    vspace(ui, th.spacing_xs);
    ui.label(
        egui::RichText::new(text)
            .size(th.font_size_caption.value())
            .color(th.text_muted()),
    );
}

/// 행 사이 1px separator(디자인 `borderTop: 1px solid separator`). base bg 위이므로
/// `th.separator`(misc ScriptRow 하단 보더와 동일 관례)로 hline.
fn row_separator(ui: &mut egui::Ui, th: &tasty_type_appearance::theme::Theme) {
    vspace(ui, th.spacing_sm);
    let w = ui.available_width();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(w, th.border_width.value()), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(th.border_width.value(), th.separator.to_egui()),
    );
    vspace(ui, th.spacing_sm);
}
