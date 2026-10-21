//! 탐색기의 미리보기·썸네일·Properties 가 쓰는 파일 읽기. 모두 read worker 에서 실행해 UI 스레드는
//! 파일시스템을 읽지 않는다. 미리보기와 썸네일은 앱 크기 상한을 넘는 파일을 읽지 않는다.
//! 정규 파일만 연다. FIFO·장치 파일은 열기가 끝나지 않거나 끝없이 읽혀 worker 를 붙잡는다.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::SystemTime;

use crate::app::file_jobs::job::leftover::FileId;
use crate::app::file_jobs::ops::drop_nested;

/// 미리보기·썸네일이 내용을 읽는 파일 크기 상한. 넘으면 "Too large" 상태와 글리프만 보인다.
pub(crate) const PREVIEW_MAX_BYTES: u64 = 1024 * 1024;
/// 텍스트 미리보기가 보이는 앞부분의 크기.
pub(crate) const PREVIEW_TEXT_BYTES: usize = 64 * 1024;
/// 썸네일 디코딩 결과의 긴 변(px). 40 슬롯을 2배 밀도 화면에서도 흐리지 않게 채운다.
const THUMB_DECODE_PX: u32 = 80;
/// 미리보기 텍스처의 긴 변 상한(px). egui 는 `max_texture_side` 를 넘는 텍스처에서 패닉하고,
/// wgpu 가 보장하는 최소값이 2048 이라 이 값으로 줄여 보낸다.
pub(crate) const PREVIEW_TEXTURE_SIDE: u32 = 2048;
/// 디코딩 전에 거절하는 그림 한 변의 픽셀 수. 바이트 상한 안에 든 거대한 그림을 펼치지 않는다.
pub(crate) const MAX_IMAGE_SIDE: u32 = 16384;
/// 디코딩이 한 번에 잡을 수 있는 메모리.
pub(crate) const MAX_DECODE_ALLOC: u64 = 256 * 1024 * 1024;

/// 미리보기·썸네일을 보이지 않는 이유가 된 상한.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TooLarge {
    /// 파일 크기가 `PREVIEW_MAX_BYTES` 를 넘는다.
    Bytes,
    /// 그림의 한 변이나 디코딩 메모리가 `MAX_IMAGE_SIDE`·`MAX_DECODE_ALLOC` 을 넘는다.
    /// 머리글에서 읽은 원본 픽셀 크기를 함께 둔다. 머리글도 읽지 못했으면 `None` 이다.
    Pixels(Option<[u32; 2]>),
}

/// 미리보기 패널 본문.
pub(crate) enum PreviewData {
    /// 앞 64 KB 의 UTF-8 텍스트.
    Text(String),
    /// 디코딩한 그림. 텍스처 상한에 맞춰 줄였을 수 있고, `size` 는 원본 픽셀 크기다.
    Image {
        image: egui::ColorImage,
        size: [usize; 2],
    },
    /// 텍스트·그림이 아니거나 폴더·특수 파일이다.
    Unsupported,
    /// 상한을 넘었다.
    TooLarge(TooLarge),
}

/// 확장자로 그림 디코더가 읽을 수 있는 형식인지 정한다. 빌드에 켠 `image` 크레이트 기능과 같다.
pub(crate) fn decodable_image_ext(ext: &str) -> bool {
    matches!(
        ext,
        "png" | "jpg" | "jpeg" | "bmp" | "webp" | "ico" | "tif" | "tiff"
    )
}

