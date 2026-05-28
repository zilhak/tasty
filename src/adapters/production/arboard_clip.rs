//! ArboardClipboard — `arboard` crate 기반 production ClipboardSystem.

use crate::ports::clipboard::{ClipboardImage, ClipboardSystem};

#[derive(Debug, Default)]
pub struct ArboardClipboard;

impl ClipboardSystem for ArboardClipboard {
    fn read_text(&self) -> anyhow::Result<String> {
        let mut cb = arboard::Clipboard::new()?;
        Ok(cb.get_text()?)
    }

    fn write_text(&self, text: &str) -> anyhow::Result<()> {
        let mut cb = arboard::Clipboard::new()?;
        cb.set_text(text)?;
        Ok(())
    }

    fn read_image(&self) -> anyhow::Result<ClipboardImage> {
        let mut cb = arboard::Clipboard::new()?;
        let img = cb.get_image()?;
        Ok(ClipboardImage {
            width: img.width as u32,
            height: img.height as u32,
            pixels: img.bytes.into_owned(),
        })
    }

    fn write_image(&self, image: &ClipboardImage) -> anyhow::Result<()> {
        let mut cb = arboard::Clipboard::new()?;
        let img = arboard::ImageData {
            width: image.width as usize,
            height: image.height as usize,
            bytes: std::borrow::Cow::Borrowed(&image.pixels),
        };
        cb.set_image(img)?;
        Ok(())
    }
}
