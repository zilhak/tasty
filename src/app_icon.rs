//! Embedded application icon for runtime use (window icon, tray icon).

/// 256x256 PNG icon bytes (embedded at compile time).
pub static ICON_PNG_256: &[u8] = include_bytes!("../assets/icons/icon_256.png");

/// 32x32 PNG icon bytes for small contexts (tray icon, taskbar).
#[cfg(windows)]
pub static ICON_PNG_32: &[u8] = include_bytes!("../assets/icons/icon_32.png");

/// Decode a PNG byte slice into RGBA pixels and dimensions.
fn decode_png(png_bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());

    // Ensure RGBA8 format
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }

    Some((buf, info.width, info.height))
}

/// Create a winit window icon from the embedded 256x256 PNG.
pub fn winit_window_icon() -> Option<winit::window::Icon> {
    let (rgba, w, h) = decode_png(ICON_PNG_256)?;
    winit::window::Icon::from_rgba(rgba, w, h).ok()
}

/// Create a tray-icon Icon from the embedded 32x32 PNG.
#[cfg(windows)]
pub fn tray_icon() -> Option<tray_icon::Icon> {
    let (rgba, w, h) = decode_png(ICON_PNG_32)?;
    tray_icon::Icon::from_rgba(rgba, w, h).ok()
}
