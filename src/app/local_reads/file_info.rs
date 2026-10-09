//! 탐색기의 미리보기·썸네일·Properties 가 쓰는 파일 읽기. 모두 read worker 에서 실행해 UI 스레드는
//! 파일시스템을 읽지 않는다. 미리보기와 썸네일은 앱 크기 상한을 넘는 파일을 읽지 않는다.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::SystemTime;

/// 미리보기·썸네일이 내용을 읽는 파일 크기 상한. 넘으면 "Too large" 상태와 글리프만 보인다.
pub(crate) const PREVIEW_MAX_BYTES: u64 = 1024 * 1024;
/// 텍스트 미리보기가 보이는 앞부분의 크기.
pub(crate) const PREVIEW_TEXT_BYTES: usize = 64 * 1024;
/// 썸네일 디코딩 결과의 긴 변(px). 40 슬롯을 2배 밀도 화면에서도 흐리지 않게 채운다.
const THUMB_DECODE_PX: u32 = 80;

/// 미리보기 패널 본문.
pub(crate) enum PreviewData {
    /// 앞 64 KB 의 UTF-8 텍스트.
    Text(String),
    /// 디코딩한 그림.
    Image(egui::ColorImage),
    /// 텍스트·그림이 아니거나 폴더다.
    Unsupported,
    /// 상한을 넘었다.
    TooLarge,
}

/// 확장자로 그림 디코더가 읽을 수 있는 형식인지 정한다. 빌드에 켠 `image` 크레이트 기능과 같다.
pub(crate) fn decodable_image_ext(ext: &str) -> bool {
    matches!(
        ext,
        "png" | "jpg" | "jpeg" | "bmp" | "webp" | "ico" | "tif" | "tiff"
    )
}

pub(super) fn read_preview(path: &Path) -> io::Result<PreviewData> {
    let meta = std::fs::metadata(path)?;
    if meta.is_dir() {
        return Ok(PreviewData::Unsupported);
    }
    let ext = extension(path);
    if decodable_image_ext(&ext) {
        if meta.len() > PREVIEW_MAX_BYTES {
            return Ok(PreviewData::TooLarge);
        }
        return decode(path, None).map(PreviewData::Image);
    }
    let mut head = Vec::with_capacity(PREVIEW_TEXT_BYTES.min(meta.len() as usize));
    std::fs::File::open(path)?
        .take(PREVIEW_TEXT_BYTES as u64)
        .read_to_end(&mut head)?;
    let Some(text) = utf8_text(&head) else {
        return Ok(PreviewData::Unsupported);
    };
    if meta.len() > PREVIEW_MAX_BYTES {
        return Ok(PreviewData::TooLarge);
    }
    Ok(PreviewData::Text(text))
}

/// NUL 이 없고 UTF-8 로 읽히면 텍스트로 본다. 앞부분만 읽었으므로 끝에서 잘린 한 글자는 버린다.
fn utf8_text(bytes: &[u8]) -> Option<String> {
    if bytes.contains(&0) {
        return None;
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => Some(s.to_owned()),
        Err(e) if e.error_len().is_none() => {
            Some(String::from_utf8_lossy(&bytes[..e.valid_up_to()]).into_owned())
        }
        Err(_) => None,
    }
}

pub(super) fn read_thumbnail(path: &Path) -> io::Result<egui::ColorImage> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > PREVIEW_MAX_BYTES {
        return Err(io::Error::other("over the preview size limit"));
    }
    decode(path, Some(THUMB_DECODE_PX))
}

fn decode(path: &Path, fit: Option<u32>) -> io::Result<egui::ColorImage> {
    let img = image::ImageReader::open(path)?
        .with_guessed_format()?
        .decode()
        .map_err(io::Error::other)?;
    let img = match fit {
        Some(px) if img.width() > px || img.height() > px => img.thumbnail(px, px),
        _ => img,
    };
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        size,
        rgba.as_raw(),
    ))
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default()
}

/// Properties 의 항목 종류.
#[derive(Clone, Debug)]
pub(crate) enum ItemKind {
    File,
    Folder,
    /// 대상 경로. 대상이 없어도 링크 자신은 있다.
    Link(PathBuf),
}

/// 항목 하나의 정보.
#[derive(Clone, Debug)]
pub(crate) struct ItemFacts {
    pub(crate) kind: ItemKind,
    /// 파일·링크의 바이트 길이. 폴더는 배경 계산 결과를 쓴다.
    pub(crate) size: u64,
    pub(crate) modified: Option<SystemTime>,
    pub(crate) created: Option<SystemTime>,
    /// Unix 의 `rwxr-xr-x` 형태. 다른 OS 에서는 없다.
    pub(crate) mode: Option<String>,
    pub(crate) read_only: bool,
}

/// Properties 가 연 항목들의 정보. 여러 개면 파일·폴더 수와 파일 크기 합을 함께 준다.
#[derive(Clone, Debug)]
pub(crate) struct PropertiesFacts {
    pub(crate) items: Vec<ItemFacts>,
}

/// 폴더 크기의 배경 계산 진행. 폴더가 없으면 시작하자마자 끝난다.
#[derive(Default, Debug)]
pub(crate) struct FolderCount {
    pub(crate) items: AtomicU64,
    pub(crate) bytes: AtomicU64,
    pub(crate) done: AtomicBool,
}

