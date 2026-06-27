//! Type-keyed 클립보드 reader 추상화.
//!
//! 새 포맷(이미지 / 헥스 / HTML / RTF 등) 추가가 `ClipboardType` enum arm +
//! `read_available` 내 reader 한 줄 추가로 끝나도록 설계한다. 1차는 텍스트만
//! 지원하며 arboard 로 3 OS 공통 read 한다. 플랫폼별 네이티브 포맷이 필요한
//! 타입은 도입 시점에 `#[cfg(...)]` 분기로 reader 를 붙인다.

/// 클립보드에 담길 수 있는 콘텐츠 타입. 좌측 목록의 키이자 선택 단위.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipboardType {
    Text,
    // Image, Hex, Html, Rtf — 추후 타입 도입 시 arm 추가.
}

impl ClipboardType {
    /// node id / 선택 직렬화에 쓰는 안정 키 (`type-{key}`).
    pub fn key(self) -> &'static str {
        match self {
            ClipboardType::Text => "text",
        }
    }

    /// `type-{key}` 클릭에서 역파싱.
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "text" => Some(ClipboardType::Text),
            _ => None,
        }
    }

    /// 타입 라벨 i18n 키.
    pub fn label_i18n_key(self) -> &'static str {
        match self {
            ClipboardType::Text => "clipboard_viewer.type.text",
        }
    }
}

/// 한 타입의 표시용 표현. 추후 bytes / 공유버퍼 이미지 핸들 등으로 확장.
#[derive(Clone, Debug)]
pub enum ContentRepr {
    Text(String),
}

/// 현재 시스템 클립보드에서 가용한 (타입, 내용) 목록을 수집한다.
///
/// - `Ok(vec)` — 가용 타입 목록. 빈 vec 이면 표시할 내용이 없는 상태(빈 클립보드).
/// - `Err(msg)` — 클립보드 핸들 자체를 못 연 경우(read 실패 상태).
pub fn read_available() -> Result<Vec<(ClipboardType, ContentRepr)>, String> {
    let mut clip = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let mut out = Vec::new();

    // Text — 비어있지 않은 텍스트가 있을 때만 타입으로 노출.
    match clip.get_text() {
        Ok(text) if !text.is_empty() => {
            out.push((ClipboardType::Text, ContentRepr::Text(text)));
        }
        Ok(_) => {}
        Err(e) => {
            // 텍스트 없음/접근 불가는 치명 오류가 아니라 "Text 타입 부재"로 처리.
            tracing::debug!("clipboard get_text: {e}");
        }
    }

    Ok(out)
}
