//! Tasty 아이콘 전용 텍스처 로더.
//!
//! egui 0.31의 기본 텍스처 로더는 uri와 `TextureOptions`만 캐시 키로 쓴다. 그래서 같은
//! 아이콘을 다른 크기로 그리면 처음 래스터한 크기의 텍스처를 늘이거나 줄여 흐려진다.
//! 이 로더는 크기 힌트(논리 크기 × 배율)까지 키에 넣어 크기와 배율마다 따로 래스터한다.
//! 아이콘 uri가 아니면 `NotSupported`를 돌려 기본 로더에 넘긴다.
//!
//! 래스터도 직접 한다. egui_extras 0.31의 SVG 로더는 tiny-skia의 알파를 미리 곱한(premultiplied)
//! 픽셀을 곱하지 않은 값으로 넘겨, 가장자리 알파가 한 번 더 곱해져 획이 흐려진다.

use std::collections::HashMap;
use std::sync::Arc;

use egui::load::{
    BytesPoll, LoadError, SizeHint, SizedTexture, TextureLoadResult, TextureLoader, TexturePoll,
};
use egui::mutex::Mutex;
use egui::{Context, TextureHandle, TextureOptions};

/// `Icon::uri`의 공통 접두사. 이 로더는 이 접두사의 uri만 맡는다.
pub(crate) const ICON_URI_PREFIX: &str = "bytes://tasty_icon_";

type Bucket = HashMap<(TextureOptions, SizeHint), TextureHandle>;

#[derive(Default)]
struct IconTextureLoader {
    cache: Mutex<HashMap<String, Bucket>>,
}

impl IconTextureLoader {
    const ID: &'static str = egui::generate_loader_id!(IconTextureLoader);
}

impl TextureLoader for IconTextureLoader {
    fn id(&self) -> &str {
        Self::ID
    }

    fn load(
        &self,
        ctx: &Context,
        uri: &str,
        texture_options: TextureOptions,
        size_hint: SizeHint,
    ) -> TextureLoadResult {
        if !uri.starts_with(ICON_URI_PREFIX) {
            return Err(LoadError::NotSupported);
        }
        let key = (texture_options, size_hint);
        let mut cache = self.cache.lock();
        if let Some(handle) = cache.get(uri).and_then(|bucket| bucket.get(&key)) {
            return Ok(TexturePoll::Ready {
                texture: SizedTexture::from_handle(handle),
            });
        }
        let bytes = match ctx.try_load_bytes(uri)? {
            BytesPoll::Pending { size } => return Ok(TexturePoll::Pending { size }),
            BytesPoll::Ready { bytes, .. } => bytes,
        };
        let image = rasterize(&bytes, size_hint).map_err(LoadError::Loading)?;
        let handle = ctx.load_texture(uri, image, texture_options);
        let texture = SizedTexture::from_handle(&handle);
        cache.entry(uri.to_owned()).or_default().insert(key, handle);
        Ok(TexturePoll::Ready { texture })
    }

    fn forget(&self, uri: &str) {
        self.cache.lock().remove(uri);
    }

    fn forget_all(&self) {
        self.cache.lock().clear();
    }

    fn byte_size(&self) -> usize {
        self.cache
            .lock()
            .values()
            .flat_map(|bucket| bucket.values())
            .map(TextureHandle::byte_size)
            .sum()
    }
}

/// SVG를 크기 힌트에 맞춰 래스터한다. 크기 계산은 egui_extras 0.31과 같다(가로세로 비율 유지).
/// tiny-skia 픽셀은 premultiplied라 그대로 `from_rgba_premultiplied`로 넘긴다.
fn rasterize(svg: &[u8], size_hint: SizeHint) -> Result<egui::ColorImage, String> {
    use resvg::tiny_skia::{IntSize, Pixmap, Transform};
    use resvg::usvg::{Options, Tree, TreeParsing as _};

    let tree = Tree::from_data(svg, &Options::default()).map_err(|e| e.to_string())?;
    let source = tree.size.to_int_size();
    let size = match size_hint {
        SizeHint::Size(w, h) => IntSize::from_wh(w, h).map(|target| source.scale_to(target)),
        SizeHint::Width(w) => source.scale_to_width(w),
        SizeHint::Height(h) => source.scale_to_height(h),
        SizeHint::Scale(z) => source.scale_by(z.into_inner()),
    }
    .ok_or_else(|| "SVG를 요청한 크기로 바꿀 수 없다".to_owned())?;
    let (w, h) = (size.width(), size.height());
    let mut pixmap = Pixmap::new(w, h).ok_or_else(|| format!("{w}x{h} 픽스맵을 만들 수 없다"))?;
    let transform =
        Transform::from_scale(w as f32 / tree.size.width(), h as f32 / tree.size.height());
    resvg::Tree::from_usvg(&tree).render(transform, &mut pixmap.as_mut());
    Ok(egui::ColorImage::from_rgba_premultiplied(
        [w as usize, h as usize],
        pixmap.data(),
    ))
}

