//! 탐색기 Properties 팝업. 시안 `YProps`(갤러리 "Properties")를 따른다.
//! 컨텍스트 메뉴의 Properties 나 `explorer_properties` 단축키로 열고, 그 탐색기 칸 범위에 뜬다.
//! 연 순간의 항목을 끝까지 보이며 이후 선택 변경을 따라가지 않는다. 폴더 크기는 read worker 가
//! 배경에서 세는 동안 Spinner 와 지금까지 센 값을 보이고, 팝업을 닫으면 세기를 멈춘다.
//! 원격 항목은 목록이 준 값(종류·크기·수정 시각·위치)만 보이고 그렇다고 알린다.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use tasty_ui_widgets::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Spinner,
};

use crate::adapters::ui::icons::{self, Icon};
use crate::adapters::ui::popup::PopupAction;
use crate::app::local_reads::{self, FolderCount, ItemFacts, ItemKind, PropertiesFacts, Query};
use crate::core::fs_list::{DirEntryInfo, group_digits, human_size};
use crate::i18n::{t, t_count, t_fmt};
use crate::runtime::engine_read::EngineRead;
use crate::state::MainViewState;
use crate::theme::{self, Theme};

pub const EXPLORER_PROPERTIES_POPUP_ID: &str = "explorer_properties";

/// 팝업이 연 항목과 읽은 정보.
pub struct ExplorerProperties {
    surface_id: u32,
    paths: Vec<PathBuf>,
    /// 원격이면 목록이 준 항목. 로컬이면 `None` 이고 read worker 가 읽는다.
    remote: Option<Vec<DirEntryInfo>>,
    query: Option<Query<PropertiesFacts>>,
    facts: Option<Result<PropertiesFacts, String>>,
    count: Arc<FolderCount>,
    cancel: Arc<AtomicBool>,
    /// 직전 프레임에 잰 내용 높이. sizer 가 팝업 높이로 쓴다.
    content_h: Option<f32>,
}

/// 팝업을 연다. `remote` 는 원격 탐색기의 목록 항목이다. 이미 열려 있으면 새 항목으로 바꾼다.
pub(crate) fn open(
    state: &mut MainViewState,
    surface_id: u32,
    paths: Vec<PathBuf>,
    remote: Option<Vec<DirEntryInfo>>,
    from_shortcut: bool,
) {
    if paths.is_empty() {
        return;
    }
    if let Some(old) = state.dialogs.explorer_properties.take() {
        old.cancel.store(true, Ordering::Relaxed);
    }
    let count = Arc::new(FolderCount::default());
    let cancel = Arc::new(AtomicBool::new(false));
    let query = remote
        .is_none()
        .then(|| local_reads::properties(paths.clone(), count.clone(), cancel.clone()));
    state.dialogs.explorer_properties = Some(ExplorerProperties {
        surface_id,
        paths,
        remote,
        query,
        facts: None,
        count,
        cancel,
        content_h: None,
    });
    let intent = crate::intent::UiIntent::OpenPopup {
        id: EXPLORER_PROPERTIES_POPUP_ID,
        mode: crate::intent::OpenPopupMode::WithScope(
            crate::model::popup_kind::PopupScope::Surface(surface_id),
        ),
    };
    state.dispatch_intent(if from_shortcut {
        intent.from_user_shortcut("explorer_properties")
    } else {
        intent.from_user_context_menu()
    });
}

/// 컨텍스트 메뉴의 Properties. 대상이 없으면(빈 영역) 현재 폴더를 연다. 원격이면 목록 항목을 넘긴다.
pub(crate) fn open_from_menu(
    state: &mut MainViewState,
    remote: bool,
    surface_id: u32,
    paths: &[PathBuf],
    cwd: &Path,
) {
    let paths = if paths.is_empty() {
        vec![cwd.to_path_buf()]
    } else {
        paths.to_vec()
    };
    let listed = remote.then(|| remote_entries(state, surface_id, &paths));
    open(state, surface_id, paths, listed, false);
}

