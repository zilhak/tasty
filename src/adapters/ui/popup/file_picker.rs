//! Native file picker popup (04) — Tasty 자체 "파일 열기" 다이얼로그. 로컬/원격
//! (attach mirror workspace) 겸용 select-and-confirm 다이얼로그로, OS native 다이얼로그
//! (ADR-0042, native OS 다이얼로그 host 위임)이 원격 개념을 가질 수 없다는 근본
//! 한계를 신규 ADR(`docs/adr/0046-*.md`)로 보완한다.
//!
//! gallery specimen: `crates/tasty-gallery/src/catalog/components/file_picker.rs`
//! (mock 데이터로 독립 렌더 — 본체와 코드 공유는 하지 않는다, `file_handler_picker`
//! 갤러리 specimen과 동일 관례).
//!
//! ## Split: wrapper / view / action
//!
//! 순수 시각 [`draw_file_picker_view`] 는 [`FilePickerProps`] 만 받고
//! [`FilePickerAction`] 만 반환한다(AppState/CoreState 비의존). [`draw_file_picker`]
//! wrapper 가 runtime 상태에서 props 를 추출하고, mirror 판별(`Workspace.mirror`) +
//! 원격 조회 큐잉(`CoreState::pending_list_dir_forward`) + action → mutation 을 담당.
//!
//! ## 로컬/원격 데이터 흐름
//!
//! - **로컬**: `crate::core::fs_list::read_dir_entries` 를 wrapper 가 직접 동기 호출.
//! - **원격**: wrapper 가 `CoreState::pending_list_dir_forward` 에 요청을 push(popup
//!   상태를 `FpLoadState::Loading{request_id, sent_at}` 로 전이) → App 이
//!   `about_to_wait` 에서 attach 채널로 전송 → 원격이 같은 `read_dir_entries` 로
//!   처리 → `list_dir_result` 커스텀 이벤트로 회신 → reader thread 가
//!   `MirrorEvent::ListDirResult` 로 이 popup 상태에 직접 반영(`attach_client.rs`).
//!   soft timeout(응답 없음) 은 wrapper 가 매 프레임 `sent_at.elapsed()` 로 자체 판정.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tasty_type_geometry::length::LogicalPx;

use crate::adapters::ui::icons;

/// 경로 breadcrumb 의 구분자 글리프. 아이콘 스케일 밖(13) — 스케일의 12 와 14 사이다.
/// 어느 쪽으로 맞출지는 디자인 판단이라 스냅하지 않고 이름을 붙여 둔다(ADR-0126 과 같은
/// 처리). 갤러리 specimen 이 같은 값을 같은 이름으로 갖는다.
const CRUMB_GLYPH: LogicalPx = LogicalPx(13.0);
use crate::adapters::ui::popup::PopupAction;
use crate::i18n::t;
use crate::state::{AppState, FilePickerResult, FpLoadState};
use crate::theme::{self, Theme};
use tasty_ui_widgets::{Button, ButtonVariant, IconButton, IconButtonVariant, Spinner, hspace};

pub const FILE_PICKER_POPUP_ID: &str = "file_picker";

const POPUP_WIDTH: LogicalPx = LogicalPx(640.0);
const POPUP_HEIGHT: LogicalPx = LogicalPx(480.0);

// 중앙 블록 치수는 `tasty-ui-widgets::tokens` 가 단일 출처다 — 같은 이디엄을 쓰는
// `remote_attach` popup 과 갤러리 specimen 둘이 같은 상수를 읽는다.
use tasty_ui_widgets::tokens::{
    CENTER_BLOCK_H_POPUP as CENTER_BLOCK_H, CENTER_GLYPH_SIZE, STRUCT_GAP_2,
};
/// 원격 응답이 이 시간 안에 오지 않으면 `ErrorConn` 으로 전이(soft timeout — 세션의
/// `disconnected` 플래그만으론 "서버는 살아있는데 응답이 안 오는" 케이스를 못 잡는다).
const LIST_DIR_SOFT_TIMEOUT: Duration = Duration::from_secs(8);

/// PopupDef.sizer — 고정 640×480(gallery specimen `FRAME_W`/`FRAME_H`).
pub fn picker_sizer(_state: &AppState, _engine: &crate::core::CoreState) -> egui::Vec2 {
    egui::vec2(POPUP_WIDTH.value(), POPUP_HEIGHT.value())
}

/// 목록 한 행의 시각 입력. `DirEntryInfo` 를 그대로 쓰지 않는 이유는
/// `file_handler_picker` 관례와 동일 — 순수 view 가 표시용 pre-formatted 문자열만
/// 받게 해 gallery mock 에서도 안전하게 만들 수 있다.
#[derive(Clone)]
pub struct FilePickerEntryView {
    pub name: String,
    pub is_dir: bool,
    pub size_display: String,
    pub modified_display: String,
}

