//! 문자 하나가 터미널 셀을 몇 칸 차지하는가.
//!
//! 렌더러 · 선택 모델 · 링크 스캐너가 **같은 답**을 써야 열과 화면이 어긋나지 않는다.
//! 종전에는 이 함수가 GPU 렌더러 모듈 안에 있어서, 셀 열을 세야 하는 도메인·입력 쪽
//! 코드가 렌더러를 거꾸로 들여다봤다. 답을 잎으로 내려 그 방향을 없앤다.
//!
//! 판정은 코드포인트 구간표다 — 외부 `unicode-width` 크레이트와 결과가 같다는 보장은
//! 없고, 여기서는 **옮기기 전과 같은 답**을 내는 것이 계약이다.

/// Check if a character is a wide (2-cell) character (CJK, fullwidth, etc.)
pub fn unicode_width(ch: char) -> usize {
    // CJK Unified Ideographs, Hangul, Fullwidth forms, etc.
    let cp = ch as u32;
    if (0x1100..=0x115F).contains(&cp)     // Hangul Jamo
        || (0x2E80..=0x303E).contains(&cp) // CJK Radicals, Kangxi, CJK Symbols
        || (0x3040..=0x33BF).contains(&cp) // Hiragana, Katakana, CJK Compat
        || (0x3400..=0x4DBF).contains(&cp) // CJK Extension A
        || (0x4E00..=0x9FFF).contains(&cp) // CJK Unified Ideographs
        || (0xA000..=0xA4CF).contains(&cp) // Yi
        || (0xAC00..=0xD7AF).contains(&cp) // Hangul Syllables
        || (0xF900..=0xFAFF).contains(&cp) // CJK Compat Ideographs
        || (0xFE30..=0xFE4F).contains(&cp) // CJK Compat Forms
        || (0xFF01..=0xFF60).contains(&cp) // Fullwidth Forms
        || (0xFFE0..=0xFFE6).contains(&cp) // Fullwidth Signs
        || (0x20000..=0x2FA1F).contains(&cp) // CJK Extensions B-F, Compat Supplement
        || (0x30000..=0x3134F).contains(&cp)
    // CJK Extension G
    {
        2
    } else {
        1
    }
}