/// `explorer_properties` 단축키. 선택이 있으면 목록 순서대로 그 항목들을, 없으면 현재 폴더를 연다.
pub(crate) fn open_for_shortcut(
    state: &mut MainViewState,
    remote: bool,
    surface_id: u32,
    cwd: &Path,
) {
    let mut paths: Vec<PathBuf> = state
        .explorer_views
        .get(surface_id)
        .map(|v| {
            v.entries
                .iter()
                .filter(|e| v.selected.contains(&e.path))
                .map(|e| e.path.clone())
                .collect()
        })
        .unwrap_or_default();
    if paths.is_empty() {
        paths.push(cwd.to_path_buf());
    }
    let listed = remote.then(|| remote_entries(state, surface_id, &paths));
    open(state, surface_id, paths, listed, true);
}

/// 원격 목록에서 경로들의 항목을 찾는다. 목록에 없는 경로(현재 폴더 자신)는 이름만 있는 폴더로 둔다.
fn remote_entries(state: &MainViewState, surface_id: u32, paths: &[PathBuf]) -> Vec<DirEntryInfo> {
    let listed = state
        .explorer_views
        .get(surface_id)
        .map(|v| v.entries.as_slice())
        .unwrap_or_default();
    paths
        .iter()
        .map(|p| {
            listed
                .iter()
                .find(|e| &e.path == p)
                .cloned()
                .unwrap_or_else(|| DirEntryInfo {
                    path: p.clone(),
                    name: file_name(p),
                    is_dir: true,
                    size: 0,
                    modified: None,
                    ext: String::new(),
                    link: Default::default(),
                })
        })
        .collect()
}

/// 정보 읽기 결과를 받는다. 받았으면 다시 그리도록 true 를 돌려준다.
pub(crate) fn poll(
    state: &mut MainViewState,
    owner: &mut crate::app::local_reads::ReadRequests,
) -> bool {
    let Some(props) = state.dialogs.explorer_properties.as_mut() else {
        return false;
    };
    let Some(result) = props.query.as_mut().and_then(|q| q.poll(owner)) else {
        return false;
    };
    props.query = None;
    props.facts = Some(result.map_err(|e| e.to_string()));
    true
}

/// 첫 프레임에서 내용 높이를 재기 전의 크기. 폭은 토큰, 높이는 필드 여덟 줄 몫이다.
pub fn default_size() -> egui::Vec2 {
    let th = theme::theme();
    egui::vec2(
        th.explorer_props_width().value(),
        th.explorer_props_row_min_height().value() * 8.0,
    )
}

pub fn sizer(state: &MainViewState, _engine: &EngineRead<'_>) -> egui::Vec2 {
    let th = theme::theme();
    let h = state
        .dialogs
        .explorer_properties
        .as_ref()
        .and_then(|p| p.content_h)
        .unwrap_or(default_size().y);
    egui::vec2(th.explorer_props_width().value(), h)
}

pub fn on_close(_ctx: &egui::Context, state: &mut MainViewState, _engine: &EngineRead<'_>) {
    if let Some(props) = state.dialogs.explorer_properties.take() {
        props.cancel.store(true, Ordering::Relaxed);
    }
}

/// 한 줄의 내용.
struct Field {
    label: &'static str,
    value: String,
    mono: bool,
    copy: bool,
    counting: bool,
}

fn field(label: &'static str, value: String) -> Field {
    Field {
        label,
        value,
        mono: false,
        copy: false,
        counting: false,
    }
}

fn mono(label: &'static str, value: String, copy: bool) -> Field {
    Field {
        label,
        value,
        mono: true,
        copy,
        counting: false,
    }
}