/// 정보를 먼저 보내고, 폴더가 있으면 같은 worker 에서 크기를 센다. 취소되면 세기를 멈춘다.
pub(super) fn read_properties(
    paths: &[PathBuf],
    count: &Arc<FolderCount>,
    cancel: &AtomicBool,
    deliver: impl FnOnce(io::Result<PropertiesFacts>) -> bool,
) -> bool {
    let facts: io::Result<Vec<ItemFacts>> = paths.iter().map(|p| item_facts(p)).collect();
    let folders: Vec<PathBuf> = match &facts {
        Ok(items) => paths
            .iter()
            .zip(items)
            .filter(|(_, f)| matches!(f.kind, ItemKind::Folder))
            .map(|(p, _)| p.clone())
            .collect(),
        Err(_) => Vec::new(),
    };
    let delivered = deliver(facts.map(|items| PropertiesFacts { items }));
    if delivered {
        for folder in folders {
            count_folder(&folder, count, cancel);
        }
    }
    count.done.store(true, Ordering::Release);
    delivered
}

fn item_facts(path: &Path) -> io::Result<ItemFacts> {
    let own = std::fs::symlink_metadata(path)?;
    let (kind, meta) = if own.file_type().is_symlink() {
        let target = std::fs::read_link(path)?;
        (ItemKind::Link(target), own)
    } else if own.is_dir() {
        (ItemKind::Folder, own)
    } else {
        (ItemKind::File, own)
    };
    Ok(ItemFacts {
        size: if matches!(kind, ItemKind::Folder) {
            0
        } else {
            meta.len()
        },
        kind,
        modified: meta.modified().ok(),
        created: meta.created().ok(),
        mode: mode_string(&meta),
        read_only: meta.permissions().readonly(),
    })
}

#[cfg(unix)]
fn mode_string(meta: &std::fs::Metadata) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode();
    let bits = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];
    Some(
        bits.iter()
            .map(|(bit, c)| if mode & bit != 0 { *c } else { '-' })
            .collect(),
    )
}

#[cfg(not(unix))]
fn mode_string(_meta: &std::fs::Metadata) -> Option<String> {
    None
}

/// 링크는 따라가지 않는다. 읽지 못한 하위 폴더는 건너뛰고 센 만큼만 보인다.
fn count_folder(root: &Path, count: &FolderCount, cancel: &AtomicBool) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::debug!(%error, dir = %dir.display(), "properties: skipped a folder");
                continue;
            }
        };
        for entry in entries.flatten() {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            count.items.fetch_add(1, Ordering::Relaxed);
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                count.bytes.fetch_add(meta.len(), Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_utf8_without_nul_and_a_cut_last_char_is_dropped() {
        assert_eq!(utf8_text(b"hello").as_deref(), Some("hello"));
        assert!(utf8_text(b"a\0b").is_none());
        assert!(utf8_text(&[0xff, 0xfe, b'a']).is_none());
        // "가" = EA B0 80. 앞 두 바이트만 읽힌 경우.
        assert_eq!(utf8_text(&[b'x', 0xEA, 0xB0]).as_deref(), Some("x"));
    }

    #[test]
    fn preview_reads_text_and_refuses_a_file_over_the_limit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let small = dir.path().join("notes.md");
        std::fs::write(&small, "# Notes\n").expect("write");
        assert!(matches!(read_preview(&small), Ok(PreviewData::Text(t)) if t == "# Notes\n"));

        let big = dir.path().join("server.log");
        std::fs::write(&big, vec![b'a'; PREVIEW_MAX_BYTES as usize + 1]).expect("write");
        assert!(matches!(read_preview(&big), Ok(PreviewData::TooLarge)));

        let bin = dir.path().join("blob.bin");
        std::fs::write(&bin, [0u8, 1, 2]).expect("write");
        assert!(matches!(read_preview(&bin), Ok(PreviewData::Unsupported)));
        assert!(matches!(
            read_preview(dir.path()),
            Ok(PreviewData::Unsupported)
        ));
    }

    #[test]
    fn properties_count_a_folder_and_stop_when_cancelled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).expect("mkdir");
        std::fs::write(sub.join("a.txt"), "abc").expect("write");
        std::fs::write(dir.path().join("b.txt"), "de").expect("write");

        let count = Arc::new(FolderCount::default());
        let mut got = None;
        let delivered = read_properties(
            &[dir.path().to_path_buf()],
            &count,
            &AtomicBool::new(false),
            |facts| {
                got = Some(facts);
                true
            },
        );
        assert!(delivered);
        let facts = got.expect("delivered").expect("readable");
        assert!(matches!(facts.items[0].kind, ItemKind::Folder));
        assert_eq!(count.items.load(Ordering::Relaxed), 3);
        assert_eq!(count.bytes.load(Ordering::Relaxed), 5);
        assert!(count.done.load(Ordering::Relaxed));

        let cancelled = Arc::new(FolderCount::default());
        read_properties(
            &[dir.path().to_path_buf()],
            &cancelled,
            &AtomicBool::new(true),
            |_| true,
        );
        assert_eq!(cancelled.items.load(Ordering::Relaxed), 0);
        assert!(cancelled.done.load(Ordering::Relaxed));
    }
}
