//! 목록 오른쪽 미리보기 패널. 시안 `YPreview`(갤러리 "Preview panel")를 따른다.
//! 폭은 `explorer_preview_width`(288)에서 시작해 경계선을 끌어 `explorer_preview_min_width`…`max_width`
//! 사이로 바꾸며 이 탐색기 동안 기억한다. 칸이 좁아 목록 최소 폭을 남기지 못하면 패널만 숨고 토글은 켜진 채다.
//! 선택이 하나일 때 그 항목을 보인다. 선택이 없으면 "Select a file", 여러 개면 개수와 하나를 고르라는 안내다. 새 선택은 이전 미리보기를 바로 지우고 읽기가 끝날 때까지 Loading 을 보인다.
//! 읽기는 read worker 가 맡는다(`local_reads::preview`). 원격 탐색기는 파일 내용을 받을 경로가 없어
//! 지원하지 않는 형식 상태를 보인다.

use std::path::PathBuf;
use std::time::SystemTime;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use super::state_screen;
use super::view::{DirEntryInfo, ExplorerView, human_size};
use crate::adapters::ui::icons;
use crate::app::local_reads::{
    self, MAX_DECODE_ALLOC, MAX_IMAGE_SIDE, PREVIEW_MAX_BYTES, PreviewData, Query, ReadRequests,
    TooLarge,
};
use crate::i18n::{t, t_fmt, t_fmt2};
use crate::model::ExplorerPreview;

/// 패널 본문 상태.
enum Body {
    /// 선택이 없거나 둘 이상이다.
    NoTarget,
    Loading,
    Text(String),
    Image {
        image: Option<egui::ColorImage>,
        texture: Option<egui::TextureHandle>,
        size: [usize; 2],
    },
    Unsupported,
    TooLarge(TooLarge),
    Error(String),
}

/// 탐색기 하나의 미리보기 패널 상태.
pub struct PreviewPane {
    /// 토글. 칸이 좁아 패널이 숨어도 켜진 채다.
    pub open: bool,
    /// 사용자가 끌어 정한 폭을 UI 배율로 나눈 값. `None` 이면 토큰 기본값을 쓴다.
    width: Option<f32>,
    /// 지금 보이는 항목, 그 수정 시각, 그림을 맞춘 최대 표시 폭(물리 px). 파일이 바뀌거나 배율이 바뀌어
    /// 최대 표시 폭이 달라지면 다시 읽는다.
    shown: Option<(PathBuf, Option<SystemTime>, u32)>,
    query: Option<Query<PreviewData>>,
    body: Body,
    /// view 와 model 이 마지막으로 같았던 값. 받아 오거나 model 에 남기면 갱신한다.
    /// model 이 이 값과 달라지면 다른 경로가 바꾼 것이므로 다시 받는다. 같은 surface id 로 kind 를
    /// 바꿨다 돌아와 새 `ExplorerPanel` 이 기본값으로 생긴 경우도 여기서 잡는다(view 는 id 로 남는다).
    known: Option<ExplorerPreview>,
    /// 사용자가 토글·폭을 바꿔 model 에 아직 남기지 않았다.
    changed: bool,
}

impl Default for PreviewPane {
    fn default() -> Self {
        Self {
            open: false,
            width: None,
            shown: None,
            query: None,
            body: Body::NoTarget,
            known: None,
            changed: false,
        }
    }
}