pub fn draw(ui: &mut egui::Ui, state: &mut MainViewState, _engine: &EngineRead<'_>) -> PopupAction {
    let th = theme::theme();
    let Some(props) = state.dialogs.explorer_properties.as_ref() else {
        return PopupAction::Close;
    };
    // 탐색기 칸이 닫히면 팝업도 닫는다.
    if state.explorer_views.get(props.surface_id).is_none() {
        return PopupAction::Close;
    }
    let (mut glyph, name, fields, note) = content(props);
    let failure = match &props.facts {
        Some(Err(reason)) => {
            glyph = item_glyph(state, props.surface_id, &props.paths[0]);
            Some(reason.clone())
        }
        _ => None,
    };
    let mut retry = false;
    let pad_x = th.explorer_props_padding_x().value();
    let mut close = false;
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    let top = ui.cursor().top();
    ui.vertical(|ui| {
        ui.set_width(th.explorer_props_width().value());
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: pad_x as i8,
                right: pad_x as i8,
                top: th.spacing_md.value() as i8,
                bottom: th.spacing_sm.value() as i8,
            })
            .show(ui, |ui| {
                close = header(ui, &th, glyph, &name);
            });
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: pad_x as i8,
                right: pad_x as i8,
                top: 0,
                bottom: th.spacing_md.value() as i8,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if let Some(reason) = &failure {
                    retry = unreadable_body(ui, &th, reason);
                }
                for f in &fields {
                    field_row(ui, &th, f);
                }
                if let Some(text) = note {
                    ui.add_space(th.spacing_sm.value());
                    ui.label(
                        egui::RichText::new(text)
                            .size(th.font_size_caption.value())
                            .color(th.text_muted().to_egui()),
                    );
                }
            });
    });
    let measured = ui.cursor().top() - top;
    if let Some(props) = state.dialogs.explorer_properties.as_mut() {
        props.content_h = Some(measured);
        if retry {
            props.retry();
        }
    }
    if close {
        PopupAction::Close
    } else {
        PopupAction::None
    }
}

impl ExplorerProperties {
    /// 읽기에 실패한 로컬 항목을 다시 읽는다. 원격 항목은 목록 값을 쓰므로 실패하지 않는다.
    fn retry(&mut self) {
        if self.remote.is_some() {
            return;
        }
        self.facts = None;
        self.query = Some(local_reads::properties(
            self.paths.clone(),
            self.count.clone(),
            self.cancel.clone(),
        ));
    }
}

/// 읽지 못한 항목의 제목 글리프. 탐색기 목록에 있으면 그 종류를, 없으면 지금 폴더인지로 정한다.
fn item_glyph(state: &MainViewState, surface_id: u32, path: &Path) -> Icon {
    let view = state.explorer_views.get(surface_id);
    let is_dir = view.and_then(|v| v.entries.iter().find(|e| e.path == path).map(|e| e.is_dir));
    let is_dir = is_dir.unwrap_or_else(|| view.and_then(|v| v.shown_dir()) == Some(path));
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();
    if is_dir {
        icons::FOLDER
    } else if crate::adapters::ui::surface::explorer::is_image_ext(&ext) {
        icons::IMAGE
    } else {
        icons::FILE
    }
}

/// 읽기 실패 본문: alertTriangle 과 "Can't read properties"(explorer-error-fg), OS 이유(mono muted), Retry.
/// Retry 를 누르면 true.
fn unreadable_body(ui: &mut egui::Ui, th: &Theme, reason: &str) -> bool {
    let error = th.explorer_error_fg().to_egui();
    let gap = th.spacing_xs.value();
    ui.add_space(gap);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        let g = th.icon_glyph_size_md.value();
        let (r, _) = ui.allocate_exact_size(egui::vec2(g, g), egui::Sense::hover());
        icons::ALERT_TRIANGLE.image(g, error).paint_at(ui, r);
        ui.label(
            egui::RichText::new(t("explorer.properties.unreadable"))
                .size(th.font_size_body.value())
                .color(error),
        );
    });
    ui.add_space(gap);
    ui.add(
        egui::Label::new(
            egui::RichText::new(reason)
                .monospace()
                .size(th.font_size_caption.value())
                .color(th.text_muted().to_egui()),
        )
        .wrap(),
    );
    ui.add_space(gap * 2.0);
    Button::new(t("explorer.properties.retry"))
        .variant(ButtonVariant::Secondary)
        .size(ControlSize::Sm)
        .show(ui, th)
        .clicked()
}

/// 머리: 글리프 · 이름(14, regular, 말줄임) · 닫기. 닫기를 누르면 true.
/// 제목은 regular 다. 테마에 semibold UI 글꼴이 없어 크기로 제목을 구분한다.
fn header(ui: &mut egui::Ui, th: &Theme, glyph: Icon, name: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        let g = th.icon_glyph_size_md.value();
        let (r, _) = ui.allocate_exact_size(egui::vec2(g, g), egui::Sense::hover());
        glyph.image(g, th.text_muted().to_egui()).paint_at(ui, r);
        let close_w = ControlSize::Sm.height(th);
        let name_w = (ui.available_width() - close_w - th.spacing_sm.value()).max(0.0);
        let (name_rect, _) = ui.allocate_exact_size(egui::vec2(name_w, g), egui::Sense::hover());
        let mut job = egui::text::LayoutJob::simple_singleline(
            name.to_owned(),
            egui::FontId::proportional(th.font_size_max.value()),
            th.text_primary().to_egui(),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(name_w);
        let galley = ui.fonts(|f| f.layout_job(job));
        let y = name_rect.center().y - galley.rect.height() / 2.0;
        ui.painter().galley(
            egui::pos2(name_rect.left(), y),
            galley,
            th.text_primary().to_egui(),
        );
        close = icon_button(ui, th, icons::CLOSE)
            .on_hover_text(t("explorer.properties.close"))
            .clicked();
    });
    close
}

