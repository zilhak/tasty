//! 소스에서 함수 본문과 match 분기의 바이트 구간을 읽는다.
//! 구분자는 주석·리터럴을 가린 사본에서 찾고 실제 값은 같은 위치의 원문에서 읽는다.
//! 파일 전체를 source_text로 마스킹해 여러 줄 문자열과 중첩 주석을 처리한다.
//!
//! Rust 문법 전체를 파싱하지 않는다. 본문 안의 중첩 match·클로저·매크로는 현재 분기의
//! 본문으로 묶는다. 지원하지 않는 쉼표 생략 등은 다음 분기를 누락하지 않도록 Err로 알린다.
//! 추가 의존성 없이 여러 검사에서 같은 구현을 사용하기 위해 별도의 syn 파서를 두지 않는다.

use std::ops::Range;

use crate::source_text::{mask_comments_aligned, mask_non_code_aligned};

/// 원본과, 바이트 위치가 같은 두 사본.
pub struct Source<'a> {
    /// 원본 텍스트.
    pub text: &'a str,
    /// 주석·문자열·문자 리터럴을 바이트 수까지 맞춰 공백으로 덮은 사본 — 구조를 여기서 찾는다.
    pub code: String,
    /// 주석만 덮은 사본 — 여기서 공백인 바이트가 건너뛸 수 있는 자리(공백·주석)다. `code` 로
    /// 건너뛰면 덮인 문자열 리터럴까지 공백으로 보고 넘어가 이름을 잃는다.
    trivia: String,
}

/// `match` 팔 하나의 구간들. 모두 원본 기준 바이트 구간이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arm {
    /// 팔 앞의 속성(`#[cfg(...)]` …)을 뺀 패턴 — guard 는 들어 있지 않다.
    pub pattern: Range<usize>,
    /// ` if ` 뒤의 guard 식. 없으면 `None`.
    pub guard: Option<Range<usize>>,
    /// `=>` 뒤의 본문. 뒤따르는 쉼표는 들어 있지 않다.
    pub body: Range<usize>,
    /// 팔 앞의 속성들이 차지한 구간(없으면 빈 구간).
    pub attrs: Range<usize>,
}

impl<'a> Source<'a> {
    pub fn new(text: &'a str) -> Self {
        Source {
            text,
            code: mask_non_code_aligned(text),
            trivia: mask_comments_aligned(text),
        }
    }

