//! Tasty 자체 파일 열기 팝업. 로컬과 원격 디렉터리 조회를 같은 UI로 제공한다.
//! 동작은 `docs/features/native-file-picker/index.md`, 원격 조회 권한은 ADR-0022를 따른다.
//! OS의 로컬 파일 선택 대화상자는 원격 경로를 탐색할 수 없어 이 화면을 따로 둔다.
//!
//! 표시 함수는 FilePickerProps를 받아 동작을 반환하며 갤러리에서 따로 그릴 수 있다.
//! 로컬 목록은 호출부에서 동기 read_dir_entries로 읽는다.
//! 원격 목록은 pending_list_dir_forward에 넣고 attach 응답으로 갱신한다.
//! 응답 제한은 매 프레임 sent_at의 경과 시간으로 확인하며 로컬 읽기에는 적용하지 않는다.

mod footer;
#[cfg(test)]
mod layout_tests;
mod path_bar;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tasty_type_geometry::length::LogicalPx;

use crate::adapters::ui::icons;

/// breadcrumb 구분자 크기. 대응 토큰이 없어 갤러리와 같은 별도 값을 사용한다.
pub(super) const CRUMB_GLYPH: LogicalPx = LogicalPx(13.0);
use crate::adapters::ui::popup::PopupAction;
use crate::i18n::t;
use crate::state::{AppState, FilePickerResult, FpLoadState};
use crate::theme::{self, Theme};
use tasty_ui_widgets::{Button, ButtonVariant, IconButton, IconButtonVariant, Spinner, hspace};

pub const FILE_PICKER_POPUP_ID: &str = "file_picker";

pub(crate) const POPUP_WIDTH: LogicalPx = LogicalPx(640.0);
pub(crate) const POPUP_HEIGHT: LogicalPx = LogicalPx(480.0);

// FilePickerFrame/FpRow 열 치수 — gallery specimen 과 같은 구조 폭.
const ROW_H: LogicalPx = LogicalPx(28.0);
const SIZE_COL_W: LogicalPx = LogicalPx(68.0);
const MOD_COL_W: LogicalPx = LogicalPx(108.0);

// 원격 연결 화면과 같은 공용 중앙 안내 영역 치수.
use tasty_ui_widgets::tokens::{CENTER_BLOCK_H_POPUP as CENTER_BLOCK_H, CENTER_GLYPH_SIZE};
/// 원격 응답이 이 시간 안에 오지 않으면 `ErrorConn` 으로 전이(soft timeout — 세션의
/// `disconnected` 플래그만으론 "서버는 살아있는데 응답이 안 오는" 케이스를 못 잡는다).
const LIST_DIR_SOFT_TIMEOUT: Duration = Duration::from_secs(8);

/// PopupDef.sizer — 고정 640×480(gallery specimen `FRAME_W`/`FRAME_H`).
pub fn picker_sizer(_state: &AppState, _engine: &crate::core::CoreState) -> egui::Vec2 {
    egui::vec2(POPUP_WIDTH.value(), POPUP_HEIGHT.value())
}

/// 갤러리에서도 사용할 수 있도록 표시 문자열로 준비한 목록 행.
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

/// 피커가 무엇을 하는가 — footer 이름 칸의 성질이 여기서 갈린다.
#[derive(Clone, Copy)]
pub enum FilePickerMode<'a> {
    /// 기존 파일을 고른다. 이름 칸은 읽기 전용으로 현재 선택을 보여준다.
    Open { selection_text: &'a str },
    /// 경로를 만든다. 이름 칸이 **유일한 확정 대상**이고 편집 가능하다.
    Save {
        name: &'a str,
        /// 입력한 이름이 지금 나열된 폴더에 이미 있다 — 경고 줄을 띄운다.
        overwrite: bool,
        /// 확정 버튼 활성 여부(빈 이름·경로 같은 이름이면 `false`). 판정은 wrapper 가 한다.
        can_confirm: bool,
    },
}

