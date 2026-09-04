use std::sync::LazyLock;

/// Maximum size of the output buffer (1 MB).
const OUTPUT_BUFFER_MAX: usize = 1_048_576;

/// Raw PTY output buffer with dual read-mark support.
///
/// Tracks two independent marks:
/// - `read_mark`: owned by the IPC `terminal.read_since_mark` API
/// - `scan_mark`: owned by the ClaudeError output scanner
pub(crate) struct OutputBuffer {
    buffer: Vec<u8>,
    read_mark: Option<usize>,
    scan_mark: usize,
}

impl OutputBuffer {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            read_mark: None,
            scan_mark: 0,
        }
    }

    /// Append raw bytes and trim to max size, adjusting marks as needed.
    pub fn append(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
        if self.buffer.len() > OUTPUT_BUFFER_MAX {
            let excess = self.buffer.len() - OUTPUT_BUFFER_MAX;
            self.buffer.drain(..excess);
            // Adjust read mark
            if let Some(mark) = &mut self.read_mark {
                if *mark <= excess {
                    self.read_mark = None; // mark was in trimmed region, invalidate
                } else {
                    *mark -= excess;
                }
            }
            // Adjust scan mark
            self.scan_mark = self.scan_mark.saturating_sub(excess);
        }
    }

    /// Set a read mark at the current end of the buffer.
    pub fn set_mark(&mut self) {
        self.read_mark = Some(self.buffer.len());
    }

    /// Read output since the last mark. If no mark was set, reads from the beginning.
    pub fn read_since_mark(&self, strip_ansi: bool) -> String {
        let start = self.read_mark.unwrap_or(0).min(self.buffer.len());
        let bytes = &self.buffer[start..];
        let text = String::from_utf8_lossy(bytes).to_string();
        if strip_ansi {
            strip_ansi_escapes(&text)
        } else {
            text
        }
    }

    /// Return raw bytes accumulated since the last `set_scan_mark()` call.
    /// Used by the ClaudeError scanner; independent of `read_since_mark`'s mark.
    pub fn output_since_scan_mark(&self, strip_ansi: bool) -> String {
        let start = self.scan_mark.min(self.buffer.len());
        let bytes = &self.buffer[start..];
        let text = String::from_utf8_lossy(bytes).to_string();
        if strip_ansi {
            strip_ansi_escapes(&text)
        } else {
            text
        }
    }

    /// Advance the scan mark to the current end of the buffer.
    pub fn set_scan_mark(&mut self) {
        self.scan_mark = self.buffer.len();
    }
}

/// CSI 파라미터 바이트는 ECMA-48 이 `0x30-0x3F` 로 정한 **16 글자 전부**다 —
/// `0-9 : ; < = > ?`. 문자군은 그 범위를 그대로 적는다(`[0-9:;<=>?]`).
/// `?` 는 private mode(`\x1b[?25l` 커서 숨김, `\x1b[?1049h` alt 스크린,
/// `\x1b[?2004h` bracketed paste), `:` 는 서브파라미터(곱슬 밑줄 `\x1b[4:3m`,
/// 콜론형 truecolor `\x1b[38:2::R:G:Bm`), `< = >` 는 private parameter prefix
/// (SGR 마우스 `\x1b[<0;12;3M`, DA 질의 `\x1b[=1c`, modifyOtherKeys `\x1b[>4;1m`).
/// 종결 바이트도 알파벳만이 아니다(`@-~`). 문자군이 좁으면 그 시퀀스들이 **그대로
/// 남아**, `strip_ansi` 를 켠 호출자가 여전히 escape 가 섞인 텍스트를 받는다.
///
/// 같은 정규식이 `tasty-output` 의 `strip_ansi` 에도 있다. 두 사본은 **문자 단위로
/// 같아야** 하고, 그 동등은 `tests/strip_ansi_regex_parity.rs` 가 강제한다 —
/// 주석으로 적어두는 것만으로는 한쪽만 고치는 것을 막지 못한다. 하나로 합치는 것은
/// 두 크레이트의 의존 방향 문제라 따로 다룬다.
static ANSI_ESCAPE_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\x1b\[[0-9:;<=>?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)")
        .expect("static regex is valid")
});

