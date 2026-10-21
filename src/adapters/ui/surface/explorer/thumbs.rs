//! Grid 보기의 썸네일. 시안 "Grid thumbnails"를 따른다.
//! 모든 Grid 셀은 `explorer_grid_thumb_size`(40) 슬롯을 잡아 썸네일 유무와 관계없이 행 높이가 같다.
//! 로컬 그림 파일만 화면에 보인 셀부터 read worker 에서 만들고, 경로와 내용 키(수정 시각·크기·Unix 의
//! inode·ctime, `DirEntryInfo::content_key`)로 캐시한다.
//! 캐시가 차면 이번 프레임에 보이지 않은 항목 중 가장 오래 쓰지 않은 것부터 버린다.
//! 만드는 동안이나 앱 크기 상한을 넘으면 16 글리프(accent-info)를 그대로 둔다. 원격 탐색기는 글리프만 쓴다.

use std::collections::HashMap;
use std::path::PathBuf;

use tasty_type_appearance::theme::Theme;

use super::view::DirEntryInfo;
use crate::adapters::ui::icons::Icon;
use crate::app::local_reads::{self, PREVIEW_MAX_BYTES, Query, ReadRequests};
use crate::core::fs_list::ContentKey;

/// 캐시에 두는 썸네일 수. 넘으면 화면에 보이지 않는 항목을 오래 쓰지 않은 순서로 버린다.
const MAX_SLOTS: usize = 512;

enum State {
    Pending(Query<egui::ColorImage>),
    Decoded(egui::ColorImage),
    Ready(egui::TextureHandle),
    Failed,
}

struct Slot {
    key: ContentKey,
    state: State,
    /// 마지막으로 화면에 보인 프레임 번호.
    used: u64,
}

/// 탐색기 하나의 썸네일 캐시.
#[derive(Default)]
pub struct Thumbs {
    slots: HashMap<PathBuf, Slot>,
    /// 프레임 번호. 매 프레임 `poll` 이 올린다.
    frame: u64,
}

