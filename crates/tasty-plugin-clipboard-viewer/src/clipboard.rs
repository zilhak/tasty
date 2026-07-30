//! Type-keyed 클립보드 reader 추상화.
//!
//! 새 포맷(헥스 / HTML / RTF 등) 추가가 `ClipboardType` enum arm + `read_available`
//! 내 reader 한 줄 추가로 끝나도록 설계한다. Text/Files 는 arboard 로 3 OS 공통
//! read 한다. 플랫폼별 네이티브 포맷이 필요한 타입은 도입 시점에 `#[cfg(...)]`
//! 분기로 reader 를 붙인다.

use std::path::PathBuf;

/// 클립보드에 담길 수 있는 콘텐츠 타입. 좌측 목록의 키이자 선택 단위.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipboardType {
    Text,
    Files,
    Image,
    // Hex, Html, Rtf — 추후 타입 도입 시 arm 추가.
}

impl ClipboardType {
    /// 타입 라벨 i18n 키.
    pub fn label_i18n_key(self) -> &'static str {
        match self {
            ClipboardType::Text => "clipboard_viewer.type.text",
            ClipboardType::Files => "clipboard_viewer.type.files",
            ClipboardType::Image => "clipboard_viewer.type.image",
        }
    }

    /// 푸터에 표시할 MIME 타입 문자열(design `t.mime`) — 기술 용어라 번역하지 않는다.
    pub fn mime_str(self) -> &'static str {
        match self {
            ClipboardType::Text => "text/plain",
            ClipboardType::Files => "text/uri-list",
            // arboard::get_image() 은 원본 인코딩(PNG/JPEG 등) 정보 없이 항상 raw
            // RGBA8 픽셀로 정규화해 반환한다(`ImageData::bytes` 문서) — 디자인 mock 의
            // "image/png" 는 예시 데이터일 뿐, 실제로 알 수 없는 원본 포맷을 사칭하지
            // 않고 arboard 가 실제로 반환하는 표현을 그대로 명명한다.
            ClipboardType::Image => "image/rgba8",
        }
    }
}

/// 한 타입의 표시용 표현. 추후 bytes / 공유버퍼 이미지 핸들 등으로 확장.
#[derive(Clone, Debug)]
pub enum ContentRepr {
    Text(String),
    Files(Vec<PathBuf>),
    /// 이미지는 렌더링하지 않는다(design 결정, TODO48) — 픽셀 바이트를 들고 있을
    /// 필요가 없어 치수/바이트 수 메타만 보존한다.
    Image {
        width: usize,
        height: usize,
        byte_len: usize,
    },
}

impl ContentRepr {
    /// design `t.meta` — type-bar 우측 슬롯 + image body 안내에 쓰는 요약 문자열.
    /// `Text`/`Files` 는 이 TODO 범위 밖(문자/줄 수·파일 개수 카운트 미구현,
    /// TODO51 부터 이어지는 defer)이라 `None` — 기존처럼 type-bar 우측 슬롯이 빈
    /// 채로 유지된다.
    pub fn meta_text(&self) -> Option<String> {
        match self {
            ContentRepr::Text(_) => None,
            ContentRepr::Files(_) => None,
            ContentRepr::Image {
                width,
                height,
                byte_len,
            } => Some(format!("{width}×{height} · {}", format_bytes(*byte_len))),
        }
    }
}

/// 사람이 읽는 바이트 크기 문자열(`src/core/fs_list.rs::human_size` 와 동형 — 이
/// plugin 은 별도 프로세스 바이너리라 그 crate 를 의존할 수 없어 로컬 재구현).
/// TODO 문서가 명시한 대로 원본 파일 크기가 아니라 `ImageData::bytes`(raw RGBA8)
/// 길이의 근사치다.
fn format_bytes(n: usize) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut s = n as f64;
    let mut u = 0;
    while s >= 1024.0 && u < UNITS.len() - 1 {
        s /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} {}", UNITS[0])
    } else {
        format!("{s:.1} {}", UNITS[u])
    }
}

/// Text 리더 — 비어있지 않은 텍스트가 있을 때만 `Some`.
fn read_text(clip: &mut arboard::Clipboard) -> Option<(ClipboardType, ContentRepr)> {
    match clip.get_text() {
        Ok(text) if !text.is_empty() => Some((ClipboardType::Text, ContentRepr::Text(text))),
        Ok(_) => None,
        Err(e) => {
            // 텍스트 없음/접근 불가는 치명 오류가 아니라 "Text 타입 부재"로 처리.
            tracing::debug!("clipboard get_text: {e}");
            None
        }
    }
}

/// Files 리더 — 비어있지 않은 파일 목록이 있을 때만 `Some`.
fn read_files(clip: &mut arboard::Clipboard) -> Option<(ClipboardType, ContentRepr)> {
    match clip.get().file_list() {
        Ok(files) if !files.is_empty() => Some((ClipboardType::Files, ContentRepr::Files(files))),
        Ok(_) => None,
        Err(e) => {
            // 파일 목록 없음/접근 불가는 치명 오류가 아니라 "Files 타입 부재"로 처리.
            tracing::debug!("clipboard get file_list: {e}");
            None
        }
    }
}

/// Image 리더 — 렌더링하지 않으므로(design 결정) 픽셀 바이트는 버리고 메타만 보존.
fn read_image(clip: &mut arboard::Clipboard) -> Option<(ClipboardType, ContentRepr)> {
    match clip.get_image() {
        Ok(img) => Some((
            ClipboardType::Image,
            ContentRepr::Image {
                width: img.width,
                height: img.height,
                byte_len: img.bytes.len(),
            },
        )),
        Err(e) => {
            // 이미지 없음/디코딩 불가는 치명 오류가 아니라 "Image 타입 부재"로 처리.
            tracing::debug!("clipboard get_image: {e}");
            None
        }
    }
}

/// 현재 시스템 클립보드에서 가용한 (타입, 내용) 목록을 수집한다.
///
/// - `Ok(vec)` — 가용 타입 목록. 빈 vec 이면 표시할 내용이 없는 상태(빈 클립보드).
/// - `Err(msg)` — 클립보드 핸들 자체를 못 연 경우(read 실패 상태).
///
/// 타입별 리더를 개별 함수로 뽑아둔다 — 이 함수 본문에 인라인하면 타입이 늘어날수록
/// cognitive_complexity(clippy deny-level lint, workspace `Cargo.toml`)에 걸린다.
pub fn read_available() -> Result<Vec<(ClipboardType, ContentRepr)>, String> {
    let mut clip = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    out.extend(read_text(&mut clip));
    out.extend(read_files(&mut clip));
    out.extend(read_image(&mut clip));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(4096), "4.0 KB");
        assert_eq!(format_bytes(2 * 1024 * 1024 + 300 * 1024), "2.3 MB");
    }

    #[test]
    fn image_meta_text_formats_dimensions_and_size() {
        let repr = ContentRepr::Image {
            width: 1920,
            height: 1080,
            byte_len: 1920 * 1080 * 4,
        };
        assert_eq!(repr.meta_text().as_deref(), Some("1920×1080 · 7.9 MB"));
    }

    #[test]
    fn text_meta_text_is_none() {
        assert_eq!(ContentRepr::Text("x".into()).meta_text(), None);
    }

    #[test]
    fn image_type_uses_own_label_and_mime() {
        assert_eq!(
            ClipboardType::Image.label_i18n_key(),
            "clipboard_viewer.type.image"
        );
        assert_eq!(ClipboardType::Image.mime_str(), "image/rgba8");
    }
}