/// `fit_width` 는 미리보기 패널이 그림을 보일 수 있는 최대 폭(물리 px)이다. 그림을 그 폭과 텍스처 상한에
/// 맞춰 worker 에서 Triangle 필터로 줄인다. egui-wgpu 텍스처에는 밉맵이 없어, GPU 가 크게 줄이면 앨리어싱이 생긴다.
pub(super) fn read_preview(path: &Path, fit_width: u32) -> io::Result<PreviewData> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Ok(PreviewData::Unsupported);
    }
    let ext = extension(path);
    if decodable_image_ext(&ext) {
        if meta.len() > PREVIEW_MAX_BYTES {
            return Ok(PreviewData::TooLarge(TooLarge::Bytes));
        }
        let fit = [
            fit_width.clamp(1, PREVIEW_TEXTURE_SIDE),
            PREVIEW_TEXTURE_SIDE,
        ];
        return match decode(path, fit, Shrink::Triangle) {
            Ok((image, size)) => Ok(PreviewData::Image { image, size }),
            Err(Decode::OverLimits) => Ok(PreviewData::TooLarge(TooLarge::Pixels(
                header_dimensions(path),
            ))),
            Err(Decode::Io(error)) => Err(error),
        };
    }
    let mut head = Vec::with_capacity(PREVIEW_TEXT_BYTES.min(meta.len() as usize));
    std::fs::File::open(path)?
        .take(PREVIEW_TEXT_BYTES as u64)
        .read_to_end(&mut head)?;
    let Some(text) = utf8_text(&head) else {
        return Ok(PreviewData::Unsupported);
    };
    if meta.len() > PREVIEW_MAX_BYTES {
        return Ok(PreviewData::TooLarge(TooLarge::Bytes));
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
    if !meta.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    if meta.len() > PREVIEW_MAX_BYTES {
        return Err(io::Error::other("over the preview size limit"));
    }
    match decode(path, [THUMB_DECODE_PX; 2], Shrink::Area) {
        Ok((image, _)) => Ok(image),
        Err(Decode::OverLimits) => Err(io::Error::other("over the image pixel limits")),
        Err(Decode::Io(error)) => Err(error),
    }
}

/// 디코딩하지 않고 머리글만 읽어 픽셀 크기를 얻는다. 상한을 넘은 그림의 실제 크기를 보이는 데 쓴다.
fn header_dimensions(path: &Path) -> Option<[u32; 2]> {
    let reader = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    reader.into_dimensions().ok().map(|(w, h)| [w, h])
}

enum Decode {
    /// `MAX_IMAGE_SIDE`·`MAX_DECODE_ALLOC` 을 넘어 펼치지 않았다.
    OverLimits,
    Io(io::Error),
}

