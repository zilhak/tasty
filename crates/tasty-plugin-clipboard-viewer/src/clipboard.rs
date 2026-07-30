//! Type-keyed 클립보드 reader 추상화.
//!
//! 새 포맷(헥스 / RTF 등) 추가가 `ClipboardType` enum arm + `read_available`
//! 내 reader 한 줄 추가로 끝나도록 설계한다. Text/Files/Image/Html 은 arboard 로 3 OS
//! 공통 read 한다(TODO48 — Image 는 `get_image()`, TODO49 — Html 은 `get().html()`,
//! feature gate 없이 제공). Other(TODO50)는 arboard 가 노출하지 않는 raw 포맷 열거를
//! `crate::raw_formats`(플랫폼별 `#[cfg(...)]` 분기)로 직접 구현해 text/files/image/
//! html 로 이미 소비된 변형을 제외한 나머지를 하나로 묶는다.

use std::path::PathBuf;

/// 클립보드에 담길 수 있는 콘텐츠 타입. 좌측 목록의 키이자 선택 단위.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipboardType {
    Text,
    Files,
    Image,
    Html,
    /// text/files/image/html 어디에도 속하지 않는 raw 포맷들을 하나로 묶은 버킷
    /// (TODO50). 개별 포맷이 아니라 "그 외 전부"라 다른 arm 과 달리 항상 여러
    /// 포맷을 하나의 값으로 들고 다닌다(`ContentRepr::Other`).
    Other,
}

