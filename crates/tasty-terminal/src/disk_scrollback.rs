use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use termwiz::cell::CellAttributes;
use termwiz::color::ColorAttribute;

use crate::scrollback::ScrollbackLine;

/// Magic + version stamped at the head of every disk scrollback file. Bumped
/// whenever the on-disk layout changes; old files are simply truncated and
/// re-created (no backwards compatibility — Tasty is pre-1.0).
pub const FILE_MAGIC: &[u8; 4] = b"TSSB";
pub const FORMAT_VERSION: u32 = 2;
const HEADER_LEN: u64 = 8; // 4-byte magic + 4-byte version

/// Serialize a batch of scrollback lines into a self-contained byte blob.
/// Layout: `FILE_MAGIC | FORMAT_VERSION:u32 | { len:u32 | line_bytes }*`.
/// Pairs with [`deserialize_lines`]. Used by the host crate's persistence
/// layer (`~/.tasty/scrollback/*.bin`).
pub fn serialize_lines(lines: &[ScrollbackLine]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(HEADER_LEN as usize + lines.len() * 64);
    buf.extend_from_slice(FILE_MAGIC);
    buf.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    for line in lines {
        let bytes = serialize_line(line);
        let len = bytes.len() as u32;
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(&bytes);
    }
    buf
}

/// Parse a byte blob produced by [`serialize_lines`]. Returns `None` if the
/// header doesn't match or the version is newer than this build supports.
pub fn deserialize_lines(data: &[u8]) -> Option<Vec<ScrollbackLine>> {
    if data.len() < HEADER_LEN as usize {
        return None;
    }
    if &data[0..4] != FILE_MAGIC {
        return None;
    }
    let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if version != FORMAT_VERSION {
        return None;
    }
    let mut pos = HEADER_LEN as usize;
    let mut lines = Vec::new();
    while pos + 4 <= data.len() {
        let len =
            u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        if pos + len > data.len() {
            break;
        }
        lines.push(deserialize_line(&data[pos..pos + len]));
        pos += len;
    }
    Some(lines)
}

/// Disk-backed scrollback storage. Older lines are written to a temp file,
/// while recent lines remain in memory for fast access.
pub struct DiskScrollback {
    file_path: PathBuf,
    /// Number of lines stored on disk.
    disk_line_count: usize,
    /// Byte offsets of each line in the file (for random access).
    line_offsets: Vec<u64>,
    /// File size.
    file_size: u64,
}

impl DiskScrollback {
    pub fn new(surface_id: u32) -> std::io::Result<Self> {
        let dir = std::env::temp_dir().join("tasty-scrollback");
        std::fs::create_dir_all(&dir)?;
        let file_path = dir.join(format!("surface-{}.scrollback", surface_id));
        // Truncate any existing file and write the header.
        let mut f = File::create(&file_path)?;
        f.write_all(FILE_MAGIC)?;
        f.write_all(&FORMAT_VERSION.to_le_bytes())?;
        f.flush()?;
        Ok(Self {
            file_path,
            disk_line_count: 0,
            line_offsets: Vec::new(),
            file_size: HEADER_LEN,
        })
    }

    /// Write lines to disk. Returns number of lines written.
    pub fn push_lines(&mut self, lines: &[ScrollbackLine]) -> std::io::Result<usize> {
        let file = OpenOptions::new().append(true).open(&self.file_path)?;
        let mut writer = BufWriter::new(file);

        for line in lines {
            self.line_offsets.push(self.file_size);
            let bytes = serialize_line(line);
            let len = bytes.len() as u32;
            writer.write_all(&len.to_le_bytes())?;
            writer.write_all(&bytes)?;
            self.file_size += 4 + bytes.len() as u64;
            self.disk_line_count += 1;
        }

        writer.flush()?;
        Ok(lines.len())
    }

