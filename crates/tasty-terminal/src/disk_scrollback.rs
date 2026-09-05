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
pub const FORMAT_VERSION: u32 = 3;
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
        // 격리축은 **프로세스**다. `surface_id` 공간은 인스턴스마다 독립이고 매 실행
        // 1 부터 재발급되므로(`IdGenerator::next_surface`), 이름에 프로세스 성분이 없으면
        // 같은 프로필 두 벌이 같은 번호의 파일을 서로 truncate 한다. pid 를 이름에 실어
        // 그것을 막는다 — 이 하나가 debug↔release 축도 함께 덮는다(둘은 서로 다른
        // 프로세스라 pid 가 다르다). 같은 형태의 처방은 `prompt_file::path_for` 를 따른다.
        //
        // 하위 디렉터리는 격리가 아니라 **묶음**이다(debug 빌드 산출물을 한자리에 모아
        // 식별을 돕는다). 격리를 지는 것은 파일명의 pid 다.
        let subdir = if cfg!(debug_assertions) {
            "tasty-scrollback-debug"
        } else {
            "tasty-scrollback"
        };
        let dir = std::env::temp_dir().join(subdir);
        std::fs::create_dir_all(&dir)?;
        let file_path = dir.join(format!(
            "surface-{}-{}.scrollback",
            std::process::id(),
            surface_id
        ));
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

    /// Discard all on-disk scrollback, truncating the file back to just the
    /// header. Used by ED3 (`CSI 3J`) erase-scrollback.
    pub fn clear(&mut self) -> std::io::Result<()> {
        let mut f = File::create(&self.file_path)?;
        f.write_all(FILE_MAGIC)?;
        f.write_all(&FORMAT_VERSION.to_le_bytes())?;
        f.flush()?;
        self.disk_line_count = 0;
        self.line_offsets.clear();
        self.file_size = HEADER_LEN;
        Ok(())
    }
}

impl Drop for DiskScrollback {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_file(&self.file_path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::trace!(
                "scrollback temp file {} cleanup failed: {e}",
                self.file_path.display()
            );
        }
    }
}

/// Pack a cell's attributes into the flags byte.
/// bit0=bold, bit1=italic, bit2=underline, bit3=strikethrough, bit4=dim (Intensity::Half).
fn attr_flags(attrs: &CellAttributes) -> u8 {
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
    flags
}