impl From<io::Error> for Decode {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// 그림을 줄이는 방법.
#[derive(Clone, Copy)]
enum Shrink {
    /// 면적 평균(`thumbnail`). 가장 싸다. 많이 만드는 썸네일에 쓴다.
    Area,
    /// Triangle 필터. 면적 평균보다 약 3배 느리지만 줄인 결과가 Lanczos 에 더 가깝다.
    Triangle,
}

/// 상한 안의 그림만 펼치고, `fit`(폭, 높이) 상자를 넘으면 비율을 지켜 줄인다. 키우지 않는다.
/// 원본 픽셀 크기를 함께 돌려준다.
fn decode(
    path: &Path,
    fit: [u32; 2],
    shrink: Shrink,
) -> Result<(egui::ColorImage, [usize; 2]), Decode> {
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let img = reader.decode().map_err(|error| match error {
        image::ImageError::Limits(_) => Decode::OverLimits,
        other => Decode::Io(io::Error::other(other)),
    })?;
    let size = [img.width() as usize, img.height() as usize];
    let img = if img.width() > fit[0] || img.height() > fit[1] {
        match shrink {
            Shrink::Area => img.thumbnail(fit[0], fit[1]),
            Shrink::Triangle => img.resize(fit[0], fit[1], image::imageops::FilterType::Triangle),
        }
    } else {
        img
    };
    let rgba = img.to_rgba8();
    let shown = [rgba.width() as usize, rgba.height() as usize];
    Ok((
        egui::ColorImage::from_rgba_unmultiplied(shown, rgba.as_raw()),
        size,
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
    Link {
        target: PathBuf,
        state: LinkTarget,
    },
}

/// The target is resolved by the filesystem, including relative links and link chains.
#[derive(Clone, Debug)]
pub(crate) enum LinkTarget {
    Readable {
        path: PathBuf,
        is_dir: bool,
        size: u64,
        modified: Option<SystemTime>,
    },
    Missing,
}

/// 항목 하나의 정보.
#[derive(Clone, Debug)]
pub(crate) struct ItemFacts {
    pub(crate) path: PathBuf,
    pub(crate) kind: ItemKind,
    /// 일반 파일의 바이트 길이. 폴더·링크는 별도 집계·대상 정보를 쓰므로 0이다.
    pub(crate) size: u64,
    pub(crate) modified: Option<SystemTime>,
    pub(crate) created: Option<SystemTime>,
    /// Unix 의 `rwxr-xr-x` 형태. 다른 OS 에서는 없다.
    pub(crate) mode: Option<String>,
    pub(crate) read_only: bool,
    /// 하드링크가 둘 이상인 일반 파일의 식별 값. 합계에서 같은 파일을 한 번만 센다.
    pub(crate) hardlink: Option<FileId>,
}

/// Properties 가 연 항목들의 정보. 읽지 못한 항목은 `failed` 에 남기고 읽은 항목만 `items` 에 둔다.
#[derive(Clone, Debug)]
pub(crate) struct PropertiesFacts {
    pub(crate) items: Vec<ItemFacts>,
    /// 고른 파일의 크기 합. 고른 폴더 안에 든 항목과 이미 센 하드링크는 빼고 센다.
    pub(crate) file_bytes: u64,
    /// metadata 를 읽지 못한 고른 항목과 그 원인. 폴더 집계 중 하위 항목을 건너뛴 것(`FolderCount::partial`)과 다르다.
    pub(crate) failed: Vec<(PathBuf, io::ErrorKind)>,
}

/// 링크 수가 둘 이상인 일반 파일의 식별자. 파일 작업이 같은 파일을 가리는 [`FileId`] 를 쓴다.
/// Unix 는 이미 읽은 `meta` 의 (장치, inode)·링크 수라 조회가 늘지 않는다. Windows 는 파일마다 한 번 열어
/// 볼륨 일련번호·파일 인덱스·링크 수를 읽는다(내용 읽기 권한 불요, 권한 상승 없음).
#[cfg(any(unix, windows))]
fn hardlink_id(path: &Path, meta: &std::fs::Metadata) -> io::Result<Option<FileId>> {
    if !meta.is_file() {
        return Ok(None);
    }
    let (id, links) = FileId::read_links(path, meta)?;
    Ok((links > 1).then_some(id))
}

#[cfg(not(any(unix, windows)))]
fn hardlink_id(_path: &Path, _meta: &std::fs::Metadata) -> io::Result<Option<FileId>> {
    Ok(None)
}

/// 이미 크기를 더한 하드링크 파일. 링크가 하나뿐인 파일은 담지 않는다.
#[derive(Default)]
struct CountedLinks(std::collections::HashSet<FileId>);

impl CountedLinks {
    /// 처음 보는 파일이면 `true`.
    fn first(&mut self, id: Option<FileId>) -> bool {
        id.is_none_or(|id| self.0.insert(id))
    }
}

/// 폴더 크기의 배경 계산 진행. 폴더가 없으면 시작하자마자 끝난다.
#[derive(Default, Debug)]
pub(crate) struct FolderCount {
    pub(crate) items: AtomicU64,
    pub(crate) bytes: AtomicU64,
    pub(crate) done: AtomicBool,
    pub(crate) partial: AtomicBool,
}

/// 정보를 먼저 보내고, 폴더가 있으면 같은 worker 에서 크기를 센다. 취소되면 세기를 멈춘다.
pub(super) fn read_properties(
    paths: &[PathBuf],
    count: &Arc<FolderCount>,
    cancel: &AtomicBool,
    deliver: impl FnOnce(io::Result<PropertiesFacts>) -> bool,
) -> bool {
    let (facts, folders, mut links) = gather(paths);
    let delivered = deliver(facts);
    if delivered {
        for folder in folders {
            count_folder(&folder, count, cancel, &mut links);
        }
    }
    count.done.store(true, Ordering::Release);
    delivered
}

/// 항목마다 읽고, 하나라도 읽으면 읽은 것만 돌려준다. 모두 못 읽으면 첫 오류다.
/// 크기 합과 셀 폴더는 파일 작업과 같은 기준([`drop_nested`])으로 겹친 선택을 뺀 항목에서 정한다.
/// 여러 항목이면 하드링크를 가리려고 파일 식별자도 읽는다. 식별자를 못 읽은 항목은 같은 파일인지
/// 알 수 없어 읽지 못한 항목으로 다룬다.
fn gather(paths: &[PathBuf]) -> (io::Result<PropertiesFacts>, Vec<PathBuf>, CountedLinks) {
    let kept: std::collections::HashSet<PathBuf> = drop_nested(paths).into_iter().collect();
    let mut links = CountedLinks::default();
    let mut items = Vec::new();
    let mut failed = Vec::new();
    let mut first_error = None;
    let mut file_bytes = 0u64;
    let mut folders = Vec::new();
    for path in paths {
        let read = item_facts_with_meta(path).and_then(|(mut item, meta)| {
            if paths.len() > 1 {
                item.hardlink = hardlink_id(path, &meta)?;
            }
            Ok(item)
        });
        let item = match read {
            Ok(item) => item,
            Err(error) => {
                tracing::warn!(path = %path.display(), kind = ?error.kind(), %error, "properties: item unreadable");
                failed.push((path.clone(), error.kind()));
                first_error.get_or_insert(error);
                continue;
            }
        };
        if kept.contains(path) {
            match item.kind {
                ItemKind::Folder => folders.push(path.clone()),
                ItemKind::File if links.first(item.hardlink) => file_bytes += item.size,
                _ => {}
            }
        }
        items.push(item);
    }
    let facts = match first_error {
        Some(error) if items.is_empty() => Err(error),
        _ => Ok(PropertiesFacts {
            items,
            file_bytes,
            failed,
        }),
    };
    (facts, folders, links)
}

/// 항목 정보와 그때 읽은 자기 metadata(링크는 링크 자신).
fn item_facts_with_meta(path: &Path) -> io::Result<(ItemFacts, std::fs::Metadata)> {
    let own = std::fs::symlink_metadata(path)?;
    let link = own.file_type().is_symlink();
    let kind = if link {
        let target = std::fs::read_link(path)?;
        let state = match std::fs::canonicalize(path)
            .and_then(|resolved| std::fs::metadata(&resolved).map(|meta| (resolved, meta)))
        {
            Ok((path, meta)) => LinkTarget::Readable {
                path,
                is_dir: meta.is_dir(),
                size: meta.len(),
                modified: meta.modified().ok(),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => LinkTarget::Missing,
            Err(error) => return Err(error),
        };
        ItemKind::Link { target, state }
    } else if own.is_dir() {
        ItemKind::Folder
    } else {
        ItemKind::File
    };
    let item = ItemFacts {
        path: std::path::absolute(path)?,
        size: if own.is_dir() || link { 0 } else { own.len() },
        kind,
        modified: (!link).then(|| own.modified().ok()).flatten(),
        created: (!link).then(|| own.created().ok()).flatten(),
        mode: (!link).then(|| mode_string(&own)).flatten(),
        read_only: own.permissions().readonly(),
        hardlink: None,
    };
    Ok((item, own))
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
/// 항목 수는 이름마다 세고, 크기는 같은 하드링크 파일을 한 번만 더한다. 식별자를 못 읽은 파일은
/// 같은 파일인지 알 수 없어 크기를 더하지 않고 부분 집계로 남긴다.
fn count_folder(root: &Path, count: &FolderCount, cancel: &AtomicBool, links: &mut CountedLinks) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) || !count_dir(&dir, count, cancel, &mut stack, links) {
            return;
        }
    }
}

/// `dir` 바로 아래 항목을 세고 하위 폴더를 `stack` 에 쌓는다. 취소되면 `false`.
fn count_dir(
    dir: &Path,
    count: &FolderCount,
    cancel: &AtomicBool,
    stack: &mut Vec<PathBuf>,
    links: &mut CountedLinks,
) -> bool {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            count.partial.store(true, Ordering::Release);
            tracing::debug!(%error, dir = %dir.display(), "properties: skipped a folder");
            return true;
        }
    };
    for entry in entries {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let meta = match entry.and_then(|entry| {
            std::fs::symlink_metadata(entry.path()).map(|meta| (entry.path(), meta))
        }) {
            Ok(found) => found,
            Err(error) => {
                count.partial.store(true, Ordering::Release);
                tracing::debug!(%error, "properties: unreadable directory entry");
                continue;
            }
        };
        let (path, meta) = meta;
        count.items.fetch_add(1, Ordering::Relaxed);
        if meta.is_dir() {
            stack.push(path);
        } else if !meta.file_type().is_symlink() {
            add_file_size(&path, &meta, count, links);
        }
    }
    true
}