    /// 원본의 구간 문자열.
    pub fn slice(&self, r: &Range<usize>) -> &'a str {
        &self.text[r.clone()]
    }

    /// 코드 사본의 구간 문자열.
    pub fn code_slice(&self, r: &Range<usize>) -> &str {
        &self.code[r.clone()]
    }

    /// `pos` 가 원본의 몇째 줄(1 부터)인가.
    pub fn line_of(&self, pos: usize) -> usize {
        self.text[..pos].matches('\n').count() + 1
    }

    /// 같은 이름의 fn 본문을 모두 반환한다. cfg로 나뉜 정의도 포함하며 주석·리터럴은 제외한다.
    pub fn fn_bodies(&self, name: &str) -> Vec<Range<usize>> {
        let code = self.code.as_str();
        let mut out = Vec::new();
        let mut from = 0;
        while let Some(pos) = code[from..].find("fn ") {
            let at = from + pos;
            from = at + 3;
            let before_ok = !code[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
            let rest = code[at + 3..].trim_start();
            let name_at = code.len() - rest.len();
            let after = rest.get(name.len()..).unwrap_or("");
            if !before_ok
                || !rest.starts_with(name)
                || !after.starts_with(|c: char| c == '(' || c == '<' || c.is_whitespace())
            {
                continue;
            }
            // 함수 이름 뒤에 처음 나오는 중괄호를 본문 시작으로 취급한다.
            let Some(open) = code[name_at..].find('{').map(|k| name_at + k) else {
                continue;
            };
            if code[name_at..open].contains(';') {
                // 본문 없는 선언(trait 항목)이다.
                continue;
            }
            if let Some(close) = matching_close(code, open) {
                out.push(open..close + 1);
            }
        }
        out
    }

    /// match 블록의 분기를 차례로 읽는다. 지원하지 않는 형태는 Err로 반환한다.
    pub fn match_arms(&self, block: Range<usize>) -> Result<Vec<Arm>, String> {
        let code = self.code.as_bytes();
        if code.get(block.start) != Some(&b'{') || block.end == 0 || code[block.end - 1] != b'}' {
            return Err(format!(
                "{}행: match 블록이 `{{ … }}` 가 아니다",
                self.line_of(block.start)
            ));
        }
        let close = block.end - 1;
        let mut arms = Vec::new();
        let mut i = self.skip_trivia(block.start + 1, close);
        while i < close {
            let arrow = self.arm_arrow(i, close)?;
            let (attrs_end, pattern, guard) = self.split_head(i..arrow)?;
            let body_start = self.skip_trivia(arrow + 2, close);
            let (body_end, next) = self.arm_body_end(body_start, close)?;
            arms.push(Arm {
                pattern,
                guard,
                body: body_start..body_end,
                attrs: i..attrs_end,
            });
            i = self.skip_trivia(next, close);
        }
        Ok(arms)
    }

    /// 패턴을 깊이 0 의 `|` 로 나눈 조각들(앞머리 `|` 가 만드는 빈 조각은 뺀다).
    pub fn alternatives(&self, pattern: &Range<usize>) -> Vec<Range<usize>> {
        let code = self.code.as_bytes();
        let mut out = Vec::new();
        let mut depth = 0usize;
        let mut start = pattern.start;
        for j in pattern.clone() {
            match code[j] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth = depth.saturating_sub(1),
                b'|' if depth == 0 => {
                    out.push(start..j);
                    start = j + 1;
                }
                _ => {}
            }
        }
        out.push(start..pattern.end);
        out.into_iter()
            .map(|r| self.trim(r))
            .filter(|r| !r.is_empty())
            .collect()
    }

    /// 구간이 **보통 문자열 리터럴 하나**(`"…"`, 이스케이프 없음)면 그 내용을 돌려준다.
    ///
    /// raw 문자열 · byte 문자열 · 상수 경로 · 매크로 · 바인딩은 `None` 이다. 사본에서 그
    /// 구간이 전부 공백이어야 하므로(= 통째로 덮인 리터럴) 앞뒤에 코드가 붙은 것도 `None`.
    pub fn plain_string(&self, r: &Range<usize>) -> Option<&'a str> {
        let r = self.trim(r.clone());
        let raw = self.slice(&r);
        let inner = raw.strip_prefix('"')?.strip_suffix('"')?;
        let covered = self.code_slice(&r).bytes().all(|b| b.is_ascii_whitespace());
        (covered && !inner.contains(['"', '\\'])).then_some(inner)
    }

    /// 앞뒤의 공백·주석을 뺀 구간.
    pub fn trim(&self, r: Range<usize>) -> Range<usize> {
        let s = &self.trivia[r.clone()];
        let start = r.start + (s.len() - s.trim_start().len());
        let end = r.end - (s.len() - s.trim_end().len());
        start..end.max(start)
    }

    /// `i` 부터 공백·주석을 건너뛴 위치(`end` 를 넘지 않는다).
    fn skip_trivia(&self, mut i: usize, end: usize) -> usize {
        let t = self.trivia.as_bytes();
        while i < end && t[i].is_ascii_whitespace() {
            i += 1;
        }
        i
    }

    /// `from` 에서 시작하는 팔 머리의 `=>` 위치.
    fn arm_arrow(&self, from: usize, close: usize) -> Result<usize, String> {
        let code = self.code.as_bytes();
        let mut depth = 0usize;
        let mut j = from;
        while j < close {
            match code[j] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth = depth.saturating_sub(1),
                b'=' if depth == 0 && code.get(j + 1) == Some(&b'>') => return Ok(j),
                b',' | b';' if depth == 0 => break,
                _ => {}
            }
            j += 1;
        }
        Err(format!(
            "{}행: 팔 머리에서 `=>` 를 못 찾았다",
            self.line_of(from)
        ))
    }

    /// 팔 머리를 (속성 끝, 패턴, guard) 로 가른다.
    fn split_head(&self, head: Range<usize>) -> Result<SplitHead, String> {
        let code = self.code.as_bytes();
        let mut i = self.skip_trivia(head.start, head.end);
        while code.get(i) == Some(&b'#') {
            let open = self.skip_trivia(i + 1, head.end);
            if code.get(open) != Some(&b'[') {
                return Err(format!("{}행: 팔 속성을 못 읽었다", self.line_of(i)));
            }
            let close = matching(code, open, b'[', b']')
                .filter(|c| *c < head.end)
                .ok_or_else(|| format!("{}행: 팔 속성이 닫히지 않는다", self.line_of(i)))?;
            i = self.skip_trivia(close + 1, head.end);
        }
        let attrs_end = i;
        // guard 는 깊이 0 의 `if` 낱말부터다(rustfmt 가 다음 줄로 내려도 같다).
        let mut depth = 0usize;
        let mut guard_at = None;
        for j in i..head.end {
            match code[j] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth = depth.saturating_sub(1),
                b'i' if depth == 0
                    && code[j..head.end].starts_with(b"if")
                    && j > i
                    && code[j - 1].is_ascii_whitespace()
                    && code
                        .get(j + 2)
                        .is_some_and(|c| c.is_ascii_whitespace() || *c == b'(') =>
                {
                    guard_at = Some(j);
                    break;
                }
                _ => {}
            }
        }
        Ok(match guard_at {
            Some(g) => (attrs_end, self.trim(i..g), Some(self.trim(g + 2..head.end))),
            None => (attrs_end, self.trim(i..head.end), None),
        })
    }

    /// 본문이 `start` 에서 시작할 때 (본문 끝, 다음 팔의 시작) 을 돌려준다.
    fn arm_body_end(&self, start: usize, close: usize) -> Result<(usize, usize), String> {
        let code = self.code.as_bytes();
        if code.get(start) == Some(&b'{') {
            let end = matching(code, start, b'{', b'}')
                .filter(|e| *e < close)
                .ok_or_else(|| {
                    format!("{}행: 팔 본문 블록이 닫히지 않는다", self.line_of(start))
                })?;
            let after = self.skip_trivia(end + 1, close);
            match code.get(after) {
                // 블록 본문 뒤의 쉼표는 있어도 없어도 된다.
                Some(b',') => return Ok((end + 1, after + 1)),
                // 블록에 이어지는 식(`{ … }.foo()` · `?`)이면 아래 일반 경로로 간다.
                Some(b'.' | b'?') => {}
                _ => return Ok((end + 1, after)),
            }
        }
        let mut depth = 0usize;
        let mut j = start;
        while j < close {
            match code[j] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth = depth.saturating_sub(1),
                b',' if depth == 0 => return Ok((self.trim(start..j).end, j + 1)),
                b'=' if depth == 0 && code.get(j + 1) == Some(&b'>') => {
                    return Err(format!(
                        "{}행: 팔 본문이 끝나기 전에 다음 팔의 `=>` 가 나왔다 — 블록형 식 뒤의 \
                         쉼표 생략으로 보인다. 이 판정기는 그 모양을 가르지 않는다",
                        self.line_of(start)
                    ));
                }
                _ => {}
            }
            j += 1;
        }
        Ok((self.trim(start..close).end, close))
    }
}