/// 순수 시각 view 의 로드 상태. gallery specimen `FpState` 와 1:1 대응.
#[derive(Clone, PartialEq, Eq)]
pub enum FpViewState {
    Loading,
    Empty,
    ErrorPerm(String),
    ErrorConn(String),
    Loaded,
}

/// 브레드크럼 한 항목의 표시 라벨.
#[derive(Clone)]
pub struct CrumbView {
    pub label: String,
}

/// 순수 시각 view 의 입력. AppState/CoreState 의존 없음.
pub struct FilePickerProps<'a> {
    pub theme: &'a Theme,
    /// `Some(host)` 면 헤더에 host 배지 렌더(원격 브라우징).
    pub remote_host: Option<&'a str>,
    pub crumbs: &'a [CrumbView],
    pub state: FpViewState,
    pub entries: &'a [FilePickerEntryView],
    /// 선택된 엔트리 이름(현재 디렉토리 기준).
    pub selected: &'a [String],
    pub name_filter_text: &'a str,
    /// 이 프레임에 Esc 를 소비할 자격이 있는가(규칙 7 의 키보드 판, ADR-0084).
    /// `false` 면 위에 다른 popup 이 있다는 뜻이라 Esc 를 무시한다 — 한 번의 Esc 로
    /// 스택 전체가 닫히는 것을 막는다. 판정은 `AppState.popup_escape_owner`.
    pub owns_escape: bool,

    // i18n — 호출처가 t() 로 미리 해상해서 전달(file_handler_picker.rs 관례).
    pub title_label: &'a str,
    pub name_field_label: &'a str,
    pub cancel_label: &'a str,
    pub open_label: &'a str,
    pub empty_label: &'a str,
    pub loading_label: &'a str,
    pub loading_body_local: &'a str,
    pub loading_body_remote: &'a str,
    pub error_perm_title: &'a str,
    pub error_perm_retry: &'a str,
    pub error_conn_title: &'a str,
    pub error_conn_reconnect: &'a str,
}

/// View 가 발생시킨 사용자 의도. Wrapper 가 mutation 으로 변환.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilePickerAction {
    None,
    /// ESC 또는 헤더 ✕/[취소] — popup 닫기 + result=Cancelled.
    Cancel,
    /// 목록 행 단일 클릭 — 선택만 갱신, popup 유지.
    Select(String),
    /// 디렉토리 행 더블클릭 — 그 하위로 내비게이트.
    NavigateInto(String),
    /// 브레드크럼 세그먼트 클릭 — 그 인덱스까지의 경로로 내비게이트.
    NavigateTo(usize),
    /// 상위 폴더로.
    NavigateUp,
    /// path bar 의 refresh 버튼 또는 에러 상태의 Retry/Reconnect 버튼.
    Refresh,
    /// [열기] 버튼 — 현재 `selected` 로 확정.
    Confirm,
    /// 파일 행 더블클릭 — 선택 상태와 무관하게 그 엔트리 하나로 즉시 확정.
    ConfirmEntry(String),
}