/// 순수 시각 view 의 입력. AppState/CoreState 의존 없음.
pub struct FilePickerProps<'a> {
    pub theme: &'a Theme,
    /// `Some(host)` 면 헤더에 host 배지 렌더(원격 브라우징).
    pub remote_host: Option<&'a str>,
    /// root 부터 현재 폴더까지 **전체** 경로. 가운데 생략은 렌더 규칙이라 여기서 줄이지 않는다
    /// — `…` 메뉴가 숨긴 조상을 열어야 한다.
    pub crumbs: &'a [CrumbView],
    pub state: FpViewState,
    pub entries: &'a [FilePickerEntryView],
    /// 선택된 엔트리 이름(현재 디렉토리 기준).
    pub selected: &'a [String],
    pub mode: FilePickerMode<'a>,
    /// 이 프레임에 Esc 를 소비할 자격이 있는가(ADR-0036).
    /// `false` 면 위에 다른 popup 이 있다는 뜻이라 Esc 를 무시한다 — 한 번의 Esc 로
    /// 스택 전체가 닫히는 것을 막는다. 판정은 `AppState.popup_escape_owner`.
    pub owns_escape: bool,

    // i18n — 호출처가 t() 로 미리 해상해서 전달(file_handler_picker.rs 관례).
    pub title_label: &'a str,
    pub name_field_label: &'a str,
    /// 이름 칸이 비었을 때의 placeholder(열기: 선택 없음 / 저장: 이름 입력 안내).
    pub name_placeholder: &'a str,
    pub cancel_label: &'a str,
    /// footer primary 버튼 라벨(Open / Save / Overwrite — 모드와 덮어쓰기 여부로 호출처가 고른다).
    pub confirm_label: &'a str,
    /// 덮어쓰기 경고 줄. `{name}` 자리에 이름이 mono 로 들어간다.
    pub overwrite_warning: &'a str,
    /// 저장 모드에서 선택한 폴더가 저장 대상은 아니라는 안내. {name}은 고정폭 글꼴로 표시한다.
    pub folder_not_save_target: &'a str,
    /// 열기 모드에서 폴더 행을 고른 상태의 안내 줄. `{name}` 은 폴더 이름(mono),
    /// `{confirm}` 은 확정 버튼 이름이다 — 그 버튼이 곧 키보드로 들어가는 길이다.
    pub folder_open_enters: &'a str,
    /// 숨긴 조상이 하나일 때의 툴팁. 단수형 문구를 별도로 받는다.
    pub hidden_folders_one: &'a str,
    /// `…` 툴팁 — 숨긴 조상이 둘 이상일 때. `{}` 가 수로 치환된다.
    pub hidden_folders_many: &'a str,
    pub empty_label: &'a str,
    pub loading_label: &'a str,
    pub loading_body_local: &'a str,
    pub loading_body_remote: &'a str,
    pub error_perm_title: &'a str,
    pub error_perm_retry: &'a str,
    pub error_conn_title: &'a str,
    pub error_conn_reconnect: &'a str,
}

/// 선택한 폴더 하나의 이름. 푸터 안내와 확정 버튼이 같은 판정을 쓴다.
pub(super) fn selected_folder<'a>(props: &'a FilePickerProps<'a>) -> Option<&'a str> {
    let [name] = props.selected else {
        return None;
    };
    props
        .entries
        .iter()
        .find(|e| &e.name == name)
        .filter(|e| e.is_dir)
        .map(|e| e.name.as_str())
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
    /// 브레드크럼 세그먼트(또는 `…` 메뉴의 숨긴 조상) 클릭 — 그 인덱스까지의 경로로 내비게이트.
    NavigateTo(usize),
    /// 상위 폴더로.
    NavigateUp,
    /// path bar 의 refresh 버튼 또는 에러 상태의 Retry/Reconnect 버튼.
    Refresh,
    /// footer primary 버튼 — 열기는 현재 `selected`, 저장은 이름 칸으로 확정.
    Confirm,
    /// 파일 행 더블클릭 — 선택 상태와 무관하게 그 엔트리 하나로 즉시 확정.
    ConfirmEntry(String),
    /// 저장 모드 이름 칸이 편집됐다 — 새 전체 값.
    EditName(String),
}