impl PreviewPane {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.changed = true;
        if !self.open {
            self.shown = None;
            self.query = None;
            self.body = Body::NoTarget;
        }
    }

    /// model 값이 마지막으로 맞춘 값과 다를 때만 받는다. 처음 그릴 때는 레이아웃에서 복원한 값을 받는다.
    /// 끌기 중처럼 view 만 바뀐 동안에는 model 이 그대로라 덮어쓰지 않는다.
    pub(super) fn adopt(&mut self, model: &ExplorerPreview) {
        if self.known == Some(*model) {
            return;
        }
        self.known = Some(*model);
        self.changed = false;
        if self.open && !model.open {
            self.shown = None;
            self.query = None;
            self.body = Body::NoTarget;
        }
        self.open = model.open;
        self.width = model.width.map(LogicalPx::value);
    }

    /// 사용자가 바꾼 토글·폭이 있으면 model 에 남길 값을 꺼낸다.
    pub fn take_change(&mut self) -> Option<ExplorerPreview> {
        if !std::mem::take(&mut self.changed) {
            return None;
        }
        let preview = ExplorerPreview {
            open: self.open,
            width: self.width.map(LogicalPx),
        };
        // model 이 곧 이 값이 된다. 다음 프레임에 자기 변경을 다시 받지 않게 한다.
        self.known = Some(preview);
        Some(preview)
    }

    pub(super) fn poll(&mut self, owner: &mut ReadRequests) -> bool {
        let Some(result) = self.query.as_mut().and_then(|q| q.poll(owner)) else {
            return false;
        };
        self.query = None;
        self.body = match result {
            Ok(PreviewData::Text(text)) => Body::Text(text),
            Ok(PreviewData::Image { image, size }) => Body::Image {
                size,
                image: Some(image),
                texture: None,
            },
            Ok(PreviewData::Unsupported) => Body::Unsupported,
            Ok(PreviewData::TooLarge(by)) => Body::TooLarge(by),
            Err(error) => Body::Error(error.to_string()),
        };
        true
    }

    /// 보일 항목이 바뀌었으면 이전 내용을 지우고 새로 읽는다.
    fn retarget(&mut self, target: Option<&DirEntryInfo>, remote: bool, fit_width: u32) {
        let key = target.map(|e| (e.path.clone(), e.modified, fit_width));
        if key == self.shown {
            return;
        }
        self.shown = key;
        self.query = None;
        self.body = match target {
            None => Body::NoTarget,
            Some(e) if e.is_dir || remote => Body::Unsupported,
            Some(e) => {
                self.query = Some(local_reads::preview(e.path.clone(), fit_width));
                Body::Loading
            }
        };
    }
}

/// 패널이 켜져 있고 자리가 있으면 받은 ui 의 오른쪽에 패널과 경계선을 그리고, 목록을 그릴 왼쪽 ui 를 돌려준다.
pub(super) fn split(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &mut ExplorerView,
    id_suffix: &str,
) -> egui::Ui {
    let full = ui.available_rect_before_wrap();
    // 자식 ui 는 부모 커서를 옮기지 않으므로 받은 영역 전체를 부모에 먼저 잡아 둔다.
    ui.advance_cursor_after_rect(full);
    let whole = |ui: &mut egui::Ui| {
        ui.new_child(
            egui::UiBuilder::new()
                .max_rect(full)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        )
    };
    if !view.preview.open {
        return whole(ui);
    }
    let min = theme.explorer_preview_min_width().value();
    let max = theme.explorer_preview_max_width().value();
    let line = theme.border_width.value();
    let list_min = theme.explorer_list_min_width().value();
    if full.width() < min + line + list_min {
        return whole(ui);
    }
    let zoom = theme.ui_zoom;
    let wanted = view
        .preview
        .width
        .map_or(theme.explorer_preview_width().value(), |w| w * zoom);
    let width = wanted
        .clamp(min, max)
        .min(full.width() - line - list_min)
        .max(min);
    let panel = egui::Rect::from_min_max(egui::pos2(full.right() - width, full.top()), full.max);
    let edge_x = panel.left() - line / 2.0;

    let hit = crate::state::mouse::DIVIDER_HIT_THRESHOLD.value();
    let grip = egui::Rect::from_min_max(
        egui::pos2(edge_x - hit, full.top()),
        egui::pos2(edge_x + hit, full.bottom()),
    );
    let resp = ui.interact(
        grip,
        ui.id().with(("explorer_preview_split", id_suffix)),
        egui::Sense::drag(),
    );
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if resp.dragged() {
        let next = (width - resp.drag_delta().x).clamp(min, max);
        view.preview.width = Some(next / zoom);
    }
    // 끌기를 마쳤을 때 한 번만 model 에 남긴다.
    if resp.drag_stopped() {
        view.preview.changed = true;
    }
    ui.painter().vline(
        edge_x,
        full.y_range(),
        egui::Stroke::new(line, theme.separator.to_egui_premultiplied()),
    );

    let target = (view.selected.len() == 1)
        .then(|| {
            view.entries
                .iter()
                .find(|e| view.selected.contains(&e.path))
        })
        .flatten()
        .cloned();
    let several = (view.selected.len() > 1).then(|| {
        let folders = view
            .shown()
            .filter(|e| e.is_dir && view.selected.contains(&e.path))
            .count();
        Several {
            count: view.selected.len(),
            folders,
        }
    });
    let remote = view.is_remote();
    // 그림은 패널이 가장 넓을 때의 그림 영역 폭(아래 draw_panel 의 body.shrink(spacing_md))에 맞춰 worker 에서
    // 줄인다. 패널을 줄이면 GPU 가 그만큼만 더 줄인다.
    let fit_width = (theme.explorer_preview_max_width() - theme.spacing_md * 2.0)
        .to_physical(ui.ctx().pixels_per_point())
        .value()
        .ceil() as u32;
    view.preview.retarget(target.as_ref(), remote, fit_width);
    let mut panel_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(panel)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    panel_ui.set_clip_rect(panel.intersect(ui.clip_rect()));
    draw_panel(
        &mut panel_ui,
        theme,
        &mut view.preview,
        target.as_ref(),
        several,
    );

    let list = egui::Rect::from_min_max(full.min, egui::pos2(panel.left() - line, full.bottom()));
    let mut list_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(list)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    list_ui.set_clip_rect(list.intersect(ui.clip_rect()));
    list_ui
}