/// 순수 시각 view. AppState/CoreState/`theme::theme()` 비의존.
pub fn draw_file_picker_view(ui: &mut egui::Ui, props: &FilePickerProps<'_>) -> FilePickerAction {
    let ctx = ui.ctx().clone();
    if props.owns_escape && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return FilePickerAction::Cancel;
    }

    let th = props.theme;
    let mut action = FilePickerAction::None;

    // ── Header ────────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        ui.add(icons::FILE.image(th.icon_glyph_size_md.value(), th.text_muted().into()));
        ui.label(
            egui::RichText::new(props.title_label)
                .size(th.font_size_body.value())
                .strong()
                .color(th.text_primary()),
        );
        if let Some(host) = props.remote_host {
            host_badge(ui, th, host);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if IconButton::new()
                .variant(IconButtonVariant::Ghost)
                .show(ui, th, &|ui, rect, c| {
                    icons::CLOSE.image(rect.height(), c).paint_at(ui, rect)
                })
                .clicked()
            {
                action = FilePickerAction::Cancel;
            }
        });
    });
    ui.add_space(th.spacing_xs.value());
    hline(ui, th);

    // ── Path bar (breadcrumbs + refresh) ────────────────────────────────
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = STRUCT_GAP_2.value();
        for (i, crumb) in props.crumbs.iter().enumerate() {
            if i > 0 {
                ui.add(icons::CHEVRON_RIGHT.image(CRUMB_GLYPH.value(), th.text_disabled().into()));
            }
            let is_current = i + 1 == props.crumbs.len();
            let color = if is_current {
                th.text_primary()
            } else {
                th.accent_primary()
            };
            let mut rt = egui::RichText::new(&crumb.label)
                .size(th.font_size_caption.value())
                .color(color);
            if is_current {
                rt = rt.strong();
            }
            if is_current {
                ui.label(rt);
            } else if ui.link(rt).clicked() {
                action = FilePickerAction::NavigateTo(i);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if IconButton::new()
                .variant(IconButtonVariant::Ghost)
                .show(ui, th, &|ui, rect, c| {
                    icons::REFRESH.image(rect.height(), c).paint_at(ui, rect)
                })
                .clicked()
            {
                action = FilePickerAction::Refresh;
            }
            if IconButton::new()
                .variant(IconButtonVariant::Ghost)
                .show(ui, th, &|ui, rect, c| {
                    icons::CHEVRON_UP.image(rect.height(), c).paint_at(ui, rect)
                })
                .clicked()
            {
                action = FilePickerAction::NavigateUp;
            }
        });
    });
    ui.add_space(th.spacing_xs.value());
    hline(ui, th);

    // ── Body ─────────────────────────────────────────────────────────
    let body_height =
        (POPUP_HEIGHT - LogicalPx(44.0) - LogicalPx(36.0) - LogicalPx(84.0)).max(LogicalPx(60.0));
    match &props.state {
        FpViewState::Loaded => {
            egui::ScrollArea::vertical()
                .id_salt("file_picker_list")
                .max_height(body_height.value())
                .show(ui, |ui| {
                    for entry in props.entries {
                        let selected = props.selected.iter().any(|s| s == &entry.name);
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 28.0),
                            egui::Sense::click(),
                        );
                        if selected {
                            ui.painter()
                                .rect_filled(rect, 0.0, th.surface_active().to_egui());
                        } else if resp.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                0.0,
                                th.hover_overlay.to_egui_premultiplied(),
                            );
                        }
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let (glyph, glyph_color) = if entry.is_dir {
                            (icons::FOLDER, th.accent_primary())
                        } else {
                            (icons::FILE, th.text_muted())
                        };
                        let glyph_size = th.icon_glyph_size_md.value();
                        let icon_rect = egui::Rect::from_min_size(
                            egui::pos2(rect.left() + 8.0, rect.center().y - glyph_size * 0.5),
                            egui::vec2(glyph_size, glyph_size),
                        );
                        glyph
                            .image(glyph_size, glyph_color.into())
                            .paint_at(ui, icon_rect);
                        ui.painter().text(
                            egui::pos2(icon_rect.right() + 6.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            &entry.name,
                            egui::FontId::proportional(th.font_size_body.value()),
                            if selected {
                                th.text_primary().into()
                            } else {
                                th.text_secondary().into()
                            },
                        );
                        let mono = egui::FontId::monospace(th.font_size_caption.value());
                        ui.painter().text(
                            egui::pos2(rect.right() - 116.0, rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            &entry.size_display,
                            mono.clone(),
                            th.text_muted().into(),
                        );
                        ui.painter().text(
                            egui::pos2(rect.right() - 8.0, rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            &entry.modified_display,
                            mono,
                            th.text_muted().into(),
                        );
                        if resp.double_clicked() {
                            action = if entry.is_dir {
                                FilePickerAction::NavigateInto(entry.name.clone())
                            } else {
                                FilePickerAction::ConfirmEntry(entry.name.clone())
                            };
                        } else if resp.clicked() && matches!(action, FilePickerAction::None) {
                            action = FilePickerAction::Select(entry.name.clone());
                        }
                    }
                });
        }
        FpViewState::Loading => {
            center_state(
                ui,
                th,
                body_height,
                CenterGlyph::Spinner,
                props.loading_label,
                if props.remote_host.is_some() {
                    Some(props.loading_body_remote)
                } else {
                    Some(props.loading_body_local)
                },
                None,
            );
        }
        FpViewState::Empty => {
            center_state(
                ui,
                th,
                body_height,
                CenterGlyph::Icon(icons::FOLDER_OPEN, th.text_placeholder().into()),
                props.empty_label,
                None,
                None,
            );
        }
        FpViewState::ErrorPerm(reason) => {
            let retry = center_state(
                ui,
                th,
                body_height,
                CenterGlyph::Icon(icons::ALERT_TRIANGLE, th.accent_danger().into()),
                props.error_perm_title,
                Some(reason.as_str()),
                Some(props.error_perm_retry),
            );
            if retry {
                action = FilePickerAction::Refresh;
            }
        }
        FpViewState::ErrorConn(reason) => {
            let retry = center_state(
                ui,
                th,
                body_height,
                CenterGlyph::Icon(icons::ALERT_TRIANGLE, th.accent_danger().into()),
                props.error_conn_title,
                Some(reason.as_str()),
                Some(props.error_conn_reconnect),
            );
            if retry {
                action = FilePickerAction::Refresh;
            }
        }
    }

    // ── Footer ───────────────────────────────────────────────────────
    ui.add_space(th.spacing_xs.value());
    hline(ui, th);
    ui.add_space(th.spacing_sm.value());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        ui.label(
            egui::RichText::new(props.name_field_label)
                .size(th.font_size_caption.value())
                .color(th.text_muted()),
        );
        ui.label(
            egui::RichText::new(props.name_filter_text)
                .size(th.font_size_caption.value())
                .monospace()
                .color(th.text_secondary()),
        );
    });
    ui.add_space(th.spacing_sm.value());
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // 디렉토리는 [열기] 로 확정할 수 없다 — 더블클릭으로 진입해야 한다.
            // 단일 선택만이 아니라 selected 전원이 파일이어야 활성화(멀티 선택 확장 대비).
            let can_open = matches!(props.state, FpViewState::Loaded)
                && !props.selected.is_empty()
                && props.selected.iter().all(|name| {
                    props
                        .entries
                        .iter()
                        .find(|e| &e.name == name)
                        .is_some_and(|e| !e.is_dir)
                });
            ui.add_enabled_ui(can_open, |ui| {
                if ui.button(props.open_label).clicked() {
                    action = FilePickerAction::Confirm;
                }
            });
            if ui.button(props.cancel_label).clicked() {
                action = FilePickerAction::Cancel;
            }
        });
    });

    action
}