/// Strip ANSI escape sequences from a string using regex.
fn strip_ansi_escapes(s: &str) -> String {
    ANSI_ESCAPE_RE.replace_all(s, "").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `strip_ansi` 를 켜고도 escape 가 남아 있던 형태들. 전부 **private mode**(`?`)라
    /// 예전 문자군 `[0-9;]` 에 걸리지 않았다. 에이전트가 화면을 되읽어 문구를 찾을 때
    /// 이 잔재가 섞이면 매칭이 어긋난다 — 실제로 그렇게 잃은 적이 있다.
    #[test]
    fn private_mode_sequences_are_stripped() {
        assert_eq!(strip_ansi_escapes("\x1b[?25lhidden\x1b[?25h"), "hidden");
        assert_eq!(strip_ansi_escapes("\x1b[?1049halt"), "alt");
        assert_eq!(strip_ansi_escapes("\x1b[?2004hpaste\x1b[?2004l"), "paste");
    }

    /// 종결 바이트가 알파벳이 아닌 CSI 도 있다(`@-~`). 예전 `[a-zA-Z]` 는 못 잡았다.
    #[test]
    fn a_csi_with_a_non_alphabetic_final_byte_is_stripped() {
        assert_eq!(strip_ansi_escapes("\x1b[1@x"), "x");
        assert_eq!(strip_ansi_escapes("\x1b[2`y"), "y");
    }

    /// ECMA-48 의 CSI 파라미터 바이트는 `0x30-0x3F` **전체**다 — `0-9 : ; < = > ?`.
    /// `:` 는 서브파라미터 구분자(곱슬 밑줄 `\x1b[4:3m`, 콜론형 truecolor
    /// `\x1b[38:2::R:G:Bm`, 밑줄 색 `\x1b[58:2::R:G:Bm`)이고, `< = >` 는 private
    /// parameter prefix(SGR 마우스 `\x1b[<0;12;3M`, DA 질의 `\x1b[=1c`,
    /// modifyOtherKeys `\x1b[>4;1m`)다. 이론적인 형태가 아니다 — vim/nvim 은 뜰 때마다
    /// `\x1b[>4;1m` 을 방출하고, 최근 rustc/gcc 진단과 delta 는 `4:3` 곱슬 밑줄을 쓴다.
    ///
    /// 이 잔재는 **눈으로 훑으면 놓친다**: 닫는 `\x1b[0m` 은 파라미터가 없어 문자군과
    /// 무관하게 지워지므로, 여는 escape 한쪽에만 남는다.
    #[test]
    fn csi_parameter_bytes_beyond_digits_are_stripped() {
        assert_eq!(strip_ansi_escapes("\x1b[4:3mwavy\x1b[0m"), "wavy");
        assert_eq!(strip_ansi_escapes("\x1b[38:2::255:0:0mred\x1b[0m"), "red");
        assert_eq!(strip_ansi_escapes("\x1b[58:2::255:0:0mu\x1b[0m"), "u");
        assert_eq!(strip_ansi_escapes("\x1b[>4;1mx"), "x");
        assert_eq!(strip_ansi_escapes("\x1b[=1cy"), "y");
        assert_eq!(strip_ansi_escapes("\x1b[<0;12;3Mz"), "z");
    }

    /// 원래 잡던 것들은 그대로 잡는다 — 넓힌 것이 좁힌 것이 되지 않게.
    #[test]
    fn sgr_and_osc_are_still_stripped() {
        assert_eq!(strip_ansi_escapes("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(strip_ansi_escapes("\x1b]0;title\x07after"), "after");
        assert_eq!(strip_ansi_escapes("\x1b]8;;http://x\x1b\\link"), "link");
    }

    /// escape 가 없는 텍스트는 그대로 통과한다. `?` 를 파라미터로 받아들이게 했다고
    /// 본문의 물음표가 지워지면 안 된다.
    #[test]
    fn plain_text_including_question_marks_is_untouched() {
        assert_eq!(
            strip_ansi_escapes("really? yes [0-9] ok"),
            "really? yes [0-9] ok"
        );
        // 파라미터 문자군을 `0x30-0x3F` 전체로 넓혔다고 본문의 `: < = >` 가 지워지면
        // 안 된다 — 문자군은 `\x1b[` 뒤에서만 의미가 있다.
        assert_eq!(
            strip_ansi_escapes("a<b >c =d ratio 3:4"),
            "a<b >c =d ratio 3:4"
        );
    }
}
