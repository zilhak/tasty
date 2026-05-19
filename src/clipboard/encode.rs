//! arboard 의 클립보드 이미지 → 히스토리 저장용 PNG 인코딩.

use crate::clipboard_history::ImageData;

/// Encode arboard image data to PNG for clipboard history storage.
pub(crate) fn encode_clipboard_image(img: &arboard::ImageData<'_>) -> Option<ImageData> {
    use image::ImageEncoder;
    let w = img.width as u32;
    let h = img.height as u32;
    let mut png_buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new_with_quality(
        &mut png_buf,
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Sub,
    );
    if let Err(e) = encoder.write_image(&img.bytes, w, h, image::ExtendedColorType::Rgba8) {
        tracing::warn!("Failed to encode clipboard image to PNG: {e}");
        return None;
    }
    Some(ImageData {
        png_bytes: png_buf,
        width: w,
        height: h,
    })
}