enum CenterGlyph {
    Spinner,
    Icon(icons::Icon, egui::Color32),
}

/// 로딩/빈/에러 상태 공통 렌더. Retry/Reconnect 버튼 클릭 시 `true`.
fn center_state(
    ui: &mut egui::Ui,
    th: &Theme,
    body_height: LogicalPx,
    glyph: CenterGlyph,
    heading: &str,
    body_text: Option<&str>,
    action_label: Option<&str>,
) -> bool {
    let mut clicked = false;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), body_height.value()),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.add_space(
                (body_height - LogicalPx(CENTER_BLOCK_H))
                    .max(LogicalPx(0.0))
                    .scaled(0.5)
                    .value(),
            );
            ui.spacing_mut().item_spacing.y = th.spacing_sm.value();
            match glyph {
                CenterGlyph::Spinner => {
                    Spinner::new().size(CENTER_GLYPH_SIZE).show(ui, th);
                }
                CenterGlyph::Icon(icon, color) => {
                    ui.add(icon.image(CENTER_GLYPH_SIZE, color));
                }
            }
            ui.label(
                egui::RichText::new(heading)
                    .size(th.font_size_body.value())
                    .strong()
                    .color(th.text_primary()),
            );
            if let Some(b) = body_text {
                ui.set_max_width(th.file_picker_note_max_width().value());
                ui.label(
                    egui::RichText::new(b)
                        .size(th.font_size_caption.value())
                        .color(th.text_muted()),
                );
            }
            if let Some(label) = action_label {
                ui.add_space(th.spacing_xs.value());
                if Button::new(label)
                    .variant(ButtonVariant::Secondary)
                    .show(ui, th)
                    .clicked()
                {
                    clicked = true;
                }
            }
        },
    );
    clicked
}

/// 원격 host 배지(§6.1 A안, gallery specimen 채택) — mono `user@host` 칩.
fn host_badge(ui: &mut egui::Ui, th: &Theme, host: &str) {
    hspace(ui, th.spacing_sm);
    let info = th.accent_info();
    let font = egui::FontId::monospace(th.font_size_caption.value());
    let galley = ui
        .painter()
        .layout_no_wrap(host.to_owned(), font, egui::Color32::PLACEHOLDER);
    let glyph = th.icon_glyph_size_xs.value();
    let gap = th.spacing_xs.value();
    let pad_x = th.spacing_sm.value();
    let h = 22.0;
    let w = pad_x * 2.0 + glyph + gap + galley.rect.width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let radius = th.corner_radius.value();
    let info_color: egui::Color32 = info.into();
    ui.painter()
        .rect_filled(rect, radius, info_color.gamma_multiply(0.14));
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(th.border_width.value(), info_color.gamma_multiply(0.45)),
        egui::StrokeKind::Inside,
    );
    let gy = egui::Rect::from_min_size(
        egui::pos2(rect.left() + pad_x, rect.center().y - glyph * 0.5),
        egui::vec2(glyph, glyph),
    );
    icons::REMOTE.image(glyph, info_color).paint_at(ui, gy);
    let pos = egui::pos2(
        rect.left() + pad_x + glyph + gap,
        rect.center().y - galley.rect.height() * 0.5,
    );
    ui.painter().galley(pos, galley, info_color);
}