/// 아이콘 텍스처 로더를 설치한다. 이미 설치돼 있으면 아무것도 하지 않는다.
/// 아이콘 래스터는 이 로더가 직접 하므로 별도의 SVG 이미지 로더가 없어도 된다.
pub fn install_texture_loader(ctx: &Context) {
    if !ctx.is_loader_installed(IconTextureLoader::ID) {
        ctx.add_texture_loader(Arc::new(IconTextureLoader::default()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 기본 로더 확인용. 바이트 로더와 SVG 래스터를 대신해 요청 크기만큼의 빈 이미지를 돌려준다.
    struct SizedImageLoader;

    impl egui::load::ImageLoader for SizedImageLoader {
        fn id(&self) -> &str {
            "tasty_icons::texture_loader::tests::SizedImageLoader"
        }

        fn load(&self, _: &Context, _: &str, size_hint: SizeHint) -> egui::load::ImageLoadResult {
            let side = match size_hint {
                SizeHint::Size(w, _) => w as usize,
                _ => 1,
            };
            Ok(egui::load::ImagePoll::Ready {
                image: Arc::new(egui::ColorImage::new([side, side], egui::Color32::WHITE)),
            })
        }

        fn forget(&self, _: &str) {}

        fn forget_all(&self) {}

        fn byte_size(&self) -> usize {
            0
        }
    }

    fn ctx() -> Context {
        let ctx = Context::default();
        ctx.add_image_loader(Arc::new(SizedImageLoader));
        install_texture_loader(&ctx);
        ctx.include_bytes(crate::CHEVRON_DOWN.uri, crate::CHEVRON_DOWN.svg.as_bytes());
        ctx
    }

    fn texture_side(ctx: &Context, uri: &str, side: u32) -> f32 {
        match ctx.try_load_texture(uri, TextureOptions::default(), SizeHint::Size(side, side)) {
            Ok(TexturePoll::Ready { texture }) => texture.size.x,
            Ok(TexturePoll::Pending { .. }) => panic!("{uri} @{side}: pending"),
            Err(e) => panic!("{uri} @{side}: {e}"),
        }
    }

    #[test]
    fn an_icon_gets_its_own_texture_for_each_requested_size() {
        let ctx = ctx();
        let uri = crate::CHEVRON_DOWN.uri;
        assert_eq!(texture_side(&ctx, uri, 12), 12.0);
        assert_eq!(texture_side(&ctx, uri, 16), 16.0);
        assert_eq!(texture_side(&ctx, uri, 24), 24.0);
        assert_eq!(texture_side(&ctx, uri, 12), 12.0);
    }

    #[test]
    fn a_non_icon_uri_is_left_to_the_default_loader() {
        let ctx = ctx();
        // 기본 로더는 크기를 무시하므로 첫 요청 크기가 남는다.
        assert_eq!(texture_side(&ctx, "bytes://other.svg", 12), 12.0);
        assert_eq!(texture_side(&ctx, "bytes://other.svg", 16), 12.0);
    }

    #[test]
    fn installing_twice_keeps_one_loader() {
        let ctx = ctx();
        install_texture_loader(&ctx);
        let count = ctx
            .loaders()
            .texture
            .lock()
            .iter()
            .filter(|l| l.id() == IconTextureLoader::ID)
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn both_icon_macros_produce_uris_this_loader_serves() {
        for icon in [crate::CHEVRON_DOWN, crate::STAR_FILL] {
            assert!(icon.uri.starts_with(ICON_URI_PREFIX), "{}", icon.uri);
        }
    }

    /// 흰 획을 덮은 비율이 a인 픽셀의 premultiplied 값은 (a, a, a, a)다. 같은 SVG를 tiny-skia로
    /// 직접 그린 알파와 한 픽셀씩 비교한다. 곱하지 않은 값으로 넘기면 가장자리 RGB가 a보다 작아진다.
    #[test]
    fn stroke_edges_keep_the_rasterizer_alpha() {
        use resvg::tiny_skia::{Pixmap, Transform};
        use resvg::usvg::{Options, Tree, TreeParsing as _};

        for icon in [crate::CHEVRON_LEFT, crate::CHEVRON_DOWN, crate::FOLDER] {
            for side in [12u32, 16] {
                let image = rasterize(icon.svg.as_bytes(), SizeHint::Size(side, side))
                    .unwrap_or_else(|e| panic!("{}: {e}", icon.uri));
                assert_eq!(image.size, [side as usize, side as usize]);

                let tree =
                    Tree::from_data(icon.svg.as_bytes(), &Options::default()).expect("parse");
                let mut reference = Pixmap::new(side, side).expect("pixmap");
                let scale = side as f32 / 24.0;
                resvg::Tree::from_usvg(&tree)
                    .render(Transform::from_scale(scale, scale), &mut reference.as_mut());

                let mut edges = 0;
                for (got, want) in image.pixels.iter().zip(reference.pixels()) {
                    let a = want.alpha();
                    assert_eq!(got.to_array(), [a, a, a, a], "{} @{side}", icon.uri);
                    if a > 0 && a < u8::MAX {
                        edges += 1;
                    }
                }
                assert!(edges > 0, "{} @{side}: 가장자리 픽셀이 없다", icon.uri);
            }
        }
    }
}