impl ClipboardType {
    /// 타입 라벨 i18n 키.
    pub fn label_i18n_key(self) -> &'static str {
        match self {
            ClipboardType::Text => "clipboard_viewer.type.text",
            ClipboardType::Files => "clipboard_viewer.type.files",
            ClipboardType::Image => "clipboard_viewer.type.image",
            ClipboardType::Html => "clipboard_viewer.type.html",
            ClipboardType::Other => "clipboard_viewer.type.other",
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
            ClipboardType::Html => "text/html",
            // 여러 이종 포맷을 하나로 묶은 버킷이라 단일 mime 이 없다 — RFC 2046 의
            // "종류를 모르는 바이너리" 기본값. 실제 footer 표시는 `view::footer_mime_text`
            // 가 이 값 대신 포맷 개수 메타로 대체한다(항상 비어있지 않은 채로 push 되므로
            // 이 문자열이 실제로 화면에 나가는 경우는 없다).
            ClipboardType::Other => "application/octet-stream",
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
    Html(String),
    /// text/files/image/html 어디에도 안 걸린 raw 포맷 전부(TODO50) — 포맷 이름 +
    /// 텍스트화된 미리보기의 목록. 비어 있으면 애초에 `read_available()`가 이 arm
    /// 을 push 하지 않는다(항상 비어있지 않음).
    Other(Vec<OtherFormatEntry>),
}

impl ContentRepr {
    /// design `t.meta` — type-bar 우측 슬롯 + image body 안내에 쓰는 요약 문자열.
    /// `Text`/`Files`/`Html`/`Other` 은 이 TODO 범위 밖(문자/줄 수·파일 개수 카운트
    /// 미구현, TODO51 부터 이어지는 defer)이라 `None` — 기존처럼 type-bar 우측 슬롯이
    /// 빈 채로 유지된다(Html 은 대신 view.rs 의 Pretty print 체크박스가, Other 는
    /// 포맷 개수 tooltip 이 그 슬롯 대신 type-bar 세그먼트 쪽을 차지한다).
    pub fn meta_text(&self) -> Option<String> {
        match self {
            ContentRepr::Text(_) => None,
            ContentRepr::Files(_) => None,
            ContentRepr::Image {
                width,
                height,
                byte_len,
            } => Some(format!("{width}×{height} · {}", format_bytes(*byte_len))),
            ContentRepr::Html(_) => None,
            ContentRepr::Other(_) => None,
        }
    }
}

/// "기타" 버킷 한 포맷 항목 — 포맷 이름 + raw 바이트를 텍스트화한 미리보기(design
/// 확정 결과, TODO50). `crate::raw_formats`의 플랫폼별 모듈이 raw 바이트를 읽은 뒤
/// [`OtherFormatEntry::from_bytes`]로 변환해 채운다.
#[derive(Clone, Debug)]
pub struct OtherFormatEntry {
    /// 포맷 이름(OS 가 보고하는 사람이 읽는 이름, 없으면 ID 기반 fallback).
    pub name: String,
    /// 원본 raw 바이트 길이(미리보기 절삭 전 실제 크기 — design "크기 정보").
    pub byte_len: usize,
    /// 텍스트화된 미리보기. 바이너리로 판단되면 hex 요약(`is_binary` 참고), 아니면
    /// `from_utf8_lossy` 결과 — 어느 쪽이든 raw 바이트 자체를 로그에 남기지 않는다
    /// (TODO50 — 민감 데이터일 수 있음).
    pub preview: String,
    /// `preview`가 hex 요약(바이너리 fallback)인지 — 뷰가 스타일을 달리할 수 있게.
    pub is_binary: bool,
}

impl OtherFormatEntry {
    /// raw 바이트 → 표시용 항목. `cap`(바이트)을 넘는 데이터는 미리보기 생성 전에
    /// 잘라낸다(TODO50 "크기 상한" 요구사항 — 거대한 바이너리 포맷을 통째로 문자열화
    /// 하지 않는다). U+FFFD(치환 문자) 비율이 높으면 텍스트가 아니라 바이너리로 보고
    /// hex 요약으로 대체한다.
    pub(crate) fn from_bytes(name: String, bytes: &[u8], cap: usize) -> Self {
        let byte_len = bytes.len();
        let capped = &bytes[..byte_len.min(cap)];
        let lossy = String::from_utf8_lossy(capped);
        let replacement_ratio = if lossy.is_empty() {
            0.0
        } else {
            lossy.chars().filter(|&c| c == '\u{FFFD}').count() as f64 / lossy.chars().count() as f64
        };
        let is_binary = replacement_ratio > 0.05;
        let preview = if is_binary {
            hex_summary(capped)
        } else {
            lossy.into_owned()
        };
        Self {
            name,
            byte_len,
            preview,
            is_binary,
        }
    }
}

/// 바이너리로 판단된 raw 바이트의 hex 요약(공백 구분 바이트 쌍) — 앞부분만
/// 보여준다(전체를 hex 로 펼치면 텍스트보다 몇 배 길어져 오히려 읽기 어렵다).
fn hex_summary(bytes: &[u8]) -> String {
    const HEX_PREVIEW_BYTES: usize = 256;
    bytes[..bytes.len().min(HEX_PREVIEW_BYTES)]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 사람이 읽는 바이트 크기 문자열(`src/core/fs_list.rs::human_size` 와 동형 — 이
/// plugin 은 별도 프로세스 바이너리라 그 crate 를 의존할 수 없어 로컬 재구현).
/// TODO48 은 원본 파일 크기가 아니라 `ImageData::bytes`(raw RGBA8) 길이의 근사치로
/// 썼고, TODO50 의 "기타" 포맷 크기 표시도 이 함수를 그대로 재사용한다.
pub(crate) fn format_bytes(n: usize) -> String {
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

/// Html 리더 — arboard `Get::html()` (feature gate 없이 3플랫폼 공통 제공).
fn read_html(clip: &mut arboard::Clipboard) -> Option<(ClipboardType, ContentRepr)> {
    match clip.get().html() {
        Ok(html) if !html.is_empty() => Some((ClipboardType::Html, ContentRepr::Html(html))),
        Ok(_) => None,
        Err(e) => {
            tracing::debug!("clipboard get_html: {e}");
            None
        }
    }
}

/// Other 리더(TODO50) — arboard 를 거치지 않고 `crate::raw_formats`(플랫폼별 raw
/// 열거)로 text/files/image/html 이 아닌 나머지 포맷을 모은다. arboard 는 포맷
/// 열거 자체를 노출하지 않아 이 타입만 별도 경로를 쓴다.
fn read_other() -> Option<(ClipboardType, ContentRepr)> {
    let entries = crate::raw_formats::read_other();
    if entries.is_empty() {
        None
    } else {
        Some((ClipboardType::Other, ContentRepr::Other(entries)))
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
    out.extend(read_html(&mut clip));
    out.extend(read_other());
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
    fn html_meta_text_is_none() {
        assert_eq!(ContentRepr::Html("<p>x</p>".into()).meta_text(), None);
    }

    #[test]
    fn image_type_uses_own_label_and_mime() {
        assert_eq!(
            ClipboardType::Image.label_i18n_key(),
            "clipboard_viewer.type.image"
        );
        assert_eq!(ClipboardType::Image.mime_str(), "image/rgba8");
    }

    #[test]
    fn html_type_uses_own_label_and_mime() {
        assert_eq!(
            ClipboardType::Html.label_i18n_key(),
            "clipboard_viewer.type.html"
        );
        assert_eq!(ClipboardType::Html.mime_str(), "text/html");
    }

    #[test]
    fn other_type_uses_own_label_and_mime() {
        assert_eq!(
            ClipboardType::Other.label_i18n_key(),
            "clipboard_viewer.type.other"
        );
        assert_eq!(ClipboardType::Other.mime_str(), "application/octet-stream");
    }

    #[test]
    fn other_meta_text_is_none() {
        assert_eq!(ContentRepr::Other(Vec::new()).meta_text(), None);
    }

    #[test]
    fn other_format_entry_plain_text_is_not_binary() {
        let entry = OtherFormatEntry::from_bytes("Custom Format".into(), b"hello world", 4096);
        assert_eq!(entry.byte_len, 11);
        assert_eq!(entry.preview, "hello world");
        assert!(!entry.is_binary);
    }

    #[test]
    fn other_format_entry_binary_falls_back_to_hex() {
        let bytes: Vec<u8> = (0..=255u8).collect();
        let entry = OtherFormatEntry::from_bytes("Binary Format".into(), &bytes, 4096);
        assert!(entry.is_binary);
        assert_eq!(entry.byte_len, 256);
        assert!(entry.preview.starts_with("00 01 02"));
    }

    #[test]
    fn other_format_entry_caps_before_previewing() {
        let bytes = vec![b'a'; 10_000];
        let entry = OtherFormatEntry::from_bytes("Big Format".into(), &bytes, 100);
        // byte_len 은 원본(절삭 전) 크기를 보존 — "크기 정보"는 실제 크기를 보여줘야
        // 한다(design). preview 만 cap 만큼만 텍스트화된다.
        assert_eq!(entry.byte_len, 10_000);
        assert_eq!(entry.preview.len(), 100);
        assert!(!entry.is_binary);
    }
}