fn hline(ui: &mut egui::Ui, th: &Theme) {
    let rect = ui.available_rect_before_wrap();
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(th.border_width.value(), th.separator),
    );
}

/// PopupDef::on_close entry point — file_handler_picker 와 동일 관례: X 버튼/외부
/// 닫기처럼 dispatch 없이 닫히면 `result` 가 아직 `None` 일 수 있다. 미확정이면
/// Cancelled 로 명시해 호스트 본체의 result-drain 이 대기 상태로 남지 않게 한다.
pub fn on_close_file_picker(
    _ctx: &egui::Context,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) {
    if let Some(p) = state.dialogs.file_picker.as_mut()
        && p.result.is_none()
    {
        p.result = Some(crate::state::FilePickerResult::Cancelled);
    }
}

/// PopupDef.draw_fn — runtime wrapper. props 추출 + view 호출 + action → mutation.
pub fn draw_file_picker(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    let Some(data) = state.dialogs.file_picker.as_ref() else {
        return PopupAction::Close;
    };

    // 원격 mirror 워크스페이스가 사라졌으면(disconnected 정리 — attach_client.rs
    // cleanup_mirror_workspace) 응답을 영영 못 받으므로 ErrorConn 으로 전이한다.
    // 세션의 raw `disconnected` 플래그는 App 소유(popup wrapper 도달 불가)라, 이
    // "mirror workspace 자체가 사라졌다"는 더 상위의 관측 가능한 결과로 판별한다.
    if let Some(mirror_ws_id) = data.mirror_ws_id
        && engine.find_workspace_index_for_id(mirror_ws_id).is_none()
        && !matches!(data.load, FpLoadState::ErrorConn(_))
    {
        let data = state.dialogs.file_picker.as_mut().unwrap();
        data.load = FpLoadState::ErrorConn(t("filepicker.error_conn.session_lost").to_string());
    }

    // soft timeout: Loading 상태에서 일정 시간 응답이 없으면 ErrorConn 전이.
    if let Some(FpLoadState::Loading { sent_at, .. }) =
        state.dialogs.file_picker.as_ref().map(|d| d.load.clone())
        && sent_at.elapsed() > LIST_DIR_SOFT_TIMEOUT
    {
        let data = state.dialogs.file_picker.as_mut().unwrap();
        data.load = FpLoadState::ErrorConn(t("filepicker.error_conn.timeout").to_string());
    }

    let th = theme::theme();
    // Esc 소유권은 popup 매니저가 프레임 초입에 정한다(ADR-0084) — 여기서 다시
    // 계산하지 않고 그 판정을 읽기만 한다.
    let owns_escape = state.popup_escape_owner == Some(FILE_PICKER_POPUP_ID);
    let data = state.dialogs.file_picker.as_ref().unwrap();

    let is_remote = data.mirror_ws_id.is_some();
    let crumb_targets = path_ancestors(is_remote, &data.current_dir);
    let crumbs: Vec<CrumbView> = crumb_targets
        .iter()
        .map(|full| CrumbView {
            label: crumb_label(is_remote, full),
        })
        .collect();

    let view_entries: Vec<FilePickerEntryView> = data
        .entries
        .iter()
        .filter(|e| e.is_dir || matches_filters(&data.filters, &e.name))
        .map(|e| FilePickerEntryView {
            name: e.name.clone(),
            is_dir: e.is_dir,
            size_display: crate::core::fs_list::human_size(e.is_dir, e.size),
            modified_display: crate::core::fs_list::format_modified(e.modified),
        })
        .collect();

    let view_state = match &data.load {
        FpLoadState::Loading { .. } => FpViewState::Loading,
        FpLoadState::Loaded => FpViewState::Loaded,
        FpLoadState::Empty => FpViewState::Empty,
        FpLoadState::ErrorPerm(r) => FpViewState::ErrorPerm(r.clone()),
        FpLoadState::ErrorConn(r) => FpViewState::ErrorConn(r.clone()),
    };

    let name_filter_text = data.selected.join(", ");
    let title_label = t("filepicker.title");
    let name_field_label = t("filepicker.name_field_label");
    let cancel_label = t("button.cancel");
    let open_label = t("filepicker.open_button");
    let empty_label = t("filepicker.empty");
    let loading_label = t("filepicker.loading");
    let loading_body_local = t("filepicker.loading_body_local");
    let loading_body_remote = t("filepicker.loading_body_remote");
    let error_perm_title = t("filepicker.error_perm.title");
    let error_perm_retry = t("filepicker.error_perm.retry");
    let error_conn_title = t("filepicker.error_conn.title");
    let error_conn_reconnect = t("filepicker.error_conn.reconnect");

    let props = FilePickerProps {
        theme: &th,
        remote_host: data.remote_host.as_deref(),
        crumbs: &crumbs,
        state: view_state,
        entries: &view_entries,
        selected: &data.selected,
        name_filter_text: &name_filter_text,
        owns_escape,
        title_label,
        name_field_label,
        cancel_label,
        open_label,
        empty_label,
        loading_label,
        loading_body_local,
        loading_body_remote,
        error_perm_title,
        error_perm_retry,
        error_conn_title,
        error_conn_reconnect,
    };

    let action = draw_file_picker_view(ui, &props);
    apply_action(state, engine, action)
}