/// 헤더·경로·푸터 영역을 먼저 정하고 목록에는 남은 높이를 준다.
/// 긴 경로는 breadcrumb 안에서 줄여 푸터 버튼 영역을 보존한다.
pub fn draw_file_picker_view(ui: &mut egui::Ui, props: &FilePickerProps<'_>) -> FilePickerAction {
    let ctx = ui.ctx().clone();
    if props.owns_escape && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return FilePickerAction::Cancel;
    }

    let th = props.theme;
    let mut action = FilePickerAction::None;

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

    path_bar::path_bar(ui, props, &mut action);
    ui.add_space(th.spacing_xs.value());
    hline(ui, th);

    let rest = ui.available_rect_before_wrap();
    let footer_h =
        footer::footer_height(ui, props).min(LogicalPx(rest.height()).max(LogicalPx(0.0)));
    let footer_rect = egui::Rect::from_min_max(
        egui::pos2(rest.left(), rest.bottom() - footer_h.value()),
        rest.max,
    );
    let body_rect = egui::Rect::from_min_max(rest.min, egui::pos2(rest.right(), footer_rect.top()));

    let mut body_ui = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("file_picker_body")
            .max_rect(body_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    body_ui.set_clip_rect(body_rect.intersect(ui.clip_rect()));
    draw_body(
        &mut body_ui,
        props,
        LogicalPx(body_rect.height()),
        &mut action,
    );

    let mut footer_ui = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("file_picker_footer")
            .max_rect(footer_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    footer_ui.set_clip_rect(footer_rect.intersect(ui.clip_rect()));
    footer::draw_footer(&mut footer_ui, props, &mut action);

    ui.advance_cursor_after_rect(rest);
    action
}

fn draw_body(
    ui: &mut egui::Ui,
    props: &FilePickerProps<'_>,
    body_height: LogicalPx,
    action: &mut FilePickerAction,
) {
    let th = props.theme;
    match &props.state {
        FpViewState::Loaded => {
            egui::ScrollArea::vertical()
                .id_salt("file_picker_list")
                .max_height(body_height.value())
                .auto_shrink([false, true])
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    for entry in props.entries {
                        if let Some(a) = entry_row(ui, props, entry)
                            && (matches!(action, FilePickerAction::None)
                                || !matches!(a, FilePickerAction::Select(_)))
                        {
                            *action = a;
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
                *action = FilePickerAction::Refresh;
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
                *action = FilePickerAction::Refresh;
            }
        }
    }
}

/// 목록 한 행. 클릭 의도가 있으면 돌려준다.
fn entry_row(
    ui: &mut egui::Ui,
    props: &FilePickerProps<'_>,
    entry: &FilePickerEntryView,
) -> Option<FilePickerAction> {
    let th = props.theme;
    let selected = props.selected.iter().any(|s| s == &entry.name);
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_H.value()),
        egui::Sense::click(),
    );
    if selected {
        ui.painter()
            .rect_filled(rect, 0.0, th.surface_active().to_egui());
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, th.hover_overlay.to_egui_premultiplied());
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
        egui::pos2(
            rect.left() + th.spacing_md.value(),
            rect.center().y - glyph_size * 0.5,
        ),
        egui::vec2(glyph_size, glyph_size),
    );
    glyph
        .image(glyph_size, glyph_color.into())
        .paint_at(ui, icon_rect);
    // 오른쪽의 고정 열부터 배치하고 이름만 남은 폭에서 말줄임한다.
    let modified_right = LogicalPx(rect.right()) - th.spacing_md;
    let size_right = modified_right - MOD_COL_W - th.spacing_sm;
    let name_right = size_right - SIZE_COL_W - th.spacing_sm;
    let name_left = LogicalPx(icon_rect.right()) + th.spacing_sm;
    let name_color = if selected {
        th.text_primary().to_egui()
    } else {
        th.text_secondary().to_egui()
    };
    let mut job = egui::text::LayoutJob::simple_singleline(
        entry.name.clone(),
        egui::FontId::proportional(th.font_size_body.value()),
        name_color,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(
        (name_right - name_left).max(LogicalPx(0.0)).value(),
    );
    let galley = ui.fonts(|f| f.layout_job(job));
    ui.painter().galley(
        egui::pos2(name_left.value(), rect.center().y - galley.size().y * 0.5),
        galley,
        name_color,
    );
    let mono = egui::FontId::monospace(th.font_size_caption.value());
    ui.painter().text(
        egui::pos2(size_right.value(), rect.center().y),
        egui::Align2::RIGHT_CENTER,
        &entry.size_display,
        mono.clone(),
        th.text_muted().into(),
    );
    ui.painter().text(
        egui::pos2(modified_right.value(), rect.center().y),
        egui::Align2::RIGHT_CENTER,
        &entry.modified_display,
        mono,
        th.text_muted().into(),
    );
    if resp.double_clicked() {
        Some(if entry.is_dir {
            FilePickerAction::NavigateInto(entry.name.clone())
        } else {
            FilePickerAction::ConfirmEntry(entry.name.clone())
        })
    } else if resp.clicked() {
        Some(FilePickerAction::Select(entry.name.clone()))
    } else {
        None
    }
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

/// 원격 호스트를 user@host 형태로 표시하는 배지.
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
    // tinted 채움/테두리 짝 — `tint-fill-alpha` / `tint-border-alpha`.
    ui.painter().rect_filled(
        rect,
        radius,
        info_color.gamma_multiply(th.tint_fill_alpha()),
    );
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(
            th.border_width.value(),
            info_color.gamma_multiply(th.tint_border_alpha()),
        ),
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

/// 결과 없이 닫혔으면 Cancelled를 기록해 호출부가 계속 기다리지 않게 한다.
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

/// 상태에서 화면 입력을 만들고 사용자 동작을 반영한다.
pub fn draw_file_picker(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    let Some(data) = state.dialogs.file_picker.as_ref() else {
        return PopupAction::Close;
    };

    // 원격 mirror 워크스페이스가 사라지면 연결 오류로 표시한다.
    // 세션의 disconnected 플래그는 이 호출부에서 직접 읽을 수 없다.
    if let Some(mirror_ws_id) = data.mirror_ws_id
        && engine.find_workspace_index_for_id(mirror_ws_id).is_none()
        && !matches!(data.load, FpLoadState::ErrorConn(_))
    {
        let data = state.dialogs.file_picker.as_mut().unwrap();
        data.load = FpLoadState::ErrorConn(t("filepicker.error_conn.session_lost").to_string());
    }

    if let Some(FpLoadState::Loading { sent_at, .. }) =
        state.dialogs.file_picker.as_ref().map(|d| d.load.clone())
        && sent_at.elapsed() > LIST_DIR_SOFT_TIMEOUT
    {
        let data = state.dialogs.file_picker.as_mut().unwrap();
        data.load = FpLoadState::ErrorConn(t("filepicker.error_conn.timeout").to_string());
    }

    let th = theme::theme();
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

    // 폴더를 고르면 이름 칸은 비우고 확정 버튼은 해당 폴더로 이동한다.
    let selection_text = data
        .selected
        .iter()
        .filter(|name| {
            data.entries
                .iter()
                .find(|e| &&e.name == name)
                .is_some_and(|e| !e.is_dir)
        })
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let title_label = t("filepicker.title");
    let name_field_label = t("filepicker.name_field_label");
    let cancel_label = t("button.cancel");
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
        mode: FilePickerMode::Open {
            selection_text: &selection_text,
        },
        owns_escape,
        title_label,
        name_field_label,
        name_placeholder: t("filepicker.no_file_selected"),
        cancel_label,
        confirm_label: t("filepicker.open_button"),
        overwrite_warning: t("filepicker.save.overwrite_warning"),
        folder_not_save_target: t("filepicker.folder_not_save_target"),
        folder_open_enters: t("filepicker.folder_open_enters"),
        hidden_folders_one: t("filepicker.hidden_folders_one"),
        hidden_folders_many: t("filepicker.hidden_folders_many"),
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
        // 메인 피커는 열기 전용이라 이름 칸이 편집되지 않는다.
        FilePickerAction::EditName(_) => PopupAction::None,
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
            let folder = state.dialogs.file_picker.as_ref().and_then(|d| {
                let [name] = d.selected.as_slice() else {
                    return None;
                };
                d.entries
                    .iter()
                    .find(|e| &e.name == name)
                    .filter(|e| e.is_dir)
                    .map(|e| e.name.clone())
            });
            if let Some(name) = folder {
                navigate(state, engine, |dir, is_remote| {
                    join_dir(is_remote, dir, &name)
                });
                return PopupAction::None;
            }
            if let Some(d) = state.dialogs.file_picker.as_mut() {
                let is_remote = d.mirror_ws_id.is_some();
                // 화면의 활성화 검사와 별개로 디렉터리를 파일로 확정하지 않게 재확인한다.
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

/// 출발 surface의 디렉터리. 새 surface의 상속을 정하는 inherit_cwd와는 무관하다.
#[derive(Debug, Clone, Default)]
pub struct FilePickerStart {
    /// 시작 디렉토리. 로컬 출발이면 로컬 절대경로, 원격(mirror) 출발이면 원격 경로 문자열.
    pub dir: Option<String>,
    /// 활성 워크스페이스가 아니라 이 surface의 소속으로 로컬·원격을 구분한다.
    pub origin_surface_id: Option<u32>,
}

impl FilePickerStart {
    /// surface 의 cwd 에서 출발한다. mirror surface 면 원격 cwd 문자열을 그대로 싣는다.
    pub fn from_surface(engine: &crate::core::CoreState, surface_id: Option<u32>) -> Self {
        use crate::core::state::SurfaceCwd;
        let dir = surface_id
            .and_then(|sid| engine.surface_cwd(sid))
            .map(|cwd| match cwd {
                SurfaceCwd::Local(p) => p.to_string_lossy().into_owned(),
                SurfaceCwd::Remote(r) => r.as_str().to_string(),
            });
        Self {
            dir,
            origin_surface_id: surface_id,
        }
    }
}

/// 원격은 로컬에서 검사하지 않고 주어진 경로를 사용한다. 없으면 서버가 홈으로 해석할 빈 문자열이다.
/// 로컬은 실제 절대 디렉터리만 채택하며 아니면 홈을 사용한다.
fn initial_dir(is_remote: bool, requested: Option<String>) -> String {
    if is_remote {
        return requested.unwrap_or_default();
    }
    requested
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_dir())
        .unwrap_or_else(|| {
            directories::BaseDirs::new()
                .map(|d| d.home_dir().to_path_buf())
                .unwrap_or_else(|| PathBuf::from("/"))
        })
        .to_string_lossy()
        .to_string()
}

/// 출발 surface의 소속(없으면 활성 워크스페이스)으로 로컬·원격을 구분해 피커를 연다.
/// 로컬은 동기로 읽고 원격은 navigate에서 요청을 큐에 넣는다.
/// start는 출발 디렉터리와 surface이며 플러그인 입력 칸의 값은 포함하지 않는다.
/// requester가 있으면 결과를 해당 플러그인에만 보낸다. filters가 비면 확장자를 제한하지 않는다.
pub fn open(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    requester: Option<crate::state::FilePickerRequester>,
    filters: Vec<String>,
    start: FilePickerStart,
) {
    let origin_ws = start
        .origin_surface_id
        .and_then(|sid| engine.find_workspace_index_for_surface(sid))
        .map(|(idx, _)| idx);
    let mirror_ws_id = engine
        .workspaces
        .get(origin_ws.unwrap_or(state.active_workspace))
        .filter(|ws| ws.mirror)
        .map(|ws| ws.id);

    let initial_dir = initial_dir(mirror_ws_id.is_some(), start.dir);

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
pub(crate) fn matches_filters(filters: &[String], name: &str) -> bool {
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

/// 원격 OS를 알 수 없어 경로에 역슬래시가 있으면 Windows 형식으로 추정한다.
/// 역슬래시를 이름에 포함한 POSIX 경로는 이 방식으로 구별하지 못한다.
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
pub(crate) fn path_ancestors(is_remote: bool, current_dir: &str) -> Vec<String> {
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

pub(crate) fn crumb_label(is_remote: bool, full_path: &str) -> String {
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
