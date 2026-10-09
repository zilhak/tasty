//! 탐색기 Properties 팝업. 시안 `YProps`(갤러리 "Properties")를 따른다.
//! 컨텍스트 메뉴의 Properties 나 `explorer_properties` 단축키로 열고, 그 탐색기 칸 범위에 뜬다.
//! 연 순간의 항목을 끝까지 보이며 이후 선택 변경을 따라가지 않는다. 폴더 크기는 read worker 가
//! 배경에서 세는 동안 Spinner 와 지금까지 센 값을 보이고, 팝업을 닫으면 세기를 멈춘다.
//! 원격 항목은 목록이 준 값(종류·크기·수정 시각·위치)만 보이고 그렇다고 알린다.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{ControlSize, IconButton, IconButtonVariant, Spinner};

use crate::adapters::ui::icons::{self, Icon};
use crate::adapters::ui::popup::PopupAction;
use crate::app::local_reads::{self, FolderCount, ItemFacts, ItemKind, PropertiesFacts, Query};
use crate::core::fs_list::{DirEntryInfo, group_digits, human_size};
use crate::i18n::{t, t_fmt, t_fmt2};
use crate::runtime::engine_read::EngineRead;
use crate::state::MainViewState;
use crate::theme::{self, Theme};

pub const EXPLORER_PROPERTIES_POPUP_ID: &str = "explorer_properties";

/// 시안 `YProps` 좌우 여백 `--tasty-size-14`. 대응 컴포넌트 토큰이 없다.
const PAD_X: LogicalPx = LogicalPx(14.0);
/// 시안 `YField` 줄 높이 `--tasty-size-20`. 대응 컴포넌트 토큰이 없다.
const FIELD_LINE_H: LogicalPx = LogicalPx(20.0);
/// 시안 `YField` 최소 높이 `--tasty-size-24`. 대응 컴포넌트 토큰이 없다.
const FIELD_MIN_H: LogicalPx = LogicalPx(24.0);
/// 시안 `YField` 위 여백 `paddingTop: 2`. 대응 컴포넌트 토큰이 없다.
const FIELD_TOP: LogicalPx = LogicalPx(2.0);

/// 위 상수는 Theme 값과 달리 배율을 타지 않았으므로 같은 식에 쓰기 전에 UI 배율을 곱한다.
fn zoomed(th: &Theme, px: LogicalPx) -> f32 {
    (px.value() * th.ui_zoom).round()
}

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
        zoomed(&th, FIELD_MIN_H) * 8.0,
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
    let (glyph, name, fields, note) = content(props);
    let pad_x = zoomed(&th, PAD_X);
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
    }
    if close {
        PopupAction::Close
    } else {
        PopupAction::None
    }
}

/// 머리: 글리프 · 이름(14, 말줄임) · 닫기. 닫기를 누르면 true.
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
    let line_h = zoomed(th, FIELD_LINE_H);
    let min_h = zoomed(th, FIELD_MIN_H);
    let top = zoomed(th, FIELD_TOP);
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
        Some(Err(reason)) => {
            let title = file_name(&props.paths[0]);
            let fields = vec![mono("explorer.properties.error", reason.clone(), false)];
            return (icons::ALERT_TRIANGLE, title, fields, None);
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
    let title = t_fmt("explorer.properties.items", &facts.items.len().to_string());
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
                        t_fmt2("explorer.properties.counting", &items, &size),
                    )
                }
            } else if counted_items == 1 {
                field(
                    "explorer.properties.size",
                    t_fmt("explorer.properties.folder_size_one", &size),
                )
            } else {
                field(
                    "explorer.properties.size",
                    t_fmt2("explorer.properties.folder_size", &size, &items),
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
            icons::FILE
        }
        ItemKind::File => {
            let kind = if ext.is_empty() {
                t("explorer.type.file").to_owned()
            } else {
                ext.to_uppercase()
            };
            fields.push(field("explorer.properties.kind", kind));
            fields.push(field(
                "explorer.properties.size",
                t_fmt2(
                    "explorer.properties.size_bytes",
                    &human_size(false, item.size),
                    &group_digits(item.size),
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
                crate::adapters::ui::surface::explorer::type_label(e),
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
    let title = t_fmt("explorer.properties.items", &entries.len().to_string());
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
    let count = |n: usize, one: &str, many: &str| {
        if n == 1 {
            t(one).to_owned()
        } else {
            t_fmt(many, &n.to_string())
        }
    };
    let mut parts = Vec::new();
    if files > 0 {
        parts.push(count(
            files,
            "explorer.properties.files_one",
            "explorer.properties.files",
        ));
    }
    if folders > 0 {
        parts.push(count(
            folders,
            "explorer.properties.folders_one",
            "explorer.properties.folders",
        ));
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
    fn several_items_share_their_nearest_parent() {
        let paths = [PathBuf::from("/a/b/c.txt"), PathBuf::from("/a/b/d/e.txt")];
        assert_eq!(common_parent(&paths), "/a/b");
        let paths = [PathBuf::from("/a/x"), PathBuf::from("/b/y")];
        assert_eq!(common_parent(&paths), "/");
    }
}
