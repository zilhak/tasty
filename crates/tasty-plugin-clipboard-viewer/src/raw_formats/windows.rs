//! Windows raw 클립보드 포맷 열거/읽기(`clipboard-win`, arboard 와 동일 버전).
//!
//! 이 모듈은 Linux 조사/구현 머신에서 컴파일 검증이 불가능하다 — `EnumFormats`/
//! `format_name_big`/`get_vec` 는 로컬에 벤더링된
//! `clipboard-win-5.4.1/src/raw.rs:301,927-953,1081` 소스를 직접 읽어 확인한 실제
//! 공개 API 이고, arboard 자신의 `src/platform/windows.rs`(레지스트리 캐시)를 근거로
//! text/image/html 이 실제로 쓰는 포맷(`CF_UNICODETEXT`, `CF_DIBV5`+등록 포맷 "PNG",
//! 등록 포맷 "HTML Format")을 그대로 확인했다. Windows 빌드 환경에서 재확인 필요.

use clipboard_win::formats::{CF_DIB, CF_DIBV5, CF_HDROP, CF_OEMTEXT, CF_TEXT, CF_UNICODETEXT};
use clipboard_win::{Clipboard, EnumFormats, raw};

use super::MAX_RAW_BYTES;
use crate::clipboard::OtherFormatEntry;

/// text/files/image 로 이미 소비된 고정 포맷 ID. Windows 는 텍스트에 세 변형이
/// 동시에 등록되는 게 흔하다(`CF_TEXT`/`CF_UNICODETEXT`/`CF_OEMTEXT`) — 이 중 arboard
/// 가 실제로 읽는 건 `CF_UNICODETEXT` 뿐이지만, 셋 다 "텍스트"라 전부 제외한다
/// ("기타"는 배타적 카테고리가 아니다 — 단일 ID 비교 금지).
const CONSUMED_FIXED_IDS: &[u32] = &[
    CF_TEXT,
    CF_UNICODETEXT,
    CF_OEMTEXT,
    CF_HDROP,
    CF_DIB,
    CF_DIBV5,
];

/// html/image 는 `RegisterClipboardFormat`으로 등록되는 이름 기반 포맷이라 세션마다
/// ID 가 달라진다 — 상수 ID 가 아니라 `format_name_big`이 돌려주는 이름으로 판별한다.
fn is_consumed_by_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("HTML Format") || name.eq_ignore_ascii_case("PNG")
}

pub(super) fn read_other() -> Vec<OtherFormatEntry> {
    let _clip = match Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("clipboard-win open failed, skipping other-format enumeration: {e}");
            return Vec::new();
        }
    };

    let mut out = Vec::new();
    for id in EnumFormats::new() {
        if CONSUMED_FIXED_IDS.contains(&id) {
            continue;
        }
        let name = raw::format_name_big(id).unwrap_or_else(|| format!("Format #{id}"));
        if is_consumed_by_name(&name) {
            continue;
        }
        let mut buf = Vec::new();
        match raw::get_vec(id, &mut buf) {
            Ok(_) => out.push(OtherFormatEntry::from_bytes(name, &buf, MAX_RAW_BYTES)),
            // TARGETS 상당 열거(EnumFormats)와 개별 재조회 사이의 race, 혹은 이
            // 프로세스가 읽을 수 없는 핸들 타입 — 이 포맷만 건너뛴다(개별 격리,
            // 전체 "기타" 열거를 실패시키지 않음).
            Err(e) => tracing::debug!("clipboard-win get_vec({id}, {name}) failed: {e}"),
        }
    }
    out
}