/// 팔 머리를 가른 결과 — (속성 끝, 패턴, guard).
type SplitHead = (usize, Range<usize>, Option<Range<usize>>);

/// `open` 의 `{` 와 짝인 `}` 의 위치(코드 사본 기준).
pub fn matching_close(code: &str, open: usize) -> Option<usize> {
    matching(code.as_bytes(), open, b'{', b'}')
}

fn matching(code: &[u8], open: usize, l: u8, r: u8) -> Option<usize> {
    let mut depth = 0usize;
    for (k, b) in code.iter().enumerate().skip(open) {
        if *b == l {
            depth += 1;
        } else if *b == r {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(k);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(src: &Source, arm: &Arm) -> Vec<Option<String>> {
        src.alternatives(&arm.pattern)
            .iter()
            .map(|r| src.plain_string(r).map(str::to_string))
            .collect()
    }

    const ROUTER: &str = r##"
fn route(m: &str) -> Option<u8> {
    Some(match m {
        // 주석 속 "x" => 는 팔이 아니다 }
        "a" => {
            tracing::trace!("closing }}");
            1
        }
        "b" | "c" => 2,
        "d" if m.len() == r#"x,"a" if "#.len() => 3,
        #[cfg(unix)]
        "e" => match m { "n" => 4, _ => 5 },
        '}' => 6,
        _ => return None,
    })
}
"##;

    #[test]
    fn arms_are_read_past_string_and_char_delimiters() {
        let src = Source::new(ROUTER);
        let bodies = src.fn_bodies("route");
        assert_eq!(bodies.len(), 1);
        let open = src.code[bodies[0].clone()].find("m {").unwrap() + bodies[0].start + 2;
        let close = matching_close(&src.code, open).unwrap();
        let arms = src.match_arms(open..close + 1).unwrap();
        let heads: Vec<_> = arms.iter().map(|a| names(&src, a)).collect();
        assert_eq!(
            heads,
            vec![
                vec![Some("a".into())],
                vec![Some("b".into()), Some("c".into())],
                vec![Some("d".into())],
                vec![Some("e".into())],
                vec![None],
                vec![None],
            ]
        );
        assert!(arms[2].guard.is_some());
        assert_eq!(src.slice(&arms[3].attrs).trim(), "#[cfg(unix)]");
        assert_eq!(src.slice(&arms[5].pattern), "_");
        assert_eq!(src.slice(&arms[5].body), "return None");
    }

    #[test]
    fn every_definition_is_found_and_mentions_are_not() {
        let text = "// fn f() {}\nconst S: &str = \"fn f() {\";\n#[cfg(a)]\nfn f() { 1 }\n#[cfg(b)]\nfn f() { 2 }\nfn ff() {}\n";
        let src = Source::new(text);
        let bodies: Vec<_> = src.fn_bodies("f").iter().map(|r| src.slice(r)).collect();
        assert_eq!(bodies, vec!["{ 1 }", "{ 2 }"]);
    }

    #[test]
    fn an_arm_swallowed_by_comma_omission_is_an_error_not_a_skip() {
        let text = "match m { \"a\" => if x { 1 } else { 2 } \"b\" => 3, _ => 0 }";
        let src = Source::new(text);
        assert!(src.match_arms(8..text.len()).is_err());
    }

    #[test]
    fn non_plain_literals_are_not_names() {
        let text = r#"match m { r"a" | b"b" | "c\"d" | CONST | "e" /* x */ => 1 }"#;
        let src = Source::new(text);
        let arms = src.match_arms(8..text.len()).unwrap();
        // 주석은 코드가 아니라 건너뛴다 — 뒤에 주석이 붙은 `"e"` 는 이름이다.
        assert_eq!(
            names(&src, &arms[0]),
            vec![None, None, None, None, Some("e".into())]
        );
    }

    #[test]
    fn multibyte_text_in_masked_regions_keeps_offsets() {
        let text = "match m { \"가\" => { f(\"}}나\") } \"다\" => 2 }";
        let src = Source::new(text);
        let arms = src.match_arms(8..text.len()).unwrap();
        let heads: Vec<_> = arms.iter().map(|a| names(&src, a)).collect();
        assert_eq!(
            heads,
            vec![vec![Some("가".into())], vec![Some("다".into())]]
        );
    }
}
