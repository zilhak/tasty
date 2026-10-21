//! 원문은 보존하고 그릴 때만 쓰는 NFC 문자열과 강조 범위 대응.

use std::borrow::Cow;
use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

/// 표시용 NFC. 이미 NFC이면 원문을 빌린다. 저장·검색·편집 값에는 적용하지 않는다.
pub fn nfc(text: &str) -> Cow<'_, str> {
    icu_normalizer::ComposingNormalizerBorrowed::new_nfc().normalize(text)
}

/// 원문 일치가 합성으로 바뀐 grapheme 일부에 걸리면 그 표시 묶음 전체를 강조한다.
/// 바뀌지 않은 묶음은 내부 바이트 범위를 보존해 이미 NFC인 이름의 강조를 바꾸지 않는다.
pub(crate) fn highlighted(
    text: &str,
    range: Option<Range<usize>>,
) -> (Cow<'_, str>, Option<Range<usize>>) {
    let display = nfc(text);
    let Some(range) = range else {
        return (display, None);
    };
    if matches!(display, Cow::Borrowed(_)) {
        return (display, Some(range));
    }
    let mut offset = 0;
    let mut mapped: Option<Range<usize>> = None;
    for (start, grapheme) in text.grapheme_indices(true) {
        let end = start + grapheme.len();
        let normalized = nfc(grapheme);
        if start < range.end && end > range.start {
            let span = if normalized == grapheme {
                offset + range.start.saturating_sub(start)..offset + (range.end.min(end) - start)
            } else {
                offset..offset + normalized.len()
            };
            match &mut mapped {
                Some(mapped) => mapped.end = span.end,
                None => mapped = Some(span),
            }
        }
        offset += normalized.len();
    }
    (display, mapped)
}