/// Serialize a scrollback line to bytes, mirroring the in-memory compact form
/// (single text buffer + per-cell lengths + RLE attribute runs).
/// Layout:
///   [wrapped:u8]
///   [text_len:u32][text_bytes]
///   [cell_count:u32][cell_len:u16 × cell_count]
///   [run_count:u32] then per run:
///     [run_len:u32][fg_type:u8][fg_data:0-3][bg_type:u8][bg_data:0-3][flags:u8]
fn serialize_line(line: &ScrollbackLine) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(if line.wrapped { 1 } else { 0 });

    // Concatenated cell text.
    let text_bytes = line.text.as_bytes();
    buf.extend_from_slice(&(text_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(text_bytes);

    // Per-cell grapheme byte lengths.
    buf.extend_from_slice(&(line.cell_lens.len() as u32).to_le_bytes());
    for &len in &line.cell_lens {
        buf.extend_from_slice(&len.to_le_bytes());
    }

    // RLE attribute runs.
    buf.extend_from_slice(&(line.attr_runs.len() as u32).to_le_bytes());
    for (run_len, attrs) in &line.attr_runs {
        buf.extend_from_slice(&run_len.to_le_bytes());
        serialize_color(&attrs.foreground(), &mut buf);
        serialize_color(&attrs.background(), &mut buf);
        buf.push(attr_flags(attrs));
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

    // Concatenated cell text.
    if pos + 4 > data.len() {
        return ScrollbackLine::new(Vec::new(), wrapped);
    }
    let text_len =
        u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
    pos += 4;
    if pos + text_len > data.len() {
        return ScrollbackLine::new(Vec::new(), wrapped);
    }
    let text = String::from_utf8_lossy(&data[pos..pos + text_len]).to_string();
    pos += text_len;

    // Per-cell grapheme byte lengths.
    if pos + 4 > data.len() {
        return ScrollbackLine::from_raw_parts(text, Vec::new(), Vec::new(), wrapped);
    }
    let cell_count =
        u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
    pos += 4;
    let mut cell_lens = Vec::with_capacity(cell_count);
    for _ in 0..cell_count {
        if pos + 2 > data.len() {
            break;
        }
        cell_lens.push(u16::from_le_bytes([data[pos], data[pos + 1]]));
        pos += 2;
    }

    // RLE attribute runs.
    let mut attr_runs = Vec::new();
    if pos + 4 <= data.len() {
        let run_count =
            u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        attr_runs.reserve(run_count);
        for _ in 0..run_count {
            if pos + 4 > data.len() {
                break;
            }
            let run_len =
                u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
            pos += 4;

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

            attr_runs.push((run_len, attrs));
        }
    }

    ScrollbackLine::from_raw_parts(text, cell_lens, attr_runs, wrapped)
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
        let cells = out.to_cells();
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].0, "D");
        assert_eq!(cells[0].1.intensity(), Intensity::Half);
        assert!(!out.wrapped);
    }

    #[test]
    fn preserves_intensity_bold() {
        let mut a = CellAttributes::default();
        a.set_intensity(Intensity::Bold);
        let out = round_trip(ScrollbackLine::new(vec![("B".into(), a)], false));
        assert_eq!(out.to_cells()[0].1.intensity(), Intensity::Bold);
    }

    #[test]
    fn preserves_other_attrs_alongside_half() {
        let mut a = CellAttributes::default();
        a.set_intensity(Intensity::Half);
        a.set_italic(true);
        a.set_underline(Underline::Single);
        a.set_strikethrough(true);
        let out = round_trip(ScrollbackLine::new(vec![("X".into(), a)], false));
        let cells = out.to_cells();
        let r = &cells[0].1;
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
        assert_eq!(out.to_cells()[0].0, "W");
    }

    #[test]
    fn preserves_multibyte_unicode_cells() {
        // CJK (3-byte UTF-8), an emoji (4-byte), and an attributed wide char —
        // exercises exact per-cell byte boundaries + attrs across the compact
        // text+lens+RLE form (regression guard against grapheme byte desync).
        let mut bold = CellAttributes::default();
        bold.set_intensity(Intensity::Bold);
        let mut italic = CellAttributes::default();
        italic.set_italic(true);

        let input = ScrollbackLine::new(
            vec![
                ("가".into(), bold.clone()),
                ("나".into(), CellAttributes::default()),
                ("다".into(), CellAttributes::default()),
                ("🦀".into(), italic.clone()),
                ("漢".into(), CellAttributes::default()),
            ],
            true,
        );
        let out = round_trip(input);
        let cells = out.to_cells();

        assert!(out.wrapped);
        assert_eq!(cells.len(), 5);

        // Exact grapheme content + byte length preserved per cell.
        assert_eq!(cells[0].0, "가");
        assert_eq!(cells[0].0.len(), 3);
        assert_eq!(cells[1].0, "나");
        assert_eq!(cells[2].0, "다");
        assert_eq!(cells[3].0, "🦀");
        assert_eq!(cells[3].0.len(), 4);
        assert_eq!(cells[4].0, "漢");
        assert_eq!(cells[4].0.len(), 3);

        // Attributes preserved (incl. RLE runs straddling multibyte cells).
        assert_eq!(cells[0].1.intensity(), Intensity::Bold);
        assert_eq!(cells[1].1.intensity(), Intensity::Normal);
        assert!(!cells[1].1.italic());
        assert!(cells[3].1.italic());
        assert!(!cells[4].1.italic());
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
        assert_eq!(out[0].to_cells()[0].0, "hello");
        assert!(!out[0].wrapped);
        let l1 = out[1].to_cells();
        assert_eq!(l1[0].0, "world");
        assert!(out[1].wrapped);
        assert_eq!(l1[0].1.intensity(), Intensity::Bold);
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
