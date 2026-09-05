//! macOS raw 클립보드 타입 열거/읽기(`objc2-app-kit`/`objc2-foundation`, arboard 와
//! 동일 버전).
//!
//! 이 모듈은 Linux 조사/구현 머신에서 컴파일 검증이 불가능하다 — `NSPasteboard`의
//! `types()`/`dataForType()` 는 로컬에 벤더링된
//! `objc2-app-kit-0.3.2/src/generated/NSPasteboard.rs:326,429,459`(둘 다 safe fn,
//! `unsafe` 블록 불필요 — `#![forbid(unsafe_code)]`와 충돌 없음)을 직접 읽어 확인한
//! 실제 공개 바인딩이고, arboard 자신의 `arboard/src/platform/osx.rs`(레지스트리 캐시)를
//! 근거로 text/html/image 가 실제로 읽는 UTI(`NSPasteboardTypeString`/
//! `NSPasteboardTypeHTML`/`NSPasteboardTypeTIFF`)를 확인했다. macOS 빌드 환경에서
//! 재확인 필요.

use objc2_app_kit::NSPasteboard;

use super::MAX_RAW_BYTES;
use crate::clipboard::OtherFormatEntry;

/// text/files/image/html 로 이미 소비된 것으로 간주하는 UTI 및 레거시 pasteboard 타입
/// 이름. 완전한 목록이 아니라 실제로 관측되는 흔한 변형들이다("기타"는 배타적
/// 카테고리가 아니다 — 단일 이름 비교가 아니라 매핑 테이블) — 새 변형이 발견되면
/// 여기 추가한다.
const CONSUMED_TYPE_NAMES: &[&str] = &[
    // Text — arboard 는 `NSPasteboardTypeString`("public.utf8-plain-text")만 읽지만,
    // 레거시/동의어 표현도 함께 온다.
    "public.utf8-plain-text",
    "public.utf16-plain-text",
    "public.plain-text",
    "NSStringPboardType",
    // Files — arboard 는 `readObjects`(NSURL, `NSPasteboardURLReadingFileURLsOnlyKey`)로
    // 읽는다, 밑단 UTI 는 `public.file-url`.
    "public.file-url",
    "NSFilenamesPboardType",
    // Image — arboard 는 `NSPasteboardTypeTIFF`만 읽지만 PNG/JPEG 도 같은 semantic.
    "public.tiff",
    "public.png",
    "public.jpeg",
    "NSTIFFPboardType",
    "NSPICTPboardType",
    // Html
    "public.html",
    "NSHTMLPboardType",
];

pub(super) fn read_other() -> Vec<OtherFormatEntry> {
    let pasteboard = NSPasteboard::generalPasteboard();
    let Some(types) = pasteboard.types() else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for ty in types.iter() {
        let name = ty.to_string();
        if CONSUMED_TYPE_NAMES.contains(&name.as_str()) {
            continue;
        }
        match pasteboard.dataForType(&ty) {
            Some(data) => {
                let bytes = data.to_vec();
                out.push(OtherFormatEntry::from_bytes(name, &bytes, MAX_RAW_BYTES));
            }
            // 타입 목록(TARGETS 상당)과 개별 재조회 사이 소유자가 바뀌는 race, 혹은
            // 이 프로세스가 접근 못 하는 타입 — 이 포맷만 건너뛴다(개별 격리).
            None => tracing::debug!("clipboard pasteboard dataForType({name}) returned nil"),
        }
    }
    out
}