/// 여러 개를 골랐을 때의 수. 선택은 보이는 항목에만 남으므로 폴더는 보이는 항목에서 센다.
#[derive(Clone, Copy)]
struct Several {
    count: usize,
    folders: usize,
}

/// 한 항목의 머리 두 줄: 이름과 "종류 · [가로 × 세로 ·] 크기".
fn item_header(pane: &PreviewPane, e: &DirEntryInfo) -> (String, String) {
    let mut facts = super::kind_word(e);
    // 상한을 넘어 펼치지 않은 그림도 머리글에서 읽은 크기를 같은 자리에 보인다.
    let pixels = match &pane.body {
        Body::Image { size, .. } => Some([size[0] as u64, size[1] as u64]),
        Body::TooLarge(TooLarge::Pixels(Some([w, h]))) => Some([u64::from(*w), u64::from(*h)]),
        _ => None,
    };
    if let Some([w, h]) = pixels {
        facts = format!("{facts} · {w} × {h}");
    }
    if !e.is_dir {
        facts = format!("{facts} · {}", human_size(false, e.size));
    }
    (e.name.clone(), facts)
}

/// 여러 개를 골랐을 때의 머리 두 줄: "N items" 와 "2 files, 1 folder".
fn several_header(several: Several) -> (String, String) {
    let files = several.count.saturating_sub(several.folders);
    (
        t_fmt("explorer.properties.items", &several.count.to_string()),
        crate::adapters::ui::popup::explorer_properties::kinds_text(files, several.folders),
    )
}