fn apply_action(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    action: FilePickerAction,
) -> PopupAction {
    match action {
        FilePickerAction::None => PopupAction::None,
        FilePickerAction::Cancel => {
            if let Some(d) = state.dialogs.file_picker.as_mut() {
                d.result = Some(FilePickerResult::Cancelled);
            }
            PopupAction::Close
        }
        FilePickerAction::Select(name) => {
            if let Some(d) = state.dialogs.file_picker.as_mut() {
                d.selected = vec![name];
            }
            PopupAction::None
        }
        FilePickerAction::NavigateInto(name) => {
            navigate(state, engine, |dir, is_remote| {
                join_dir(is_remote, dir, &name)
            });
            PopupAction::None
        }
        FilePickerAction::NavigateTo(idx) => {
            if let Some(d) = state.dialogs.file_picker.as_ref() {
                let is_remote = d.mirror_ws_id.is_some();
                let targets = path_ancestors(is_remote, &d.current_dir);
                if let Some(target) = targets.get(idx).cloned() {
                    navigate(state, engine, move |_dir, _is_remote| target.clone());
                }
            }
            PopupAction::None
        }
        FilePickerAction::NavigateUp => {
            if let Some(d) = state.dialogs.file_picker.as_ref() {
                let is_remote = d.mirror_ws_id.is_some();
                let targets = path_ancestors(is_remote, &d.current_dir);
                if targets.len() > 1 {
                    let parent = targets[targets.len() - 2].clone();
                    navigate(state, engine, move |_dir, _is_remote| parent.clone());
                }
            }
            PopupAction::None
        }
        FilePickerAction::Refresh => {
            navigate(state, engine, |dir, _is_remote| dir.to_string());
            PopupAction::None
        }
        FilePickerAction::Confirm => {
            if let Some(d) = state.dialogs.file_picker.as_mut() {
                let is_remote = d.mirror_ws_id.is_some();
                // 방어적 재확인 — view 의 `can_open` 게이트를 우회해도(예: 향후 다른
                // 호출 경로) 디렉토리를 파일로 확정하지 않는다.
                let all_files = d.selected.iter().all(|name| {
                    d.entries
                        .iter()
                        .find(|e| &e.name == name)
                        .is_some_and(|e| !e.is_dir)
                });
                let paths: Vec<String> = d
                    .selected
                    .iter()
                    .map(|name| join_dir(is_remote, &d.current_dir, name))
                    .collect();
                if all_files && !paths.is_empty() {
                    d.result = Some(FilePickerResult::Confirmed { paths, is_remote });
                    return PopupAction::Close;
                }
            }
            PopupAction::None
        }
        FilePickerAction::ConfirmEntry(name) => {
            if let Some(d) = state.dialogs.file_picker.as_mut() {
                let is_remote = d.mirror_ws_id.is_some();
                let path = join_dir(is_remote, &d.current_dir, &name);
                d.result = Some(FilePickerResult::Confirmed {
                    paths: vec![path],
                    is_remote,
                });
                return PopupAction::Close;
            }
            PopupAction::None
        }
    }
}