fn icon_button(ui: &mut egui::Ui, th: &Theme, g: Icon) -> egui::Response {
    IconButton::new()
        .variant(IconButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .show(ui, th, &|ui, rect, c| {
            g.image(rect.height(), c).paint_at(ui, rect)
        })
}

/// 라벨(96, caption muted) · 값(body 또는 mono caption, 어디서나 줄바꿈) · 선택 Copy.
fn field_row(ui: &mut egui::Ui, th: &Theme, f: &Field) {
    let label_w = th.explorer_props_label_width().value();
    let line_h = th.explorer_props_row_line().value();
    let min_h = th.explorer_props_row_min_height().value();
    let top = th.explorer_props_row_pad_top().value();
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(width, min_h),
        egui::Layout::left_to_right(egui::Align::Min),
        |ui| {
            ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
            ui.set_min_height(min_h);
            ui.vertical(|ui| {
                ui.add_space(top);
                ui.set_width(label_w);
                ui.allocate_ui_with_layout(
                    egui::vec2(label_w, line_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(t(f.label))
                                    .size(th.font_size_caption.value())
                                    .color(th.text_muted().to_egui()),
                            )
                            .truncate(),
                        )
                    },
                );
            });
            ui.vertical(|ui| {
                ui.add_space(top);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = th.spacing_xs.value();
                    if f.counting {
                        let s = th.icon_glyph_size_sm.value();
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(s, line_h), egui::Sense::hover());
                        let mut slot =
                            ui.new_child(egui::UiBuilder::new().max_rect(
                                egui::Rect::from_center_size(r.center(), egui::vec2(s, s)),
                            ));
                        Spinner::new().size(s).show(&mut slot, th);
                    }
                    let copy_w = if f.copy {
                        ControlSize::Sm.height(th) + th.spacing_xs.value()
                    } else {
                        0.0
                    };
                    let font = if f.mono {
                        egui::FontId::monospace(th.font_size_caption.value())
                    } else {
                        egui::FontId::proportional(th.font_size_body.value())
                    };
                    let color = th.text_secondary().to_egui();
                    let text_w = (ui.available_width() - copy_w).max(0.0);
                    let mut job =
                        egui::text::LayoutJob::simple(f.value.clone(), font, color, text_w);
                    job.wrap.break_anywhere = true;
                    let galley = ui.fonts(|fonts| fonts.layout_job(job));
                    // 첫 줄을 줄 높이 가운데에 두고, 줄을 바꾼 값은 글자 줄 높이만큼만 늘린다.
                    let first_h = galley.rows.first().map_or(line_h, |r| r.height());
                    let text_h = (galley.rect.height() + line_h - first_h).max(line_h);
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(text_w, text_h), egui::Sense::hover());
                    let y = r.top() + (line_h - first_h) / 2.0;
                    ui.painter().galley(egui::pos2(r.left(), y), galley, color);
                    if f.copy
                        && icon_button(ui, th, icons::COPY)
                            .on_hover_text(t("explorer.properties.copy"))
                            .clicked()
                    {
                        ui.ctx().copy_text(f.value.clone());
                    }
                });
            });
        },
    );
}