    /// Read a line from disk by index.
    pub fn read_line(&self, index: usize) -> std::io::Result<Option<ScrollbackLine>> {
        if index >= self.disk_line_count {
            return Ok(None);
        }

        let file = File::open(&self.file_path)?;
        let mut reader = BufReader::new(file);
        let offset = self.line_offsets[index];
        reader.seek(SeekFrom::Start(offset))?;

        let mut len_buf = [0u8; 4];
        reader.read_exact(&mut len_buf)?;
        let len = u32::from_le_bytes(len_buf) as usize;

        let mut data = vec![0u8; len];
        reader.read_exact(&mut data)?;

        Ok(Some(deserialize_line(&data)))
    }

    pub fn line_count(&self) -> usize {
        self.disk_line_count
    }
}

impl Drop for DiskScrollback {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_file(&self.file_path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::trace!(
                    "scrollback temp file {} cleanup failed: {e}",
                    self.file_path.display()
                );
            }
        }
    }
}

/// Serialize a scrollback line to bytes.
/// Layout: [wrapped:u8][cell_count:u32] then per cell:
///   [text_len:u16][text_bytes][fg_type:u8][fg_data:0-3][bg_type:u8][bg_data:0-3][flags:u8]
fn serialize_line(line: &ScrollbackLine) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(if line.wrapped { 1 } else { 0 });
    let cell_count = line.cells.len() as u32;
    buf.extend_from_slice(&cell_count.to_le_bytes());

    for (text, attrs) in &line.cells {
        // Text
        let text_bytes = text.as_bytes();
        let text_len = text_bytes.len() as u16;
        buf.extend_from_slice(&text_len.to_le_bytes());
        buf.extend_from_slice(text_bytes);

        // Foreground color
        serialize_color(&attrs.foreground(), &mut buf);
        // Background color
        serialize_color(&attrs.background(), &mut buf);

        // Flags: bit0=bold, bit1=italic, bit2=underline, bit3=strikethrough, bit4=dim (Intensity::Half).
        let mut flags: u8 = 0;
        match attrs.intensity() {
            termwiz::cell::Intensity::Bold => flags |= 1,
            termwiz::cell::Intensity::Half => flags |= 16,
            termwiz::cell::Intensity::Normal => {}
        }
        if attrs.italic() {
            flags |= 2;
        }
        if attrs.underline() != termwiz::cell::Underline::None {
            flags |= 4;
        }
        if attrs.strikethrough() {
            flags |= 8;
        }
        buf.push(flags);
    }
    buf
}

fn serialize_color(color: &ColorAttribute, buf: &mut Vec<u8>) {
    match color {
        ColorAttribute::Default => buf.push(0),
        ColorAttribute::PaletteIndex(idx) => {
            buf.push(1);
            buf.push(*idx);
        }
        ColorAttribute::TrueColorWithDefaultFallback(c)
        | ColorAttribute::TrueColorWithPaletteFallback(c, _) => {
            buf.push(2);
            let (r, g, b, _) = c.to_tuple_rgba();
            buf.push((r * 255.0) as u8);
            buf.push((g * 255.0) as u8);
            buf.push((b * 255.0) as u8);
        }
    }
}

fn deserialize_line(data: &[u8]) -> ScrollbackLine {
    let mut pos = 0;
    if data.len() < 5 {
        return ScrollbackLine::new(Vec::new(), false);
    }

    let wrapped = data[0] != 0;
    pos += 1;

    let cell_count = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
        as usize;
    pos += 4;

    let mut cells = Vec::with_capacity(cell_count);

    for _ in 0..cell_count {
        if pos + 2 > data.len() {
            break;
        }
        let text_len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;

        if pos + text_len > data.len() {
            break;
        }
        let text = String::from_utf8_lossy(&data[pos..pos + text_len]).to_string();
        pos += text_len;

        let (fg, advance) = deserialize_color(&data[pos..]);
        pos += advance;
        let (bg, advance) = deserialize_color(&data[pos..]);
        pos += advance;

        if pos >= data.len() {
            break;
        }
        let flags = data[pos];
        pos += 1;

        let mut attrs = CellAttributes::default();
        attrs.set_foreground(fg);
        attrs.set_background(bg);
        if flags & 1 != 0 {
            attrs.set_intensity(termwiz::cell::Intensity::Bold);
        } else if flags & 16 != 0 {
            attrs.set_intensity(termwiz::cell::Intensity::Half);
        }
        if flags & 2 != 0 {
            attrs.set_italic(true);
        }
        if flags & 4 != 0 {
            attrs.set_underline(termwiz::cell::Underline::Single);
        }
        if flags & 8 != 0 {
            attrs.set_strikethrough(true);
        }

        cells.push((text, attrs));
    }
    ScrollbackLine::new(cells, wrapped)
}