/// 머리 두 줄: 이름(body, 말줄임)과 사실 줄(caption muted).
fn header(ui: &egui::Ui, theme: &Theme, rect: egui::Rect, (name, facts): (String, String)) {
    let pad = theme.spacing_sm.value();
    let inner_w = (rect.width() - pad * 2.0).max(0.0);
    let line = |text: String, size: f32, color: egui::Color32| {
        let mut job =
            egui::text::LayoutJob::simple_singleline(text, egui::FontId::proportional(size), color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(inner_w);
        ui.fonts(|f| f.layout_job(job))
    };
    let primary = theme.text_primary().to_egui();
    let muted = theme.text_muted().to_egui();
    let name = line(name, theme.font_size_body.value(), primary);
    let facts = line(facts, theme.font_size_caption.value(), muted);
    let top = rect.center().y - (name.rect.height() + facts.rect.height()) / 2.0;
    let name_h = name.rect.height();
    let p = ui.painter();
    p.galley(egui::pos2(rect.left() + pad, top), name, primary);
    p.galley(egui::pos2(rect.left() + pad, top + name_h), facts, muted);
}

fn draw_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    pane: &mut PreviewPane,
    target: Option<&DirEntryInfo>,
    several: Option<Several>,
) {
    let rect = ui.max_rect();
    let p = ui.painter().clone();
    p.rect_filled(rect, 0.0, theme.bg_panel().to_egui());
    let head_h = theme.explorer_preview_header_height().value();
    let head = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), head_h));
    let sep = egui::Stroke::new(
        theme.border_width.value(),
        theme.separator.to_egui_premultiplied(),
    );
    if let Some(e) = target {
        header(ui, theme, head, item_header(pane, e));
    } else if let Some(several) = several {
        header(ui, theme, head, several_header(several));
    }
    p.hline(head.x_range(), head.bottom(), sep);
    let body = egui::Rect::from_min_max(egui::pos2(rect.left(), head.bottom()), rect.max);
    p.rect_filled(body, 0.0, theme.bg_sidebar().to_egui());
    let pad = theme.spacing_sm.value();
    let mut body_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body.shrink(pad))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let max_too_large = format!("{} MB", PREVIEW_MAX_BYTES >> 20);
    match &mut pane.body {
        Body::Text(text) => {
            let g = ui.painter().layout_no_wrap(
                text.clone(),
                egui::FontId::monospace(theme.font_size_caption.value()),
                theme.text_secondary().to_egui(),
            );
            let clip = body.shrink(pad);
            ui.painter()
                .with_clip_rect(clip.intersect(ui.clip_rect()))
                .galley(clip.min, g, theme.text_secondary().to_egui());
        }
        Body::Image {
            image,
            texture,
            size,
        } => {
            if let Some(img) = image.take() {
                *texture = Some(ui.ctx().load_texture(
                    "explorer_preview",
                    img,
                    egui::TextureOptions::LINEAR,
                ));
            }
            if let Some(tex) = texture {
                let area = body.shrink(theme.spacing_md.value());
                let native = super::thumbs::native_size(ui, *size);
                // 패널에 맞추되 원래 크기보다 키우지 않는다.
                let scale = (area.width() / native.x)
                    .min(area.height() / native.y)
                    .min(1.0);
                let shown = egui::Rect::from_center_size(area.center(), native * scale);
                egui::Image::from_texture(egui::load::SizedTexture::from_handle(tex))
                    .paint_at(ui, shown);
            }
        }
        Body::NoTarget => match several {
            Some(several) => {
                let title = several_title(several.count);
                state_screen::show_preview_state(
                    &mut body_ui,
                    theme,
                    state_screen::PreviewState::Sub(
                        icons::LAYERS,
                        &title,
                        t("explorer.preview.multi_sub"),
                        None,
                    ),
                );
            }
            None => state_screen::show_preview_state(
                &mut body_ui,
                theme,
                state_screen::PreviewState::Plain(icons::FILE, t("explorer.select_file")),
            ),
        },
        Body::Loading => state_screen::show_preview_state(
            &mut body_ui,
            theme,
            state_screen::PreviewState::Loading(t("explorer.preview.loading")),
        ),
        Body::Unsupported => state_screen::show_preview_state(
            &mut body_ui,
            theme,
            state_screen::PreviewState::Plain(icons::FILE, t("explorer.preview.unsupported")),
        ),
        Body::TooLarge(by) => {
            let (sub, reason) = match by {
                TooLarge::Bytes => (
                    t_fmt("explorer.preview.too_large_sub", &max_too_large),
                    None,
                ),
                TooLarge::Pixels(size) => (pixels_sub(), size.map(pixel_size_text)),
            };
            state_screen::show_preview_state(
                &mut body_ui,
                theme,
                state_screen::PreviewState::Sub(
                    icons::FILE,
                    t("explorer.preview.too_large"),
                    &sub,
                    reason.as_deref(),
                ),
            )
        }
        Body::Error(reason) => state_screen::show_preview_state(
            &mut body_ui,
            theme,
            state_screen::PreviewState::Error(t("explorer.preview.unreadable"), reason),
        ),
    }
}