/// (글리프, 제목, 필드, 안내 문구).
fn content(props: &ExplorerProperties) -> (Icon, String, Vec<Field>, Option<&'static str>) {
    let location = common_parent(&props.paths);
    if let Some(entries) = &props.remote {
        return remote_content(entries, location);
    }
    let counting = !props.count.done.load(Ordering::Acquire);
    let counted_items = props.count.items.load(Ordering::Relaxed);
    let counted_bytes = props.count.bytes.load(Ordering::Relaxed);
    let facts = match &props.facts {
        None => {
            let title = file_name(&props.paths[0]);
            let fields = vec![Field {
                counting: true,
                ..field("explorer.properties.kind", String::new())
            }];
            return (icons::FILE, title, fields, None);
        }
        // 글리프와 본문은 `draw` 가 탐색기 목록을 보고 채운다.
        Some(Err(_)) => {
            let title = file_name(&props.paths[0]);
            return (icons::FILE, title, Vec::new(), None);
        }
        Some(Ok(facts)) => facts,
    };
    if let [item] = facts.items.as_slice() {
        return single_content(
            &props.paths[0],
            item,
            location,
            counting,
            counted_items,
            counted_bytes,
        );
    }
    let folders = facts
        .items
        .iter()
        .filter(|f| matches!(f.kind, ItemKind::Folder))
        .count();
    let files = facts.items.len() - folders;
    let file_bytes: u64 = facts.items.iter().map(|f| f.size).sum();
    let total = human_size(false, file_bytes + counted_bytes);
    let fields = vec![
        field("explorer.properties.kinds", kinds_text(files, folders)),
        Field {
            counting: counting && folders > 0,
            ..field("explorer.properties.total_size", total)
        },
        mono("explorer.properties.location", location, true),
    ];
    let n = facts.items.len();
    let title = t_count("explorer.properties.items", n as u64, &[&n.to_string()]);
    (icons::LAYERS, title, fields, None)
}

fn single_content(
    path: &Path,
    item: &ItemFacts,
    location: String,
    counting: bool,
    counted_items: u64,
    counted_bytes: u64,
) -> (Icon, String, Vec<Field>, Option<&'static str>) {
    let name = file_name(path);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();
    let mut fields = Vec::new();
    let glyph = match &item.kind {
        ItemKind::Folder => {
            fields.push(field(
                "explorer.properties.kind",
                t("explorer.type.folder").to_owned(),
            ));
            let size = human_size(false, counted_bytes);
            let items = group_digits(counted_items);
            fields.push(if counting {
                Field {
                    counting: true,
                    ..field(
                        "explorer.properties.size",
                        t_count(
                            "explorer.properties.counting",
                            counted_items,
                            &[&items, &size],
                        ),
                    )
                }
            } else {
                field(
                    "explorer.properties.size",
                    t_count(
                        "explorer.properties.folder_size",
                        counted_items,
                        &[&size, &items],
                    ),
                )
            });
            fields.push(mono(
                "explorer.properties.modified",
                time_text(item.modified),
                false,
            ));
            fields.push(mono("explorer.properties.location", location, true));
            icons::FOLDER
        }
        ItemKind::Link(target) => {
            fields.push(field(
                "explorer.properties.kind",
                t("explorer.properties.symlink").to_owned(),
            ));
            fields.push(mono(
                "explorer.properties.link_target",
                target.display().to_string(),
                true,
            ));
            fields.push(mono("explorer.properties.location", location, true));
            icons::LINK
        }
        ItemKind::File => {
            fields.push(field(
                "explorer.properties.kind",
                crate::adapters::ui::surface::explorer::file_kind_word(&ext),
            ));
            fields.push(field(
                "explorer.properties.size",
                t_count(
                    "explorer.properties.size_bytes",
                    item.size,
                    &[&human_size(false, item.size), &group_digits(item.size)],
                ),
            ));
            fields.push(mono(
                "explorer.properties.modified",
                time_text(item.modified),
                false,
            ));
            fields.push(mono(
                "explorer.properties.created",
                time_text(item.created),
                false,
            ));
            fields.push(mono("explorer.properties.location", location, true));
            fields.push(mono(
                "explorer.properties.permissions",
                permissions_text(item),
                false,
            ));
            if crate::adapters::ui::surface::explorer::is_image_ext(&ext) {
                icons::IMAGE
            } else {
                icons::FILE
            }
        }
    };
    (glyph, name, fields, None)
}