/// 처음 보는 파일이면 크기를 더한다. 식별자를 못 읽으면 더하지 않고 부분 집계로 남긴다.
fn add_file_size(
    path: &Path,
    meta: &std::fs::Metadata,
    count: &FolderCount,
    links: &mut CountedLinks,
) {
    match hardlink_id(path, meta) {
        Ok(id) if links.first(id) => {
            count.bytes.fetch_add(meta.len(), Ordering::Relaxed);
        }
        Ok(_) => {}
        Err(error) => {
            count.partial.store(true, Ordering::Release);
            tracing::debug!(%error, path = %path.display(), "properties: unreadable file id");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 아래 링크·권한 시험은 Unix 에서만 만들 수 있다.
    #[cfg(unix)]
    fn item_facts(path: &Path) -> io::Result<ItemFacts> {
        item_facts_with_meta(path).map(|(item, _)| item)
    }

    #[cfg(unix)]
    #[test]
    fn link_properties_resolve_relative_chains_and_distinguish_missing_from_errors() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.md");
        std::fs::write(&target, "target contents").unwrap();
        symlink("target.md", dir.path().join("middle")).unwrap();
        let link = dir.path().join("alias.txt");
        symlink("middle", &link).unwrap();
        let facts = item_facts(&link).unwrap();
        assert_eq!(facts.path, link);
        assert_eq!(facts.size, 0);
        assert!(facts.modified.is_none() && facts.created.is_none());
        match facts.kind {
            ItemKind::Link {
                target: raw,
                state:
                    LinkTarget::Readable {
                        path,
                        is_dir,
                        size,
                        modified,
                    },
            } => {
                assert_eq!(raw, PathBuf::from("middle"));
                assert_eq!(path, target.canonicalize().unwrap());
                let meta = std::fs::metadata(&target).unwrap();
                assert!(!is_dir);
                assert_eq!(size, meta.len());
                assert_eq!(modified, meta.modified().ok());
            }
            other => panic!("{other:?}"),
        }
        let broken = dir.path().join("broken");
        symlink("absent", &broken).unwrap();
        assert!(matches!(
            item_facts(&broken).unwrap().kind,
            ItemKind::Link {
                state: LinkTarget::Missing,
                ..
            }
        ));
        let cycle = dir.path().join("cycle");
        symlink("cycle", &cycle).unwrap();
        assert!(
            item_facts(&cycle).is_err(),
            "a link loop is not a missing target"
        );
        let private = dir.path().join("private");
        std::fs::create_dir(&private).unwrap();
        std::fs::write(private.join("file"), "secret").unwrap();
        let denied = dir.path().join("denied");
        symlink("private/file", &denied).unwrap();
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o0)).unwrap();
        let error = item_facts(&denied);
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(error.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    }

    #[cfg(unix)]
    #[test]
    fn partial_folder_counts_skip_unreadable_children_and_never_follow_links() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("plain"), "ab").unwrap();
        symlink(".", dir.path().join("loop")).unwrap();
        let private = dir.path().join("private");
        std::fs::create_dir(&private).unwrap();
        std::fs::write(private.join("hidden"), "not counted").unwrap();
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o0)).unwrap();
        let count = FolderCount::default();
        count_folder(
            dir.path(),
            &count,
            &AtomicBool::new(false),
            &mut CountedLinks::default(),
        );
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(count.bytes.load(Ordering::Relaxed), 2);
        assert_eq!(count.items.load(Ordering::Relaxed), 3);
        assert!(count.partial.load(Ordering::Acquire));
        let cancelled = FolderCount::default();
        count_folder(
            dir.path(),
            &cancelled,
            &AtomicBool::new(true),
            &mut CountedLinks::default(),
        );
        assert_eq!(cancelled.items.load(Ordering::Relaxed), 0);
        assert!(!cancelled.partial.load(Ordering::Acquire));
        let dropped = Arc::new(FolderCount::default());
        assert!(!read_properties(
            &[dir.path().to_path_buf()],
            &dropped,
            &AtomicBool::new(false),
            |_| false
        ));
        assert_eq!(dropped.items.load(Ordering::Relaxed), 0);
        assert!(dropped.done.load(Ordering::Acquire));
    }

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
        assert!(
            matches!(read_preview(&small, PREVIEW_TEXTURE_SIDE), Ok(PreviewData::Text(t)) if t == "# Notes\n")
        );

        let big = dir.path().join("server.log");
        std::fs::write(&big, vec![b'a'; PREVIEW_MAX_BYTES as usize + 1]).expect("write");
        assert!(matches!(
            read_preview(&big, PREVIEW_TEXTURE_SIDE),
            Ok(PreviewData::TooLarge(TooLarge::Bytes))
        ));

        let bin = dir.path().join("blob.bin");
        std::fs::write(&bin, [0u8, 1, 2]).expect("write");
        assert!(matches!(
            read_preview(&bin, PREVIEW_TEXTURE_SIDE),
            Ok(PreviewData::Unsupported)
        ));
        assert!(matches!(
            read_preview(dir.path(), PREVIEW_TEXTURE_SIDE),
            Ok(PreviewData::Unsupported)
        ));
    }

    fn write_png(path: &Path, w: u32, h: u32) {
        image::RgbImage::new(w, h).save(path).expect("write png");
    }

    #[test]
    fn a_wide_image_is_shrunk_under_the_texture_side_and_keeps_its_size() {
        let dir = tempfile::tempdir().expect("tempdir");
        for (w, h) in [(9000, 16), (2560, 1440), (16, 9000)] {
            let path = dir.path().join(format!("{w}x{h}.png"));
            write_png(&path, w, h);
            let Ok(PreviewData::Image { image, size }) = read_preview(&path, PREVIEW_TEXTURE_SIDE)
            else {
                panic!("{w}x{h} should preview as an image");
            };
            assert_eq!(size, [w as usize, h as usize]);
            let side = PREVIEW_TEXTURE_SIDE as usize;
            assert!(
                image.size[0] <= side && image.size[1] <= side,
                "{:?}",
                image.size
            );
            assert!(image.size[0] >= 1 && image.size[1] >= 1);
            let thumb = read_thumbnail(&path).expect("thumbnail");
            let px = THUMB_DECODE_PX as usize;
            assert!(thumb.size[0] <= px && thumb.size[1] <= px);
        }
    }

    #[test]
    fn a_preview_image_is_fitted_to_the_panel_width_and_never_enlarged() {
        let dir = tempfile::tempdir().expect("tempdir");
        let wide = dir.path().join("wide.png");
        write_png(&wide, 2560, 1440);
        let Ok(PreviewData::Image { image, size }) = read_preview(&wide, 436) else {
            panic!("should preview as an image");
        };
        assert_eq!(size, [2560, 1440]);
        assert_eq!(image.size, [436, 245]);

        let small = dir.path().join("small.png");
        write_png(&small, 120, 80);
        let Ok(PreviewData::Image { image, .. }) = read_preview(&small, 436) else {
            panic!("should preview as an image");
        };
        assert_eq!(image.size, [120, 80]);
    }

    #[test]
    fn an_image_over_the_pixel_limits_is_refused_before_decoding() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wide.png");
        write_png(&path, MAX_IMAGE_SIDE + 1, 1);
        assert!(matches!(
            read_preview(&path, PREVIEW_TEXTURE_SIDE),
            Ok(PreviewData::TooLarge(TooLarge::Pixels(Some([w, 1])))) if w == MAX_IMAGE_SIDE + 1
        ));
        assert!(read_thumbnail(&path).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_is_not_opened() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pipe.png");
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).expect("c path");
        // SAFETY: c_path 는 NUL 로 끝나는 유효한 경로 문자열이다.
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        // 열기를 시도하면 쓰는 쪽이 없어 끝나지 않으므로 다른 스레드에서 기한을 둔다.
        let (tx, rx) = std::sync::mpsc::channel();
        let probe = path.clone();
        std::thread::spawn(move || {
            let preview = matches!(
                read_preview(&probe, PREVIEW_TEXTURE_SIDE),
                Ok(PreviewData::Unsupported)
            );
            let thumbnail = read_thumbnail(&probe).is_err();
            tx.send((preview, thumbnail)).expect("send");
        });
        let got = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("a FIFO read must not block");
        assert_eq!(got, (true, true));
        let txt = dir.path().join("pipe.txt");
        std::fs::rename(&path, &txt).expect("rename");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            tx.send(matches!(
                read_preview(&txt, PREVIEW_TEXTURE_SIDE),
                Ok(PreviewData::Unsupported)
            ))
            .expect("send");
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(5))
                .expect("a FIFO read must not block")
        );
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

    /// `read_properties` 를 돌려 받은 정보와 폴더 집계를 돌려준다.
    fn read_all(paths: &[PathBuf]) -> (io::Result<PropertiesFacts>, Arc<FolderCount>) {
        let count = Arc::new(FolderCount::default());
        let mut got = None;
        read_properties(paths, &count, &AtomicBool::new(false), |facts| {
            got = Some(facts);
            true
        });
        (got.expect("delivered"), count)
    }

    #[test]
    fn a_vanished_item_among_readable_ones_keeps_their_values_and_records_why() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "abcd").expect("write");
        let gone = dir.path().join("gone.txt");
        let (facts, _) = read_all(&[file.clone(), gone.clone()]);
        let facts = facts.expect("one item is readable");
        assert_eq!(facts.items.len(), 1);
        assert_eq!(facts.file_bytes, 4);
        assert_eq!(facts.failed, [(gone.clone(), io::ErrorKind::NotFound)]);

        let (facts, _) = read_all(&[gone.clone(), dir.path().join("also-gone")]);
        assert_eq!(
            facts.expect_err("nothing is readable").kind(),
            io::ErrorKind::NotFound
        );
    }

    /// 부모 폴더의 검색 권한을 없애 `symlink_metadata` 자체를 실패시킨다. 권한 검사를 받지 않는
    /// 실행(root 등)이면 실패가 일어나지 않으므로 건너뛴다.
    #[cfg(unix)]
    #[test]
    fn an_item_behind_a_folder_without_search_permission_is_reported_as_denied() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("ok.txt");
        std::fs::write(&file, "xy").expect("write");
        let locked = dir.path().join("d");
        std::fs::create_dir(&locked).expect("mkdir");
        let hidden = locked.join("x");
        std::fs::write(&hidden, "secret").expect("write");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        let denied = std::fs::symlink_metadata(&hidden).is_err();
        let (facts, count) = read_all(&[file, hidden.clone()]);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700))
            .expect("chmod back");
        if !denied {
            return;
        }
        let facts = facts.expect("the readable file is kept");
        assert_eq!(facts.file_bytes, 2);
        assert_eq!(facts.failed, [(hidden, io::ErrorKind::PermissionDenied)]);
        assert!(
            !count.partial.load(Ordering::Acquire),
            "an unreadable chosen item is not the folder count's partial state"
        );
    }

    #[test]
    fn a_folder_and_a_file_inside_it_count_the_file_once_like_file_jobs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let folder = dir.path().join("a");
        std::fs::create_dir(&folder).expect("mkdir");
        let inner = folder.join("f");
        std::fs::write(&inner, vec![0u8; 1024]).expect("write");
        let chosen = [folder.clone(), inner.clone()];
        assert_eq!(
            drop_nested(&chosen),
            std::slice::from_ref(&folder),
            "file jobs keep the folder only"
        );
        let (facts, count) = read_all(&chosen);
        let facts = facts.expect("readable");
        assert_eq!(facts.items.len(), 2, "both chosen items are still listed");
        assert_eq!(
            facts.file_bytes, 0,
            "the inner file is carried by the folder"
        );
        assert_eq!(count.bytes.load(Ordering::Relaxed), 1024);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn hard_links_add_their_size_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = dir.path().join("a.bin");
        std::fs::write(&a, vec![0u8; 1000]).expect("write");
        let b = dir.path().join("b.bin");
        std::fs::hard_link(&a, &b).expect("link");
        let (facts, _) = read_all(&[a.clone(), b.clone()]);
        assert_eq!(facts.expect("readable").file_bytes, 1000);

        // 폴더 안의 두 이름도, 고른 파일과 폴더 안의 이름도 한 번만 더한다. 항목 수는 이름마다 센다.
        let folder = dir.path().join("sub");
        std::fs::create_dir(&folder).expect("mkdir");
        std::fs::hard_link(&a, folder.join("c.bin")).expect("link");
        std::fs::hard_link(&a, folder.join("d.bin")).expect("link");
        let (_, count) = read_all(std::slice::from_ref(&folder));
        assert_eq!(count.items.load(Ordering::Relaxed), 2);
        assert_eq!(count.bytes.load(Ordering::Relaxed), 1000);
        let (facts, count) = read_all(&[a, folder]);
        assert_eq!(facts.expect("readable").file_bytes, 1000);
        assert_eq!(count.bytes.load(Ordering::Relaxed), 0);
    }

    /// 폴더 집계 시간을 하드링크 식별 없이 걸은 시간과 비교해 `TASTY_PROPS_COST_OUT` 파일에 줄마다 적는다.
    /// Windows 는 파일마다 식별자를 읽으려고 한 번 열어 그 비용이 여기서 드러난다. 파일 수는
    /// `TASTY_PROPS_COST_FILES`(기본 20000).
    /// `TASTY_PROPS_COST_OUT=<파일> cargo test -p tasty --lib --locked -- --ignored properties_count_cost`
    #[test]
    #[ignore = "measurement only; writes timings to TASTY_PROPS_COST_OUT"]
    fn properties_count_cost() {
        use std::io::Write;
        let out = std::env::var_os("TASTY_PROPS_COST_OUT").expect("set TASTY_PROPS_COST_OUT");
        let mut out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(out)
            .expect("open TASTY_PROPS_COST_OUT");
        let files: usize = std::env::var("TASTY_PROPS_COST_FILES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(20_000);
        let dir = tempfile::tempdir().expect("tempdir");
        for i in 0..files {
            let sub = dir.path().join(format!("d{}", i % 100));
            std::fs::create_dir_all(&sub).expect("mkdir");
            std::fs::write(sub.join(format!("f{i}")), b"x").expect("write");
        }
        let walk_without_ids = || {
            let mut stack = vec![dir.path().to_path_buf()];
            let mut bytes = 0u64;
            while let Some(d) = stack.pop() {
                for entry in std::fs::read_dir(&d).expect("read_dir").flatten() {
                    let meta = std::fs::symlink_metadata(entry.path()).expect("meta");
                    if meta.is_dir() {
                        stack.push(entry.path());
                    } else {
                        bytes += meta.len();
                    }
                }
            }
            bytes
        };
        for round in 0..3 {
            let started = std::time::Instant::now();
            let plain = walk_without_ids();
            let plain_time = started.elapsed();
            let count = FolderCount::default();
            let started = std::time::Instant::now();
            count_folder(
                dir.path(),
                &count,
                &AtomicBool::new(false),
                &mut CountedLinks::default(),
            );
            let counted_time = started.elapsed();
            assert_eq!(count.bytes.load(Ordering::Relaxed), plain);
            writeln!(
                out,
                "round {round}: {files} files · without ids {plain_time:?} · count_folder {counted_time:?}"
            )
            .expect("write timings");
        }
    }
}