/// Tools 메뉴 항목 클릭 또는 `file_picker.trigger` IPC(ADR-0058) 진입점 —
/// 현재 활성 workspace 가 mirror 인지로 로컬/원격을 판별해 [`crate::state::FilePickerData`]
/// 를 채우고 popup 을 연다. 원격이면 `navigate` 가 `pending_list_dir_forward` 를
/// 큐잉(홈 디렉토리 = 빈 `dir`), 로컬이면 즉시 동기 로드한다.
///
/// `requester`: `Some` 이면 `file_picker.trigger` 로 이 popup 을 연 plugin — 확정/취소
/// 시 `app::dispatch::file_picker` 가 `"file_picker.result"` 이벤트를 이 plugin 에만
/// push 한다(ADR-0058 Decision 4). Tools 메뉴는 `None`.
/// `filters`: 확장자 필터(점 없이) — 비면 필터 없음.
pub fn open(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    requester: Option<crate::state::FilePickerRequester>,
    filters: Vec<String>,
) {
    let mirror_ws_id = engine
        .workspaces
        .get(state.active_workspace)
        .filter(|ws| ws.mirror)
        .map(|ws| ws.id);

    let initial_dir = if mirror_ws_id.is_some() {
        // 빈 dir → 서버(`list_dir_for_request`)가 원격 홈 디렉토리로 해석.
        String::new()
    } else {
        directories::BaseDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("/"))
            .to_string_lossy()
            .to_string()
    };

    state.dialogs.file_picker = Some(crate::state::FilePickerData {
        mirror_ws_id,
        remote_host: None,
        current_dir: initial_dir,
        load: FpLoadState::Empty,
        entries: Vec::new(),
        selected: Vec::new(),
        result: None,
        requester,
        filters,
    });

    navigate(state, engine, |dir, _is_remote| dir.to_string());

    let open_popup = crate::intent::UiIntent::OpenPopup {
        id: FILE_PICKER_POPUP_ID,
        mode: crate::intent::OpenPopupMode::CenteredFocused,
    };
    let dispatched = match state
        .dialogs
        .file_picker
        .as_ref()
        .and_then(|d| d.requester.as_ref())
    {
        Some(req) => open_popup.from_agent_plugin(req.plugin_id.clone()),
        None => open_popup.from_user_menu("tools_menu"),
    };
    state.dispatch_intent(dispatched);
}

/// 파일명이 확장자 필터에 매치하는지 — 대소문자 무시, 점 없는 확장자 비교.
/// `filters` 가 비면 항상 통과(필터 없음).
fn matches_filters(filters: &[String], name: &str) -> bool {
    if filters.is_empty() {
        return true;
    }
    let Some(ext) = Path::new(name).extension().and_then(|e| e.to_str()) else {
        return false;
    };
    filters.iter().any(|f| f.eq_ignore_ascii_case(ext))
}

/// 새 대상 디렉토리로 내비게이트: 선택 초기화 + 로컬은 즉시 동기 로드, 원격은
/// `pending_list_dir_forward` 큐잉 + `Loading` 전이.
fn navigate(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    target_of: impl FnOnce(&str, bool) -> String,
) {
    let Some(d) = state.dialogs.file_picker.as_mut() else {
        return;
    };
    let is_remote = d.mirror_ws_id.is_some();
    let target = target_of(&d.current_dir, is_remote);
    d.current_dir = target.clone();
    d.selected.clear();

    if let Some(mirror_ws_id) = d.mirror_ws_id {
        let request_id = crate::core::next_list_dir_request_id();
        d.load = FpLoadState::Loading {
            request_id,
            sent_at: Instant::now(),
        };
        engine
            .pending_list_dir_forward
            .push(crate::core::PendingListDirForward {
                local_ws_id: mirror_ws_id,
                request_id,
                dir: target,
                consumer: None,
            });
        return;
    }

    match crate::core::fs_list::read_dir_entries(Path::new(&target)) {
        Ok(mut entries) => {
            crate::core::fs_list::sort_entries(
                &mut entries,
                tasty_model::SortColumn::Name,
                tasty_model::SortDir::Asc,
            );
            d.load = if entries.is_empty() {
                FpLoadState::Empty
            } else {
                FpLoadState::Loaded
            };
            d.entries = entries;
        }
        Err(e) => {
            let msg = if e.kind() == std::io::ErrorKind::PermissionDenied {
                t("filepicker.error_perm.reason_permission").to_string()
            } else {
                e.to_string()
            };
            d.entries.clear();
            d.load = FpLoadState::ErrorPerm(msg);
        }
    }
}

/// 원격 경로가 Windows 스타일(`C:\Users\alice`)인지 — 원격 host OS 는 client 가
/// 미리 알 수 없으므로, 서버가 돌려준 경로 문자열 자체에 `\` 가 있는지로 판별한다.
/// POSIX 경로는 `\` 를 파일명에 거의 쓰지 않으므로 이 휴리스틱으로 충분하다.
fn is_windows_style_remote_path(p: &str) -> bool {
    p.contains('\\')
}

fn join_dir(is_remote: bool, dir: &str, name: &str) -> String {
    if is_remote {
        if is_windows_style_remote_path(dir) {
            if dir.ends_with('\\') {
                format!("{dir}{name}")
            } else {
                format!("{dir}\\{name}")
            }
        } else if dir.ends_with('/') {
            format!("{dir}{name}")
        } else {
            format!("{dir}/{name}")
        }
    } else {
        Path::new(dir).join(name).to_string_lossy().to_string()
    }
}

