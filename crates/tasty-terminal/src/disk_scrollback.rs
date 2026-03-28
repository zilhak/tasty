use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use termwiz::cell::CellAttributes;
use termwiz::color::ColorAttribute;

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
        // Truncate any existing file
        File::create(&file_path)?;
        Ok(Self {
            file_path,
            disk_line_count: 0,
            line_offsets: Vec::new(),
            file_size: 0,
        })
    }

    /// Write lines to disk. Returns number of lines written.
    pub fn push_lines(&mut self, lines: &[Vec<(String, CellAttributes)>]) -> std::io::Result<usize> {
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
    pub fn read_line(&self, index: usize) -> std::io::Result<Option<Vec<(String, CellAttributes)>>> {
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
        let _ = std::fs::remove_file(&self.file_path);
    }
}

/// Serialize a scrollback line to bytes.
/// Format per cell: [text_len:u16][text_bytes][fg_type:u8][fg_data:0-3 bytes][bg_type:u8][bg_data:0-3 bytes][flags:u8]
fn serialize_line(line: &[(String, CellAttributes)]) -> Vec<u8> {
    let mut buf = Vec::new();
    let cell_count = line.len() as u32;
    buf.extend_from_slice(&cell_count.to_le_bytes());

    for (text, attrs) in line {
        // Text
        let text_bytes = text.as_bytes();
        let text_len = text_bytes.len() as u16;
        buf.extend_from_slice(&text_len.to_le_bytes());
        buf.extend_from_slice(text_bytes);

        // Foreground color
        serialize_color(&attrs.foreground(), &mut buf);
        // Background color
        serialize_color(&attrs.background(), &mut buf);

        // Flags: bold, italic, underline, strikethrough
        let mut flags: u8 = 0;
        if attrs.intensity() == termwiz::cell::Intensity::Bold { flags |= 1; }
        if attrs.italic() { flags |= 2; }
        if attrs.underline() != termwiz::cell::Underline::None { flags |= 4; }
        if attrs.strikethrough() { flags |= 8; }
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

fn deserialize_line(data: &[u8]) -> Vec<(String, CellAttributes)> {
    let mut pos = 0;
    if data.len() < 4 { return Vec::new(); }

    let cell_count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    pos += 4;

    let mut line = Vec::with_capacity(cell_count);

    for _ in 0..cell_count {
        if pos + 2 > data.len() { break; }
        let text_len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;

        if pos + text_len > data.len() { break; }
        let text = String::from_utf8_lossy(&data[pos..pos + text_len]).to_string();
        pos += text_len;

        let (fg, advance) = deserialize_color(&data[pos..]);
        pos += advance;
        let (bg, advance) = deserialize_color(&data[pos..]);
        pos += advance;

        if pos >= data.len() { break; }
        let flags = data[pos];
        pos += 1;

        let mut attrs = CellAttributes::default();
        attrs.set_foreground(fg);
        attrs.set_background(bg);
        if flags & 1 != 0 { attrs.set_intensity(termwiz::cell::Intensity::Bold); }
        if flags & 2 != 0 { attrs.set_italic(true); }
        if flags & 4 != 0 { attrs.set_underline(termwiz::cell::Underline::Single); }
        if flags & 8 != 0 { attrs.set_strikethrough(true); }

        line.push((text, attrs));
    }
    line
}

fn deserialize_color(data: &[u8]) -> (ColorAttribute, usize) {
    if data.is_empty() { return (ColorAttribute::Default, 0); }
    match data[0] {
        0 => (ColorAttribute::Default, 1),
        1 => {
            if data.len() < 2 { return (ColorAttribute::Default, 1); }
            (ColorAttribute::PaletteIndex(data[1]), 2)
        }
        2 => {
            if data.len() < 4 { return (ColorAttribute::Default, 1); }
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
