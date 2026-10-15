//! 저장한 캡처 PNG 를 클립보드 이미지로 준비한다. 디코딩은 크기에 비례해 오래 걸리므로 UI 스레드가
//! 아닌 캡처 워커에서 호출한다.

use crate::ports::clipboard::ClipboardImage;

/// PNG 바이트를 디코딩해 클립보드 이미지로 만든다. `image` 크레이트의 기본 할당 한도(512MiB)를
/// 넘는 그림은 오류다. 실패하면 경로 텍스트만 게시하도록 사유를 돌려준다.
pub(crate) fn decode_png(png: Vec<u8>) -> Result<ClipboardImage, String> {
    let decoded = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .map_err(|error| format!("PNG decode failed: {error}"))?
        .into_rgba8();
    let (width, height) = decoded.dimensions();
    Ok(ClipboardImage {
        png,
        width,
        height,
        rgba: decoded.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba(pixel));
        let mut png = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode");
        png
    }

    #[test]
    fn a_png_keeps_its_bytes_and_decodes_to_rgba() {
        let png = encode(3, 2, [10, 20, 30, 255]);
        let image = decode_png(png.clone()).expect("decode");
        assert_eq!((image.width, image.height), (3, 2));
        assert_eq!(image.png, png);
        assert_eq!(image.rgba.len(), 3 * 2 * 4);
        assert_eq!(&image.rgba[..4], &[10, 20, 30, 255]);
    }

    #[test]
    fn a_truncated_png_is_a_decode_failure() {
        let png = encode(8, 8, [1, 2, 3, 255]);
        let error = decode_png(png[..png.len() / 2].to_vec()).unwrap_err();
        assert!(error.contains("PNG decode failed"), "{error}");
        assert!(decode_png(b"not a png".to_vec()).is_err());
    }
}
