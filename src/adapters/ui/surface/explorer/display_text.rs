//! 파일 이름·내용을 화면에 그릴 때만 쓰는 문자열 변환.
//! epaint는 글자마다 글리프 하나를 놓아 결합 문자(`e`+U+0301, 한글 첫가끝)를 앞 글자에
//! 합치지 않는다. 그리기 직전에 NFC로 합성해 미리 합성된 글자로 보이게 한다. 경로·복사·검색은
//! 원래 문자열을 그대로 쓴다. 합성형이 없는 결합 순서는 여전히 따로 그려진다.

use std::borrow::Cow;

/// 표시용 NFC 문자열. 이미 NFC면 빌려 온 그대로 돌려준다.
pub(crate) fn nfc(text: &str) -> Cow<'_, str> {
    icu_normalizer::ComposingNormalizerBorrowed::new_nfc().normalize(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decomposed_latin_and_hangul_are_composed() {
        assert_eq!(nfc("cafe\u{301}"), "caf\u{e9}");
        assert_eq!(
            nfc("\u{1100}\u{1161}\u{1112}\u{1161}\u{11ab}"),
            "\u{ac00}\u{d55c}"
        );
    }

    #[test]
    fn composed_text_is_borrowed_unchanged() {
        let text = "caf\u{e9} \u{ac00} plain";
        assert!(matches!(nfc(text), Cow::Borrowed(s) if s == text));
    }

    #[test]
    fn a_sequence_without_a_composed_form_stays_as_is() {
        // q + U+0301 은 합성형이 없다.
        assert_eq!(nfc("q\u{301}"), "q\u{301}");
    }
}