fn deserialize_color(data: &[u8]) -> (ColorAttribute, usize) {
    if data.is_empty() {
        return (ColorAttribute::Default, 0);
    }
    match data[0] {
        0 => (ColorAttribute::Default, 1),
        1 => {
            if data.len() < 2 {
                return (ColorAttribute::Default, 1);
            }
            (ColorAttribute::PaletteIndex(data[1]), 2)
        }
        2 => {
            if data.len() < 4 {
                return (ColorAttribute::Default, 1);
            }
            let c = termwiz::color::SrgbaTuple(
                data[1] as f32 / 255.0,
                data[2] as f32 / 255.0,
                data[3] as f32 / 255.0,
                1.0,
            );
            (ColorAttribute::TrueColorWithDefaultFallback(c), 4)
        }
        _ => (ColorAttribute::Default, 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termwiz::cell::{Intensity, Underline};

    fn round_trip(input: ScrollbackLine) -> ScrollbackLine {
        let bytes = serialize_line(&input);
        deserialize_line(&bytes)
    }

    #[test]
    fn preserves_intensity_half() {
        let mut a = CellAttributes::default();
        a.set_intensity(Intensity::Half);
        let out = round_trip(ScrollbackLine::new(vec![("D".into(), a)], false));
        assert_eq!(out.cells.len(), 1);
        assert_eq!(out.cells[0].0, "D");
        assert_eq!(out.cells[0].1.intensity(), Intensity::Half);
        assert!(!out.wrapped);
    }

    #[test]
    fn preserves_intensity_bold() {
        let mut a = CellAttributes::default();
        a.set_intensity(Intensity::Bold);
        let out = round_trip(ScrollbackLine::new(vec![("B".into(), a)], false));
        assert_eq!(out.cells[0].1.intensity(), Intensity::Bold);
    }

    #[test]
    fn preserves_other_attrs_alongside_half() {
        let mut a = CellAttributes::default();
        a.set_intensity(Intensity::Half);
        a.set_italic(true);
        a.set_underline(Underline::Single);
        a.set_strikethrough(true);
        let out = round_trip(ScrollbackLine::new(vec![("X".into(), a)], false));
        let r = &out.cells[0].1;
        assert_eq!(r.intensity(), Intensity::Half);
        assert!(r.italic());
        assert_ne!(r.underline(), Underline::None);
        assert!(r.strikethrough());
    }

    #[test]
    fn preserves_wrapped_flag() {
        let a = CellAttributes::default();
        let out = round_trip(ScrollbackLine::new(vec![("W".into(), a)], true));
        assert!(out.wrapped);
        assert_eq!(out.cells[0].0, "W");
    }

    #[test]
    fn batch_serialize_round_trip() {
        let mut bold = CellAttributes::default();
        bold.set_intensity(Intensity::Bold);
        let lines = vec![
            ScrollbackLine::new(vec![("hello".into(), CellAttributes::default())], false),
            ScrollbackLine::new(vec![("world".into(), bold)], true),
        ];
        let bytes = serialize_lines(&lines);
        let out = deserialize_lines(&bytes).expect("deserialize");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].cells[0].0, "hello");
        assert!(!out[0].wrapped);
        assert_eq!(out[1].cells[0].0, "world");
        assert!(out[1].wrapped);
        assert_eq!(out[1].cells[0].1.intensity(), Intensity::Bold);
    }

    #[test]
    fn deserialize_rejects_bad_header() {
        assert!(deserialize_lines(&[]).is_none());
        assert!(deserialize_lines(b"XXXX\x01\x00\x00\x00").is_none());
    }

    #[test]
    fn empty_batch_round_trip() {
        let bytes = serialize_lines(&[]);
        let out = deserialize_lines(&bytes).expect("deserialize");
        assert!(out.is_empty());
    }
}