/// root 부터 현재 경로까지의 조상 목록(문자열 형태) — 브레드크럼 라벨/내비게이션
/// 타깃 둘 다 이 목록에서 유도한다. 로컬은 `Path` 컴포넌트 기반이라 Windows 드라이브
/// 루트도 정확히 다룬다. 원격은 문자열 분해인데, `is_windows_style_remote_path` 로
/// POSIX(`/`)와 Windows(`\`, 드라이브 루트 보존) 를 분기한다.
fn path_ancestors(is_remote: bool, current_dir: &str) -> Vec<String> {
    if is_remote {
        if is_windows_style_remote_path(current_dir) {
            let mut segs = current_dir.split('\\').filter(|s| !s.is_empty());
            let Some(drive) = segs.next() else {
                return vec![current_dir.to_string()];
            };
            let root = format!("{drive}\\");
            let mut out = vec![root.clone()];
            let mut acc = root;
            for seg in segs {
                if !acc.ends_with('\\') {
                    acc.push('\\');
                }
                acc.push_str(seg);
                out.push(acc.clone());
            }
            out
        } else {
            let mut out = vec!["/".to_string()];
            let mut acc = String::new();
            for seg in current_dir.split('/').filter(|s| !s.is_empty()) {
                acc.push('/');
                acc.push_str(seg);
                out.push(acc.clone());
            }
            out
        }
    } else {
        let mut v: Vec<PathBuf> = Path::new(current_dir)
            .ancestors()
            .map(Path::to_path_buf)
            .collect();
        v.reverse();
        v.into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect()
    }
}

fn crumb_label(is_remote: bool, full_path: &str) -> String {
    if full_path == "/" {
        return "/".to_string();
    }
    if is_remote {
        if is_windows_style_remote_path(full_path) {
            if full_path.ends_with('\\') {
                return full_path.to_string(); // 드라이브 루트 — 그대로("C:\\").
            }
            return full_path
                .rsplit('\\')
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or(full_path)
                .to_string();
        }
        full_path
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(full_path)
            .to_string()
    } else {
        Path::new(full_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| full_path.to_string())
    }
}

#[cfg(test)]
mod path_helper_tests {
    use super::{crumb_label, join_dir, matches_filters, path_ancestors};

    #[test]
    fn matches_filters_empty_always_passes() {
        assert!(matches_filters(&[], "notes.txt"));
        assert!(matches_filters(&[], "README.md"));
    }

    #[test]
    fn matches_filters_matches_case_insensitive_extension() {
        let filters = vec!["md".to_string(), "markdown".to_string()];
        assert!(matches_filters(&filters, "notes.md"));
        assert!(matches_filters(&filters, "NOTES.MD"));
        assert!(matches_filters(&filters, "readme.markdown"));
    }

    #[test]
    fn matches_filters_rejects_non_matching_extension() {
        let filters = vec!["md".to_string()];
        assert!(!matches_filters(&filters, "notes.txt"));
        assert!(!matches_filters(&filters, "no_extension"));
    }

    #[test]
    fn join_dir_remote_posix() {
        assert_eq!(
            join_dir(true, "/home/alice", "notes.txt"),
            "/home/alice/notes.txt"
        );
        assert_eq!(join_dir(true, "/", "etc"), "/etc");
    }

    #[test]
    fn join_dir_remote_windows() {
        assert_eq!(
            join_dir(true, "C:\\Users\\alice", "notes.txt"),
            "C:\\Users\\alice\\notes.txt"
        );
        assert_eq!(join_dir(true, "C:\\", "Users"), "C:\\Users");
    }

    #[test]
    fn join_dir_local_uses_platform_path() {
        let joined = join_dir(false, "/tmp/dir", "file.txt");
        assert!(joined.ends_with("file.txt"));
    }

    #[test]
    fn path_ancestors_remote_posix() {
        assert_eq!(
            path_ancestors(true, "/home/alice/proj"),
            vec!["/", "/home", "/home/alice", "/home/alice/proj"]
        );
    }

    #[test]
    fn path_ancestors_remote_windows() {
        assert_eq!(
            path_ancestors(true, "C:\\Users\\alice"),
            vec!["C:\\", "C:\\Users", "C:\\Users\\alice"]
        );
    }

    #[test]
    fn crumb_label_remote_windows_root_and_segment() {
        assert_eq!(crumb_label(true, "C:\\"), "C:\\");
        assert_eq!(crumb_label(true, "C:\\Users\\alice"), "alice");
    }

    #[test]
    fn crumb_label_remote_posix_root_and_segment() {
        assert_eq!(crumb_label(true, "/"), "/");
        assert_eq!(crumb_label(true, "/home/alice"), "alice");
    }
}
