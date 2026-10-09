//! 목록 오른쪽 미리보기 패널. 시안 `YPreview`(갤러리 "Preview panel")를 따른다.
//! 폭은 `explorer_preview_width`(288)에서 시작해 경계선을 끌어 `explorer_preview_min_width`…`max_width`
//! 사이로 바꾸며 이 탐색기 동안 기억한다. 칸이 좁아 목록 최소 폭을 남기지 못하면 패널만 숨고 토글은 켜진 채다.
//! 선택이 하나일 때 그 항목을 보인다. 새 선택은 이전 미리보기를 바로 지우고 읽기가 끝날 때까지 Loading 을 보인다.
//! 읽기는 read worker 가 맡는다(`local_reads::preview`). 원격 탐색기는 파일 내용을 받을 경로가 없어
//! 지원하지 않는 형식 상태를 보인다.

use std::path::PathBuf;
use std::time::SystemTime;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{ControlSize, IconButton, IconButtonVariant};

use super::state_screen;
use super::view::{DirEntryInfo, ExplorerView, human_size};
use crate::adapters::ui::icons;
use crate::app::local_reads::{self, PREVIEW_MAX_BYTES, PreviewData, Query, ReadRequests};
use crate::i18n::{t, t_fmt};

/// 시안 `YPreview` 머리 높이 `--tasty-size-40`. 대응 컴포넌트 토큰이 없다.
const HEAD_H: LogicalPx = LogicalPx(40.0);

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
    TooLarge,
    Error(String),
}

/// 탐색기 하나의 미리보기 패널 상태.
pub struct PreviewPane {
    /// 토글. 칸이 좁아 패널이 숨어도 켜진 채다.
    pub open: bool,
    /// 사용자가 끌어 정한 폭을 UI 배율로 나눈 값. `None` 이면 토큰 기본값을 쓴다.
    width: Option<f32>,
    /// 지금 보이는 항목과 그 수정 시각. 같은 파일이 바뀌면 다시 읽는다.
    shown: Option<(PathBuf, Option<SystemTime>)>,
    query: Option<Query<PreviewData>>,
    body: Body,
}

impl Default for PreviewPane {
    fn default() -> Self {
        Self {
            open: false,
            width: None,
            shown: None,
            query: None,
            body: Body::NoTarget,
        }
    }
}

impl PreviewPane {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if !self.open {
            self.shown = None;
            self.query = None;
            self.body = Body::NoTarget;
        }
    }

    pub(super) fn poll(&mut self, owner: &mut ReadRequests) -> bool {
        let Some(result) = self.query.as_mut().and_then(|q| q.poll(owner)) else {
            return false;
        };
        self.query = None;
        self.body = match result {
            Ok(PreviewData::Text(text)) => Body::Text(text),
            Ok(PreviewData::Image(image)) => Body::Image {
                size: image.size,
                image: Some(image),
                texture: None,
            },
            Ok(PreviewData::Unsupported) => Body::Unsupported,
            Ok(PreviewData::TooLarge) => Body::TooLarge,
            Err(error) => Body::Error(error.to_string()),
        };
        true
    }

    /// 보일 항목이 바뀌었으면 이전 내용을 지우고 새로 읽는다.
    fn retarget(&mut self, target: Option<&DirEntryInfo>, remote: bool) {
        let key = target.map(|e| (e.path.clone(), e.modified));
        if key == self.shown {
            return;
        }
        self.shown = key;
        self.query = None;
        self.body = match target {
            None => Body::NoTarget,
            Some(e) if e.is_dir || remote => Body::Unsupported,
            Some(e) => {
                self.query = Some(local_reads::preview(e.path.clone()));
                Body::Loading
            }
        };
    }
}

/// 미리보기 토글 버튼. 시안 툴바 view 묶음의 columns 글리프, 켜지면 active.
pub(super) fn toggle_button(ui: &mut egui::Ui, theme: &Theme, view: &mut ExplorerView) {
    let resp = IconButton::new()
        .variant(IconButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .active(view.preview.open)
        .show(ui, theme, &|ui, rect, c| {
            icons::COLUMNS.image(rect.height(), c).paint_at(ui, rect)
        })
        .on_hover_text(t("explorer.preview.toggle"));
    if resp.clicked() {
        view.preview.toggle();
    }
}

/// 토글 버튼이 차지하는 폭. 툴바가 주소창 폭을 계산할 때 뺀다.
pub(super) fn toggle_button_width(theme: &Theme) -> f32 {
    ControlSize::Sm.height(theme)
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
    // 목록에 남길 최소 폭은 시안에 없어 패널 최소 폭과 같게 둔다(디자인 질문으로 올렸다).
    let list_min = min;
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
    let remote = view.is_remote();
    view.preview.retarget(target.as_ref(), remote);
    let mut panel_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(panel)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    panel_ui.set_clip_rect(panel.intersect(ui.clip_rect()));
    draw_panel(&mut panel_ui, theme, &mut view.preview, target.as_ref());

    let list = egui::Rect::from_min_max(full.min, egui::pos2(panel.left() - line, full.bottom()));
    let mut list_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(list)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    list_ui.set_clip_rect(list.intersect(ui.clip_rect()));
    list_ui
}

/// 머리 두 줄: 이름(body, 말줄임)과 "종류 · [가로 × 세로 ·] 크기"(caption muted).
fn header(ui: &egui::Ui, theme: &Theme, rect: egui::Rect, pane: &PreviewPane, e: &DirEntryInfo) {
    let pad = theme.spacing_sm.value();
    let inner_w = (rect.width() - pad * 2.0).max(0.0);
    let mut facts = super::type_label(e);
    if let Body::Image { size, .. } = &pane.body {
        facts = format!("{facts} · {} × {}", size[0], size[1]);
    }
    if !e.is_dir {
        facts = format!("{facts} · {}", human_size(false, e.size));
    }
    let line = |text: String, size: f32, color: egui::Color32| {
        let mut job =
            egui::text::LayoutJob::simple_singleline(text, egui::FontId::proportional(size), color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(inner_w);
        ui.fonts(|f| f.layout_job(job))
    };
    let primary = theme.text_primary().to_egui();
    let muted = theme.text_muted().to_egui();
    let name = line(e.name.clone(), theme.font_size_body.value(), primary);
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
) {
    let rect = ui.max_rect();
    let p = ui.painter().clone();
    p.rect_filled(rect, 0.0, theme.bg_panel().to_egui());
    let head_h = HEAD_H.value() * theme.ui_zoom;
    let head = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), head_h));
    let sep = egui::Stroke::new(
        theme.border_width.value(),
        theme.separator.to_egui_premultiplied(),
    );
    if let Some(e) = target {
        header(ui, theme, head, pane, e);
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
        Body::NoTarget => {
            state_screen::show_preview_state(
                &mut body_ui,
                theme,
                state_screen::PreviewState::Plain(icons::FILE, t("explorer.select_file")),
            );
        }
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
        Body::TooLarge => state_screen::show_preview_state(
            &mut body_ui,
            theme,
            state_screen::PreviewState::Sub(
                icons::FILE,
                t("explorer.preview.too_large"),
                &t_fmt("explorer.preview.too_large_sub", &max_too_large),
            ),
        ),
        Body::Error(reason) => state_screen::show_preview_state(
            &mut body_ui,
            theme,
            state_screen::PreviewState::Error(t("explorer.preview.unreadable"), reason),
        ),
    }
}
