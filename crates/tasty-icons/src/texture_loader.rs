//! Tasty 아이콘 전용 텍스처 로더.
//!
//! egui 0.31의 기본 텍스처 로더는 uri와 `TextureOptions`만 캐시 키로 쓴다. 그래서 같은
//! 아이콘을 다른 크기로 그리면 처음 래스터한 크기의 텍스처를 늘이거나 줄여 흐려진다.
//! 이 로더는 크기 힌트(논리 크기 × 배율)까지 키에 넣어 크기와 배율마다 따로 래스터한다.
//! 아이콘 uri가 아니면 `NotSupported`를 돌려 기본 로더에 넘긴다.

use std::collections::HashMap;
use std::sync::Arc;

use egui::load::{
    ImagePoll, LoadError, SizeHint, SizedTexture, TextureLoadResult, TextureLoader, TexturePoll,
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
        match ctx.try_load_image(uri, size_hint)? {
            ImagePoll::Pending { size } => Ok(TexturePoll::Pending { size }),
            ImagePoll::Ready { image } => {
                let handle = ctx.load_texture(uri, image, texture_options);
                let texture = SizedTexture::from_handle(&handle);
                cache.entry(uri.to_owned()).or_default().insert(key, handle);
                Ok(TexturePoll::Ready { texture })
            }
        }
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

/// 아이콘 텍스처 로더를 설치한다. 이미 설치돼 있으면 아무것도 하지 않는다.
/// SVG를 래스터하는 이미지 로더(`egui_extras::install_image_loaders`)는 따로 설치해야 한다.
pub fn install_texture_loader(ctx: &Context) {
    if !ctx.is_loader_installed(IconTextureLoader::ID) {
        ctx.add_texture_loader(Arc::new(IconTextureLoader::default()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 바이트 로더와 SVG 래스터를 대신해, 요청 크기만큼의 빈 이미지를 돌려준다.
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
            Ok(ImagePoll::Ready {
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
}