impl Thumbs {
    pub(super) fn poll(&mut self, owner: &mut ReadRequests) -> bool {
        self.frame += 1;
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
    /// 캐시는 로컬 파일로 만든 것이라, 같은 경로의 원격 항목에는 쓰지 않는다.
    pub(super) fn texture(
        &mut self,
        ctx: &egui::Context,
        e: &DirEntryInfo,
        remote: bool,
    ) -> Option<egui::TextureHandle> {
        if remote {
            return None;
        }
        let slot = self.slots.get_mut(&e.path)?;
        if slot.key != e.content_key() {
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

    /// 화면에 보인 셀의 썸네일을 요청한다. 대상이 아니거나 이미 있으면 쓴 시각만 갱신한다.
    pub(super) fn want(&mut self, e: &DirEntryInfo, remote: bool) {
        if remote
            || e.is_dir
            || e.size > PREVIEW_MAX_BYTES
            || !local_reads::decodable_image_ext(&e.ext)
        {
            return;
        }
        let frame = self.frame;
        if let Some(slot) = self.slots.get_mut(&e.path)
            && slot.key == e.content_key()
        {
            slot.used = frame;
            return;
        }
        if !self.slots.contains_key(&e.path) && self.slots.len() >= MAX_SLOTS && !self.evict_one() {
            return;
        }
        self.slots.insert(
            e.path.clone(),
            Slot {
                key: e.content_key(),
                state: State::Pending(local_reads::thumbnail(e.path.clone())),
                used: frame,
            },
        );
    }

    /// 캐시에 든 칸 수.
    #[cfg(test)]
    pub(super) fn cached(&self) -> usize {
        self.slots.len()
    }

    /// 이번 프레임에 보이지 않은 항목 중 가장 오래 쓰지 않은 것을 버린다. 모두 보이면 버리지 않는다.
    fn evict_one(&mut self) -> bool {
        let frame = self.frame;
        let Some(oldest) = self
            .slots
            .iter()
            .filter(|(_, slot)| slot.used < frame)
            .min_by_key(|(_, slot)| slot.used)
            .map(|(path, _)| path.clone())
        else {
            return false;
        };
        self.slots.remove(&oldest);
        true
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn image_entry(i: usize) -> DirEntryInfo {
        DirEntryInfo {
            path: PathBuf::from(format!("/nonexistent/img{i:04}.png")),
            name: format!("img{i:04}.png"),
            is_dir: false,
            size: 0,
            modified: None,
            ext: "png".into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_full_cache_drops_the_least_recently_shown_off_screen_entry() {
        let mut thumbs = Thumbs::default();
        for i in 0..MAX_SLOTS {
            thumbs.frame += 1;
            thumbs.want(&image_entry(i), false);
        }
        assert_eq!(thumbs.slots.len(), MAX_SLOTS);
        // 다음 프레임에 처음 항목을 다시 보이고 새 항목을 요청하면, 그다음으로 오래된 항목이 빠진다.
        thumbs.frame += 1;
        thumbs.want(&image_entry(0), false);
        thumbs.want(&image_entry(MAX_SLOTS), false);
        assert_eq!(thumbs.slots.len(), MAX_SLOTS);
        assert!(thumbs.slots.contains_key(&image_entry(0).path));
        assert!(thumbs.slots.contains_key(&image_entry(MAX_SLOTS).path));
        assert!(!thumbs.slots.contains_key(&image_entry(1).path));
    }

    #[test]
    fn entries_shown_this_frame_are_not_evicted() {
        let mut thumbs = Thumbs::default();
        thumbs.frame += 1;
        for i in 0..=MAX_SLOTS {
            thumbs.want(&image_entry(i), false);
        }
        assert_eq!(thumbs.slots.len(), MAX_SLOTS);
        assert!(!thumbs.slots.contains_key(&image_entry(MAX_SLOTS).path));
    }

    use crate::app::local_reads::LocalReads;
    use std::time::Duration;

    /// `w × h` PNG 를 쓰고 수정 시각을 `at` 초(UNIX)로 둔다. 목록이 읽는 것과 같은 항목을 돌려준다.
    fn write_image(path: &std::path::Path, w: u32, h: u32, at: u64) -> DirEntryInfo {
        image::RgbImage::new(w, h).save(path).expect("write png");
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(at))
            .unwrap();
        drop(file);
        crate::core::fs_list::read_dir_entries(path.parent().unwrap())
            .unwrap()
            .into_iter()
            .find(|e| e.path == path)
            .expect("listed")
    }

    /// 넘긴 요청을 worker 로 돌려 칸이 Pending 을 벗어날 때까지 받는다.
    fn settle(thumbs: &mut Thumbs, owner: &mut LocalReads, path: &std::path::Path) {
        loop {
            owner.reap();
            owner.drive(|requests| {
                thumbs.poll(requests);
                true
            });
            if !matches!(thumbs.slots[path].state, State::Pending(_)) {
                return;
            }
            std::thread::yield_now();
        }
    }

    fn decoded_size(thumbs: &Thumbs, path: &std::path::Path) -> [usize; 2] {
        match &thumbs.slots[path].state {
            State::Decoded(image) => image.size,
            _ => panic!("not decoded"),
        }
    }

    fn shutdown(mut owner: LocalReads) {
        while owner.poll_shutdown() != 0 {
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_new_modified_time_drops_the_cached_thumbnail_and_reads_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.png");
        let mut owner = LocalReads::default();
        let mut thumbs = Thumbs::default();
        let old = write_image(&path, 2, 2, 1_000_000);
        thumbs.want(&old, false);
        settle(&mut thumbs, &mut owner, &path);
        assert_eq!(decoded_size(&thumbs, &path), [2, 2]);

        let new = write_image(&path, 3, 3, 2_000_000);
        assert_ne!(old.modified, new.modified);
        let ctx = egui::Context::default();
        assert!(
            thumbs.texture(&ctx, &new, false).is_none(),
            "the old picture is not drawn for the new file"
        );
        thumbs.want(&new, false);
        assert!(matches!(thumbs.slots[&path].state, State::Pending(_)));
        settle(&mut thumbs, &mut owner, &path);
        assert_eq!(decoded_size(&thumbs, &path), [3, 3]);
        assert!(thumbs.texture(&ctx, &new, false).is_some());
        shutdown(owner);
    }

    /// `cp -p`·`touch -r` 처럼 수정 시각을 되돌린 교체. OS 값(`change`)을 지워 Windows 처럼 크기만으로
    /// 가르는지 본다.
    #[test]
    fn a_replacement_that_keeps_the_modified_time_but_not_the_size_reads_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.png");
        let mut owner = LocalReads::default();
        let mut thumbs = Thumbs::default();
        let mut old = write_image(&path, 2, 2, 1_000_000);
        old.change = None;
        thumbs.want(&old, false);
        settle(&mut thumbs, &mut owner, &path);

        let mut new = write_image(&path, 30, 30, 1_000_000);
        new.change = None;
        assert_eq!(old.modified, new.modified);
        assert_ne!(old.size, new.size);
        let ctx = egui::Context::default();
        assert!(thumbs.texture(&ctx, &new, false).is_none());
        thumbs.want(&new, false);
        settle(&mut thumbs, &mut owner, &path);
        assert_eq!(decoded_size(&thumbs, &path), [30, 30]);
        shutdown(owner);
    }

    #[test]
    fn an_untouched_cell_shown_again_keeps_its_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.png");
        let mut owner = LocalReads::default();
        let mut thumbs = Thumbs::default();
        let entry = write_image(&path, 2, 2, 1_000_000);
        thumbs.want(&entry, false);
        settle(&mut thumbs, &mut owner, &path);
        let relisted = crate::core::fs_list::read_dir_entries(dir.path())
            .unwrap()
            .remove(0);
        thumbs.want(&relisted, false);
        assert!(matches!(thumbs.slots[&path].state, State::Decoded(_)));
        shutdown(owner);
    }

    #[test]
    fn a_late_result_for_the_old_file_does_not_land_on_the_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.png");
        let mut owner = LocalReads::default();
        let mut thumbs = Thumbs::default();
        let old = write_image(&path, 2, 2, 1_000_000);
        thumbs.want(&old, false);
        // 요청을 넘기고 worker 가 끝나기를 기다린다. 결과는 받지 않은 채 영수증에 남는다.
        owner.drive(|requests| {
            thumbs.poll(requests);
            true
        });
        owner.wait_idle();

        let new = write_image(&path, 3, 3, 2_000_000);
        thumbs.want(&new, false);
        settle(&mut thumbs, &mut owner, &path);
        assert_eq!(decoded_size(&thumbs, &path), [3, 3]);
        assert_eq!(thumbs.slots[&path].key, new.content_key());
        shutdown(owner);
    }

    #[test]
    fn a_mirror_explorer_never_reads_a_local_file_with_the_same_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.png");
        let entry = write_image(&path, 2, 2, 1_000_000);
        let mut thumbs = Thumbs::default();
        thumbs.want(&entry, true);
        assert!(thumbs.slots.is_empty(), "a remote cell asks for nothing");
        thumbs.want(&entry, false);
        assert!(thumbs.slots.contains_key(&path), "the same local cell does");
    }

    #[test]
    fn a_thumbnail_made_while_local_is_not_drawn_once_the_view_is_a_mirror() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.png");
        let entry = write_image(&path, 2, 2, 1_000_000);
        let mut owner = LocalReads::default();
        let mut thumbs = Thumbs::default();
        thumbs.want(&entry, false);
        settle(&mut thumbs, &mut owner, &path);
        let ctx = egui::Context::default();
        assert!(thumbs.texture(&ctx, &entry, false).is_some());
        // 같은 경로·같은 수정 시각의 원격 항목은 로컬에서 만든 그림을 쓰지 않는다.
        assert!(thumbs.texture(&ctx, &entry, true).is_none());
        assert!(thumbs.texture(&ctx, &entry, false).is_some());
        shutdown(owner);
    }
}