fn remote_content(
    entries: &[DirEntryInfo],
    location: String,
) -> (Icon, String, Vec<Field>, Option<&'static str>) {
    let note = Some(t("explorer.properties.remote_note"));
    if let [e] = entries {
        let glyph = if e.is_dir { icons::FOLDER } else { icons::FILE };
        let fields = vec![
            field(
                "explorer.properties.kind",
                crate::adapters::ui::surface::explorer::kind_word(e),
            ),
            field("explorer.properties.size", human_size(e.is_dir, e.size)),
            mono("explorer.properties.modified", time_text(e.modified), false),
            mono("explorer.properties.location", location, true),
        ];
        return (glyph, e.name.clone(), fields, note);
    }
    let folders = entries.iter().filter(|e| e.is_dir).count();
    let files = entries.len() - folders;
    let bytes: u64 = entries.iter().filter(|e| !e.is_dir).map(|e| e.size).sum();
    let fields = vec![
        field("explorer.properties.kinds", kinds_text(files, folders)),
        field("explorer.properties.total_size", human_size(false, bytes)),
        mono("explorer.properties.location", location, true),
    ];
    let n = entries.len();
    let title = t_count("explorer.properties.items", n as u64, &[&n.to_string()]);
    (icons::LAYERS, title, fields, note)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// 항목들의 공통 상위 폴더.
fn common_parent(paths: &[PathBuf]) -> String {
    let mut parent = paths
        .first()
        .and_then(|p| p.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    for p in paths.iter().skip(1) {
        while !p.starts_with(&parent) {
            if !parent.pop() {
                break;
            }
        }
    }
    parent.display().to_string()
}

/// "2 files, 1 folder" 처럼 파일·폴더 수. 미리보기 패널의 여러 개 선택 머리도 쓴다.
pub(crate) fn kinds_text(files: usize, folders: usize) -> String {
    let count = |n: usize, key: &str| t_count(key, n as u64, &[&n.to_string()]);
    let mut parts = Vec::new();
    if files > 0 {
        parts.push(count(files, "explorer.properties.files"));
    }
    if folders > 0 {
        parts.push(count(folders, "explorer.properties.folders"));
    }
    parts.join(t("explorer.properties.list_separator"))
}

fn permissions_text(item: &ItemFacts) -> String {
    let flag = if item.read_only {
        t("explorer.properties.yes")
    } else {
        t("explorer.properties.no")
    };
    let read_only = t_fmt("explorer.properties.read_only", flag);
    match &item.mode {
        Some(mode) => format!("{mode} · {read_only}"),
        None => read_only,
    }
}

/// 로컬 시각 `YYYY-MM-DD HH:MM`. 값이 없으면 대시다.
fn time_text(t: Option<SystemTime>) -> String {
    match t {
        Some(t) => chrono::DateTime::<chrono::Local>::from(t)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        None => "—".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_reads_a_failed_local_item_again_and_leaves_remote_items_alone() {
        let failed = |remote: Option<Vec<DirEntryInfo>>| ExplorerProperties {
            surface_id: 1,
            paths: vec![PathBuf::from("/srv/private.key")],
            remote,
            query: None,
            facts: Some(Err("Permission denied (os error 13)".into())),
            count: Arc::new(FolderCount::default()),
            cancel: Arc::new(AtomicBool::new(false)),
            content_h: None,
        };
        let mut local = failed(None);
        local.retry();
        assert!(
            local.facts.is_none(),
            "the old error is cleared while it reads again"
        );
        assert!(local.query.is_some());

        let mut remote = failed(Some(Vec::new()));
        remote.retry();
        assert!(remote.facts.is_some());
        assert!(remote.query.is_none());
    }

    #[test]
    fn the_unreadable_body_shows_the_reason_and_a_retry_button() {
        crate::i18n::init("en");
        let th = crate::theme::theme();
        let ctx = egui::Context::default();
        let mut texts = Vec::new();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert!(!unreadable_body(ui, &th, "Permission denied (os error 13)"));
            });
        });
        for shape in output.shapes {
            if let egui::Shape::Text(text) = shape.shape {
                texts.push(text.galley.text().to_owned());
            }
        }
        for want in [
            "Can't read properties",
            "Permission denied (os error 13)",
            "Retry",
        ] {
            assert!(texts.iter().any(|t| t == want), "{want} in {texts:?}");
        }
    }

    #[test]
    fn several_items_share_their_nearest_parent() {
        let paths = [PathBuf::from("/a/b/c.txt"), PathBuf::from("/a/b/d/e.txt")];
        assert_eq!(common_parent(&paths), "/a/b");
        let paths = [PathBuf::from("/a/x"), PathBuf::from("/b/y")];
        assert_eq!(common_parent(&paths), "/");
    }
}
