//! `image_view` 단위 테스트.

use super::*;

#[test]
fn drop_view_removes_entry() {
    let mut store = ImageViewStore::default();
    store.views.insert(7, ImageView::new());
    store.drop_view(7);
    assert!(store.views.is_empty());
}

// 테스트 더미 — 정상 운영 경로 아님.
#[allow(clippy::disallowed_methods)]
#[test]
fn alpha_blend_transparent_fg_returns_bg() {
    let bg = Color32::from_rgba_unmultiplied(50, 100, 150, 200);
    let fg = Color32::TRANSPARENT;
    assert_eq!(alpha_blend(bg, fg), bg);
}

#[allow(clippy::disallowed_methods)]
#[test]
fn alpha_blend_opaque_fg_replaces_bg() {
    let bg = Color32::from_rgb(10, 20, 30);
    let fg = Color32::from_rgb(200, 150, 100);
    let out = alpha_blend(bg, fg);
    assert_eq!(out.r(), 200);
    assert_eq!(out.g(), 150);
    assert_eq!(out.b(), 100);
}
