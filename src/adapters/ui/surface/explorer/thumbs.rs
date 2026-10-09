//! Grid 보기의 썸네일. 시안 "Grid thumbnails"를 따른다.
//! 모든 Grid 셀은 `explorer_grid_thumb_size`(40) 슬롯을 잡아 썸네일 유무와 관계없이 행 높이가 같다.
//! 로컬 그림 파일만 화면에 보인 셀부터 read worker 에서 만들고, 경로와 수정 시각으로 캐시한다.
//! 만드는 동안이나 앱 크기 상한을 넘으면 16 글리프(accent-info)를 그대로 둔다. 원격 탐색기는 글리프만 쓴다.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

use tasty_type_appearance::theme::Theme;

use super::view::DirEntryInfo;
use crate::adapters::ui::icons::Icon;
use crate::app::local_reads::{self, PREVIEW_MAX_BYTES, Query, ReadRequests};

/// 캐시에 두는 썸네일 수. 넘으면 지금 폴더에 없는 항목부터 버린다.
const MAX_SLOTS: usize = 512;

enum State {
    Pending(Query<egui::ColorImage>),
    Decoded(egui::ColorImage),
    Ready(egui::TextureHandle),
    Failed,
}

struct Slot {
    modified: Option<SystemTime>,
    state: State,
}

/// 탐색기 하나의 썸네일 캐시.
#[derive(Default)]
pub struct Thumbs {
    slots: HashMap<PathBuf, Slot>,
}

impl Thumbs {
    pub(super) fn poll(&mut self, owner: &mut ReadRequests) -> bool {
        let mut changed = false;
        for slot in self.slots.values_mut() {
            if let State::Pending(query) = &mut slot.state
                && let Some(result) = query.poll(owner)
            {
                slot.state = match result {
                    Ok(image) => State::Decoded(image),
                    Err(error) => {
                        tracing::debug!(%error, "explorer thumbnail skipped");
                        State::Failed
                    }
                };
                changed = true;
            }
        }
        changed
    }

    /// 만들어진 썸네일. 디코딩만 끝났으면 여기서 텍스처로 올린다.
    pub(super) fn texture(
        &mut self,
        ctx: &egui::Context,
        e: &DirEntryInfo,
    ) -> Option<egui::TextureHandle> {
        let slot = self.slots.get_mut(&e.path)?;
        if slot.modified != e.modified {
            return None;
        }
        if let State::Decoded(_) = slot.state {
            let State::Decoded(image) = std::mem::replace(&mut slot.state, State::Failed) else {
                unreachable!("checked above");
            };
            let name = format!("explorer_thumb:{}", e.path.display());
            slot.state = State::Ready(ctx.load_texture(name, image, egui::TextureOptions::LINEAR));
        }
        match &slot.state {
            State::Ready(texture) => Some(texture.clone()),
            _ => None,
        }
    }

    /// 화면에 보인 셀의 썸네일을 요청한다. 대상이 아니거나 이미 있으면 아무것도 하지 않는다.
    pub(super) fn want(&mut self, e: &DirEntryInfo, remote: bool, listed: &[DirEntryInfo]) {
        if remote
            || e.is_dir
            || e.size > PREVIEW_MAX_BYTES
            || !local_reads::decodable_image_ext(&e.ext)
        {
            return;
        }
        if self
            .slots
            .get(&e.path)
            .is_some_and(|slot| slot.modified == e.modified)
        {
            return;
        }
        if self.slots.len() >= MAX_SLOTS {
            self.slots
                .retain(|path, _| listed.iter().any(|entry| &entry.path == path));
            if self.slots.len() >= MAX_SLOTS {
                return;
            }
        }
        self.slots.insert(
            e.path.clone(),
            Slot {
                modified: e.modified,
                state: State::Pending(local_reads::thumbnail(e.path.clone())),
            },
        );
    }
}

/// Grid 셀의 40 슬롯을 그린다. 썸네일은 40 × 40 에 맞추고(키우지 않는다) 1px separator 테두리와
/// radius-sm 을 두며, 없으면 16 글리프를 슬롯 가운데에 둔다.
pub(super) fn paint_slot(
    ui: &egui::Ui,
    theme: &Theme,
    slot: egui::Rect,
    thumb: Option<&egui::TextureHandle>,
    icon: Icon,
    color: egui::Color32,
) {
    let Some(texture) = thumb else {
        let glyph = theme.icon_glyph_size_md.value();
        icon.image(glyph, color).paint_at(
            ui,
            egui::Rect::from_center_size(slot.center(), egui::vec2(glyph, glyph)),
        );
        return;
    };
    let native = native_size(ui, texture.size());
    let scale = (slot.width() / native.x)
        .min(slot.height() / native.y)
        .min(1.0);
    let shown = egui::Rect::from_center_size(slot.center(), native * scale);
    let radius = theme.corner_radius_sm.value();
    let painter = ui.painter();
    egui::Image::from_texture(egui::load::SizedTexture::from_handle(texture))
        .corner_radius(radius)
        .paint_at(ui, shown);
    painter.rect_stroke(
        shown,
        radius,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.separator.to_egui_premultiplied(),
        ),
        egui::StrokeKind::Inside,
    );
}

/// 그림 픽셀 크기를 화면 논리 크기로 바꾼다. 맞출 때 이 크기보다 키우지 않는다.
pub(super) fn native_size(ui: &egui::Ui, px: [usize; 2]) -> egui::Vec2 {
    use tasty_type_geometry::length::PhysicalPx;
    let ppp = ui.ctx().pixels_per_point();
    egui::vec2(
        PhysicalPx(px[0] as f32).to_logical(ppp).value(),
        PhysicalPx(px[1] as f32).to_logical(ppp).value(),
    )
}