/// 여러 개 선택 제목. 번역문은 이름 붙은 `{n}` 자리로 개수를 받는다.
fn several_title(count: usize) -> String {
    t("explorer.preview.multi").replace("{n}", &count.to_string())
}

/// 픽셀 상한 보조 줄. 디코딩 메모리는 이진 단위 MiB 로 쓴다.
fn pixels_sub() -> String {
    t_fmt2(
        "explorer.preview.too_large_pixels_sub",
        &MAX_IMAGE_SIDE.to_string(),
        &format!("{} MiB", MAX_DECODE_ALLOC >> 20),
    )
}

/// 상한을 넘은 그림의 실제 크기 줄.
fn pixel_size_text([w, h]: [u32; 2]) -> String {
    format!("{w} × {h} px")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_gives_the_real_size_of_a_picture_over_the_pixel_limit() {
        crate::i18n::init("en");
        let entry = DirEntryInfo {
            path: PathBuf::from("/srv/scan-poster.tif"),
            name: "scan-poster.tif".into(),
            is_dir: false,
            size: 2048,
            modified: None,
            ext: "tif".into(),
            link: Default::default(),
        };
        let mut pane = PreviewPane {
            body: Body::TooLarge(TooLarge::Pixels(Some([20000, 14000]))),
            ..Default::default()
        };
        assert_eq!(
            item_header(&pane, &entry).1,
            "TIF image · 20000 × 14000 · 2.0 KB"
        );
        pane.body = Body::TooLarge(TooLarge::Pixels(None));
        assert_eq!(item_header(&pane, &entry).1, "TIF image · 2.0 KB");
    }

    #[test]
    fn several_selected_and_the_pixel_limit_have_their_own_lines() {
        crate::i18n::init("en");
        assert_eq!(several_title(3), "3 items selected");
        let (name, facts) = several_header(Several {
            count: 3,
            folders: 1,
        });
        assert_eq!(name, t_fmt("explorer.properties.items", "3"));
        assert_eq!(facts, "2 files, 1 folder");
        assert_eq!(
            pixels_sub(),
            "Over 16384 px on a side, or needs more than 256 MiB to decode."
        );
        assert_eq!(pixel_size_text([20000, 14000]), "20000 × 14000 px");
    }

    #[test]
    fn the_model_state_is_adopted_when_it_changes_and_user_changes_are_reported_once() {
        let mut pane = PreviewPane::default();
        let saved = ExplorerPreview {
            open: true,
            width: Some(LogicalPx(320.0)),
        };
        pane.adopt(&saved);
        assert!(pane.open);
        assert_eq!(pane.width, Some(320.0));
        assert_eq!(pane.take_change(), None, "restoring is not a user change");

        // model 이 그대로면 view 에서 바꾼 값을 덮어쓰지 않는다(끌기 중 등).
        pane.width = Some(400.0);
        pane.adopt(&saved);
        assert_eq!(pane.width, Some(400.0));

        pane.toggle();
        let reported = ExplorerPreview {
            open: false,
            width: Some(LogicalPx(400.0)),
        };
        assert_eq!(pane.take_change(), Some(reported));
        assert_eq!(pane.take_change(), None);
        // model 이 알린 값으로 바뀐 뒤에는 다시 받을 것이 없다.
        pane.adopt(&reported);
        assert!(!pane.open);

        // 다른 경로가 model 을 바꾸면 따라간다.
        pane.adopt(&saved);
        assert!(pane.open);
        assert_eq!(pane.width, Some(320.0));
        assert_eq!(pane.take_change(), None);
    }
}
