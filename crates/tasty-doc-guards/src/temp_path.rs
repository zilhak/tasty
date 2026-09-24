//! 공유 임시 디렉터리에서 고정 이름을 쓰면서 격리 수단이나 공유 사유를 적지 않은 코드를 찾는다(ADR-0045).
//! 프로세스 간 충돌과 같은 프로세스의 재호출을 구분한다. TempDir 계열은 호출별 이름,
//! pid는 프로세스 간 구분, 카운터는 프로세스 내 호출 구분으로 본다.
//! 재호출을 시계에만 의존하면 해상도에 따라 겹칠 수 있어 따로 보고한다.
//! 스레드 ID는 같은 스레드의 재호출을 구분하지 못하므로 유일성 수단으로 인정하지 않는다.
//! 의도한 공유나 시계 사용은 해당 위치에 이유:/reason:/사유: 주석으로 설명한다.
//!
//! temp_dir의 직접 .join 또는 같은 함수 안에서 그 값을 받은 변수의 .join을 찾는다.
//! 주변 성분과 변수의 바인딩을 읽고 선언 순서·블록 범위·shadowing을 고려한다.
//! 다른 함수에 넘긴 뒤 만드는 경로와 외부 도구가 정한 이름은 추적하지 못한다.
//! pid 재사용 뒤 남은 파일, 실제 정리 성공, 사유의 타당성도 이 검사로 보장하지 않는다.
//! 저장소 순회는 대상 수를 확인하고, 합성 입력은 과도한 허용·거부를 확인한다.

mod bindings;

#[cfg(test)]
mod scope_tests;

use std::path::Path;

use crate::source_text::{mask_literals, mask_non_code, rust_sources};

/// 경로에 있는 구분 수단. 프로세스 간 구분과 같은 프로세스의 재호출을 나눠 본다.
/// pid 재사용과 남은 파일의 충돌은 종료 방식·OS 정책·권한에 달려 있어 여기서 판단하지 않는다.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
struct Axes {
    /// 호출마다 다른 이름을 제공하는 성분.
    per_call: bool,
    /// 서로 다른 프로세스만 구분한다.
    process: bool,
    /// 시계 해상도에 의존하지 않고 같은 프로세스의 호출을 구분하는 카운터.
    counter: bool,
    /// 시계 해상도에 의존하는 호출 구분.
    clock: bool,
}

impl Axes {
    fn union(self, o: Self) -> Self {
        Self {
            per_call: self.per_call || o.per_call,
            process: self.process || o.process,
            counter: self.counter || o.counter,
            clock: self.clock || o.clock,
        }
    }

    /// 인정하는 구분 수단이 하나라도 있는지 확인한다.
    fn any(self) -> bool {
        self.per_call || self.process || self.counter || self.clock
    }

    /// 같은 프로세스의 재호출을 시계에만 의존하는지 확인한다. pid는 이 문제를 해결하지 못한다.
    fn clock_stands_alone(self) -> bool {
        self.clock && !self.per_call && !self.counter
    }

    /// 같은 프로세스의 재호출을 구분할 성분이 없는지 확인한다. 시계 사용은 clock_stands_alone에서 따로 본다.
    fn blind_to_recall(self) -> bool {
        self.process && !self.per_call && !self.counter && !self.clock
    }
}

/// 호출마다 유일한 이름을 **라이브러리가** 보증하는 성분.
const PER_CALL_TOKENS: &[&str] = &["TempDir", "tempfile", "tempdir(", "NamedTempFile"];

/// 프로세스 간 구분만 제공하는 성분. path_for는 pid와 호출자가 준 surface ID로 이름을 만든다.
/// 같은 pid·surface ID로 다시 호출하면 같은 경로이므로 호출별 이름으로 취급하지 않는다.
const PROCESS_TOKENS: &[&str] = &["process::id", "pid", "path_for"];

/// 프로세스 내부의 호출을 시계 해상도와 무관하게 구분하는 카운터 성분.
const COUNTER_TOKENS: &[&str] = &["fetch_add", "AtomicUsize"];

/// 재호출을 시계에만 맡겼는지 검사할 성분.
/// 카운터와 함께 쓰는 시계는 pid 재사용 뒤의 잔재를 구분할 수 있어 일괄 금지하지 않는다.
const CLOCK_TOKENS: &[&str] = &["nanos", "SystemTime"];

/// 스레드 ID는 보고에만 사용한다. 같은 스레드의 재호출이나 프로세스 간 충돌을 막지 못한다.
/// 스레드별 구분이 필요한 검사는 별도의 판정이 필요하며 현재 유일성 판정에는 포함하지 않는다.
const THREAD_TOKENS: &[&str] = &["thread::current", "ThreadId"];

/// 지역 변수에서도 인식할 전체 성분 목록. 각 성분 목록의 합집합과 같은지 검사한다.
const UNIQ_TOKENS: &[&str] = &[
    "TempDir",
    "tempfile",
    "tempdir(",
    "NamedTempFile",
    "path_for",
    "process::id",
    "pid",
    "fetch_add",
    "AtomicUsize",
    "nanos",
    "SystemTime",
];

/// 한 줄에 있는 구분 수단을 모은다.
fn axes_of(line: &str) -> Axes {
    Axes {
        per_call: PER_CALL_TOKENS.iter().any(|t| line.contains(t)),
        process: PROCESS_TOKENS.iter().any(|t| line.contains(t)),
        counter: COUNTER_TOKENS.iter().any(|t| line.contains(t)),
        clock: CLOCK_TOKENS.iter().any(|t| line.contains(t)),
    }
}

/// 의도한 공유나 제한을 설명하는 주석의 표지.
/// 고정 이름 공유, 재호출 구분이 필요 없는 이유, 시계 충돌을 허용하는 이유를 상황에 맞게 적는다.
/// 표지 뒤의 설명이 타당한지는 검증하지 않는다. 다른 사유 검사와 표지·위치 규칙이 같다고 가정하지 않는다.
const REASON_TOKENS: &[&str] = &["이유:", "reason:", "사유:"];

/// 경로 주변에서 유일성 성분을 찾는 줄 수. 경로 생성 여부는 별도 수신자 판정으로 확인한다.
const UNIQ_WINDOW: usize = 6;
/// 같은 줄이나 바로 앞의 연속된 주석 블록에서 사유를 찾는다. 빈 줄이나 코드에서 끊는다.
fn reason_is_attached(
    raw: &[&str],
    comments: &[&str],
    idx: usize,
    has_token: impl Fn(&str) -> bool,
) -> bool {
    if has_token(comments[idx]) {
        return true;
    }
    let mut j = idx;
    while j > 0 {
        j -= 1;
        if !raw[j].trim_start().starts_with("//") {
            return false;
        }
        if has_token(comments[j]) {
            return true;
        }
    }
    false
}

/// 한 파일을 분류한 결과. 줄 번호는 0 기반(`temp_dir()` 이 있는 줄).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FileClass {
    /// 수신자 추적으로 경로를 만드는 .join과 연결한 temp_dir 호출.
    pub sites: Vec<usize>,
    /// 그중 유니크화된 줄.
    pub uniquified: Vec<usize>,
    /// 그중 사유로 공유가 명시된 줄.
    pub reasoned: Vec<usize>,
    /// 그중 유니크화도 사유도 없는 줄 — 고정 이름 공유(위반).
    pub silent: Vec<usize>,
    /// 같은 프로세스의 호출을 시계만으로 구분하고 그 선택의 사유가 없는 줄.
    pub weak_only: Vec<usize>,
    /// 같은 프로세스의 재호출을 구분할 성분도 사유도 없는 줄.
    pub recall_blind: Vec<usize>,
    /// recall_blind 중 스레드 ID도 사용하는 항목. 보고만 세분화하며 재호출 판정은 그대로다.
    pub recall_blind_with_thread: Vec<usize>,
    /// temp_dir에서 이어지는 .join을 찾지 못한 항목. 읽기 전용 사용과 외부로 전달된 값을 함께 포함한다.
    /// 외부 함수에서 경로를 만드는지는 판단하지 못한다.
    pub unpaired: Vec<usize>,
}

/// masked 코드 줄들과 masked 주석 줄들로 temp 경로 자리를 분류한다.
///
/// `code` 는 [`mask_non_code`](crate::source_text::mask_non_code)(주석·문자열 덮음),
/// `comments` 는 [`mask_literals`](crate::source_text::mask_literals)(문자열만 덮고 주석은
/// 남김)의 결과다 — 앞은 코드 토큰, 뒤는 사유 마커를 읽는다. 둘 다 줄 수가 같아야 한다.
pub fn classify(code: &[&str], comments: &[&str], raw: &[&str]) -> FileClass {
    assert_eq!(code.len(), comments.len(), "두 마스크의 줄 수가 다르다");
    assert_eq!(code.len(), raw.len(), "raw 줄 수가 다르다");
    // 지역 변수의 성분을 선언 순서·유효 범위·shadowing에 맞춰 전달한다.
    let bindings = bindings::Bindings::new(code, raw, comments);
    let mut out = FileClass::default();
    for idx in 0..code.len() {
        if !code[idx].contains("temp_dir()") {
            continue;
        }
        let path_lines = path_building_lines(code, idx);
        if path_lines.is_empty() {
            out.unpaired.push(idx);
            continue;
        }
        out.sites.push(idx);

        // 인라인 format 변수는 문자열에 있으므로 원문에서 찾는다.
        let span = axes_span(code.len(), idx, &path_lines);
        let mut axes = Axes::default();
        for &j in &span {
            axes = axes.union(bindings.axes_on_line(idx, j));
        }
        if axes.any() {
            out.uniquified.push(idx);
            // 유일성 성분이 있어도 시계 선택의 사유는 따로 확인한다.
            let reasoned = reason_is_attached(raw, comments, idx, |line| {
                REASON_TOKENS.iter().any(|t| line.contains(t))
            });
            if axes.clock_stands_alone() && !reasoned {
                out.weak_only.push(idx);
            }
            // reasoned는 성분 없는 항목에만 적용되므로 이 경우의 사유는 별도로 확인한다.
            if axes.blind_to_recall() && !reasoned {
                out.recall_blind.push(idx);
                if span
                    .iter()
                    .any(|&j| THREAD_TOKENS.iter().any(|t| code[j].contains(t)))
                {
                    out.recall_blind_with_thread.push(idx);
                }
            }
            continue;
        }
        let reasoned = reason_is_attached(&raw, &comments, idx, |line| {
            REASON_TOKENS.iter().any(|t| line.contains(t))
        });
        if reasoned {
            out.reasoned.push(idx);
        } else {
            out.silent.push(idx);
        }
    }
    out
}

/// temp_dir의 직접 .join과 같은 함수 안에서 그 값을 받은 변수의 .join을 찾는다.
/// 다른 값의 join을 섞지 않고 판정에 사용한 줄을 반환한다. 빈 결과면 경로 생성으로 세지 않는다.
fn path_building_lines(code: &[&str], idx: usize) -> Vec<usize> {
    let end = statement_end(code, idx);
    let lines: Vec<usize> = (idx..=end).collect();
    let (flat, _) = flatten_with_map(code, idx, end);
    if let Some(at) = flat.find("temp_dir()")
        && flat[at + "temp_dir()".len()..].contains(".join(")
    {
        return lines;
    }
    let Some(name) = let_binding_name(code, idx, end) else {
        return Vec::new();
    };
    if end + 1 >= code.len() {
        return Vec::new();
    }
    let hi = enclosing_fn(code, idx).map_or(code.len() - 1, |s| enclosing_fn_end(code, s));
    if end + 1 > hi {
        return Vec::new();
    }
    let (flat, map) = flatten_with_map(code, end + 1, hi);
    let pat = format!("{name}.join(");
    let bytes = flat.as_bytes();
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(rel) = flat[from..].find(&pat) {
        let at = from + rel;
        if at == 0 || !is_word_byte(bytes[at - 1]) {
            found.push(map[at]);
        }
        from = at + 1;
    }
    if found.is_empty() {
        return Vec::new();
    }
    let mut out = lines;
    out.extend(found);
    out
}

/// 경로 생성 줄과 주변 UNIQ_WINDOW 범위를 합쳐 구분 수단을 찾는다.
fn axes_span(len: usize, idx: usize, path_lines: &[usize]) -> Vec<usize> {
    let hi = (idx + UNIQ_WINDOW).min(len - 1);
    let mut out: Vec<usize> = (idx..=hi).collect();
    for &j in path_lines {
        if j < len && !out.contains(&j) {
            out.push(j);
        }
    }
    out
}

/// 이 줄에서 시작하는 문이 끝나는 줄 — 첫 `;` 가 있는 줄. 없으면 마지막 줄.
fn statement_end(code: &[&str], idx: usize) -> usize {
    (idx..code.len())
        .find(|&j| code[j].contains(';'))
        .unwrap_or(code.len() - 1)
}

/// `fn` 선언 줄부터 중괄호를 세어 그 본문이 끝나는 줄. 못 닫으면 마지막 줄.
fn enclosing_fn_end(code: &[&str], fn_start: usize) -> usize {
    let mut depth = 0i32;
    let mut opened = false;
    for (j, line) in code.iter().enumerate().skip(fn_start) {
        for ch in line.chars() {
            if ch == '{' {
                depth += 1;
                opened = true;
            } else if ch == '}' {
                depth -= 1;
                if opened && depth <= 0 {
                    return j;
                }
            }
        }
    }
    code.len() - 1
}

/// 줄을 공백 없이 이어 붙이고 각 바이트의 원래 줄 번호도 반환한다. 줄바꿈을 넘는 .join 판독에 사용한다.
fn flatten_with_map(code: &[&str], lo: usize, hi: usize) -> (String, Vec<usize>) {
    let mut flat = String::new();
    let mut map = Vec::new();
    for (j, line) in code.iter().enumerate().take(hi + 1).skip(lo) {
        for ch in line.chars() {
            if ch.is_whitespace() {
                continue;
            }
            let before = flat.len();
            flat.push(ch);
            for _ in before..flat.len() {
                map.push(j);
            }
        }
    }
    (flat, map)
}

/// `let [mut] <name> = ...` 의 이름. `let` 이 `temp_dir()` 보다 앞에 있을 때만 인정한다.
fn let_binding_name(code: &[&str], lo: usize, hi: usize) -> Option<String> {
    let text = code[lo..=hi].join(" ");
    let at_let = text.find("let ")?;
    let at_temp = text.find("temp_dir()")?;
    if at_let > at_temp {
        return None;
    }
    let rest = text[at_let + 4..].trim_start();
    let rest = rest.strip_prefix("mut ").unwrap_or(rest);
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// `line` 이 `word` 를 **단어 경계로** 포함하는가(부분 문자열 오인 방지 — `id` 가
/// `width` 안에서 매칭되지 않게).
fn references_word(line: &str, word: &str) -> bool {
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(rel) = line[from..].find(word) {
        let start = from + rel;
        let end = start + word.len();
        let before_ok = start == 0 || !is_word_byte(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !is_word_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// 워크스페이스 전체 검사 결과.
#[derive(Debug, Default)]
pub struct Census {
    pub files_scanned: usize,
    pub sites: usize,
    pub uniquified: usize,
    pub reasoned: usize,
    /// 창 안에 `.join(` 이 없어 자리로 안 세어진 `temp_dir()` 줄의 수.
    pub unpaired: usize,
    /// `"레포상대경로:1기반줄: 원문"` 형태의 위반 목록.
    pub silent: Vec<String>,
    /// 약한 성분에만 기댔고 그 선택을 안 밝힌 자리 — 같은 형태의 목록.
    pub weak_only: Vec<String>,
    /// 재호출을 가르는 축이 비었고 그 선택을 안 밝힌 자리 — 같은 형태의 목록.
    pub recall_blind: Vec<String>,
    /// recall_blind 중 스레드 ID도 사용하는 항목.
    pub recall_blind_with_thread: Vec<String>,
}

/// scan_roots 아래에서 경로를 분류한다.
pub fn census(root: &Path, scan_roots: &[&str]) -> Census {
    let sources = rust_sources(root, scan_roots);
    let mut c = Census::default();
    for (rel, raw) in &sources {
        c.files_scanned += 1;
        let code_src = mask_non_code(raw);
        let comment_src = mask_literals(raw);
        let code: Vec<&str> = code_src.lines().collect();
        let comments: Vec<&str> = comment_src.lines().collect();
        let raw_lines: Vec<&str> = raw.lines().collect();
        let fc = classify(&code, &comments, &raw_lines);

        c.sites += fc.sites.len();
        c.uniquified += fc.uniquified.len();
        c.reasoned += fc.reasoned.len();
        c.unpaired += fc.unpaired.len();
        for &idx in &fc.silent {
            let text = raw_lines.get(idx).map(|s| s.trim()).unwrap_or("");
            c.silent
                .push(format!("{}:{}: {text}", rel.display(), idx + 1));
        }
        for &idx in &fc.weak_only {
            let text = raw_lines.get(idx).map(|s| s.trim()).unwrap_or("");
            c.weak_only
                .push(format!("{}:{}: {text}", rel.display(), idx + 1));
        }
        for &idx in &fc.recall_blind {
            let text = raw_lines.get(idx).map(|s| s.trim()).unwrap_or("");
            c.recall_blind
                .push(format!("{}:{}: {text}", rel.display(), idx + 1));
        }
        for &idx in &fc.recall_blind_with_thread {
            let text = raw_lines.get(idx).map(|s| s.trim()).unwrap_or("");
            c.recall_blind_with_thread
                .push(format!("{}:{}: {text}", rel.display(), idx + 1));
        }
    }
    c
}

// 호출자가 경로 판별자를 넘기는 경우 같은 파일의 호출을 따라 중복 인자식을 찾는다.

/// 판별자를 호출자에게서 받는 한 함수의 호출 실태.
#[derive(Debug, PartialEq, Eq)]
pub struct Chain {
    /// `"Tmp::new"` 처럼 호출에 쓰이는 이름(impl 이면 타입을 앞에 붙인다).
    pub callee: String,
    /// 판별자 자리에 넘어온 인자식 **텍스트** 목록(공백 정규화).
    pub args: Vec<String>,
    /// 그중 두 번 이상 나온 텍스트 — **이것이 위반이다.**
    pub duplicates: Vec<String>,
    /// 이 함수의 호출을 **이 파일에서 하나도 못 찾은** 경우 true.
    ///
    /// 통과가 아니다. 호출이 다른 파일에 있으면 이 판정은 그 자리를 **안 본 것**이다.
    pub no_call_seen: bool,
}

/// 경로 판별자를 매개변수로 받는 recall_blind 함수의 호출을 찾는다.
/// 인자가 호출자의 매개변수이면 더 거슬러 올라간다. 다른 파일의 호출은 찾지 못한다.
/// 호출 위치는 마스킹한 코드에서, 문자열 판별자 값은 원문에서 읽는다.
pub fn discriminator_chains(code: &[&str], raw: &[&str]) -> Vec<Chain> {
    let fc = classify(code, &mask_of_comments(raw), raw);
    let mut todo: Vec<(String, usize)> = Vec::new(); // (호출 이름, 판별자 인자 자리)
    for &idx in &fc.recall_blind {
        let Some(fj) = enclosing_fn(code, idx) else {
            continue;
        };
        let params = fn_params(code[fj]);
        let span = axes_span(raw.len(), idx, &path_building_lines(code, idx));
        let win = span.iter().map(|&j| raw[j]).collect::<Vec<_>>().join("\n");
        let Some(pos) = params.iter().position(|p| references_word(&win, p)) else {
            continue;
        };
        let name = call_name(code, fj);
        if !todo.iter().any(|(n, q)| n == &name && *q == pos) {
            todo.push((name, pos));
        }
    }
    let mut out = Vec::new();
    let mut seen = 0;
    while seen < todo.len() {
        let (name, pos) = todo[seen].clone();
        seen += 1;
        let mut args = Vec::new();
        for j in 0..code.len() {
            let Some(a) = nth_arg(code, raw, j, &name, pos) else {
                continue;
            };
            args.push(a);
        }
        let mut dups: Vec<String> = Vec::new();
        for a in &args {
            if args.iter().filter(|b| *b == a).count() > 1 && !dups.contains(a) {
                dups.push(a.clone());
            }
        }
        // 인자가 그 호출자의 매개변수를 담고 있으면 한 단계 위로.
        for j in 0..code.len() {
            if nth_arg(code, raw, j, &name, pos).is_none() {
                continue;
            }
            let Some(fj) = enclosing_fn(code, j) else {
                continue;
            };
            let a = nth_arg(code, raw, j, &name, pos).unwrap_or_default();
            let params = fn_params(code[fj]);
            if let Some(q) = params.iter().position(|p| references_word(&a, p)) {
                let up = call_name(code, fj);
                if !todo.iter().any(|(n, r)| n == &up && *r == q) {
                    todo.push((up, q));
                }
            }
        }
        out.push(Chain {
            no_call_seen: args.is_empty(),
            callee: name,
            args,
            duplicates: dups,
        });
    }
    out
}

/// 사유가 있어도 판별자 중복 검사에 포함되도록 주석 입력을 비운다.
fn mask_of_comments(raw: &[&str]) -> Vec<&'static str> {
    vec![""; raw.len()]
}

fn is_fn_decl(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("fn ")
        || t.starts_with("pub fn ")
        || t.starts_with("pub(crate) fn ")
        || t.starts_with("async fn ")
        || t.starts_with("pub async fn ")
}

fn enclosing_fn(code: &[&str], idx: usize) -> Option<usize> {
    let mut j = idx;
    while j > 0 {
        j -= 1;
        if is_fn_decl(code[j]) {
            return Some(j);
        }
    }
    None
}

/// `fn f(a: &str, b: u32)` → `["a", "b"]`.
fn fn_params(line: &str) -> Vec<String> {
    let Some(o) = line.find('(') else {
        return Vec::new();
    };
    let rest = &line[o + 1..];
    let c = rest.rfind(')').unwrap_or(rest.len());
    split_top(&rest[..c])
        .into_iter()
        .filter_map(|p| {
            p.split(':')
                .next()
                .map(|n| n.trim().trim_start_matches("mut ").to_string())
        })
        .filter(|n| !n.is_empty() && n != "&self" && n != "self")
        .collect()
}

/// 호출에 쓰이는 이름. `impl Tmp` 안의 `fn new` 이면 `"Tmp::new"`.
fn call_name(code: &[&str], fj: usize) -> String {
    let t = code[fj].trim_start();
    let after = t.split("fn ").nth(1).unwrap_or("");
    let bare: String = after
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let mut j = fj;
    while j > 0 {
        j -= 1;
        let l = code[j].trim_start();
        if l.starts_with("impl ") {
            let ty = l
                .trim_start_matches("impl ")
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches('{')
                .to_string();
            if !ty.is_empty() {
                return format!("{ty}::{bare}");
            }
            break;
        }
        if is_fn_decl(l) {
            break;
        }
    }
    bare
}

/// `j` 줄이 `name(` 호출이면 `pos` 번째 인자식 텍스트를 원문에서 읽어 돌려준다.
fn nth_arg(code: &[&str], raw: &[&str], j: usize, name: &str, pos: usize) -> Option<String> {
    let pat = format!("{name}(");
    let at = code[j].find(&pat)?;
    // 이름 앞이 단어 문자면 다른 이름의 꼬리다(`with_new(` 안의 `new(`).
    if at > 0 && is_word_byte(code[j].as_bytes()[at - 1]) {
        return None;
    }
    // 선언과 호출이 같은 줄에 있을 수 있어 해당 위치 바로 앞의 fn만 제외한다.
    if code[j][..at].trim_end().ends_with("fn") {
        return None;
    }
    let line = raw.get(j)?;
    let at_raw = line.find(&pat)?;
    let rest = &line[at_raw + pat.len()..];
    let mut depth = 0i32;
    let mut end = rest.len();
    for (k, ch) in rest.char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                if depth == 0 {
                    end = k;
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let inner = &rest[..end];
    let parts = split_top(inner);
    parts
        .get(pos)
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// 최상위 쉼표로 자른다(괄호·따옴표 안의 쉼표는 안 센다).
fn split_top(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth, mut instr) = (Vec::new(), String::new(), 0i32, false);
    let mut prev = '\0';
    for ch in s.chars() {
        if instr {
            cur.push(ch);
            if ch == '"' && prev != '\\' {
                instr = false;
            }
            prev = ch;
            continue;
        }
        match ch {
            '"' => {
                instr = true;
                cur.push(ch);
            }
            '(' | '[' | '{' => {
                depth += 1;
                cur.push(ch);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(ch),
        }
        prev = ch;
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify_src(src: &str) -> FileClass {
        let code_src = mask_non_code(src);
        let comment_src = mask_literals(src);
        let code: Vec<&str> = code_src.lines().collect();
        let comments: Vec<&str> = comment_src.lines().collect();
        let raw: Vec<&str> = src.lines().collect();
        classify(&code, &comments, &raw)
    }

    #[test]
    fn a_time_only_nonce_is_uniquified_but_graded_weak() {
        let fc = classify_src(
            "fn f() {\n    let n = nanos();\n    let p = std::env::temp_dir().join(format!(\"x-{n}\"));\n}",
        );
        assert_eq!(fc.sites.len(), 1);
        assert_eq!(fc.uniquified.len(), 1, "약한 성분도 유니크화이긴 하다");
        assert_eq!(fc.silent.len(), 0, "위반(고정 이름)이 아니다");
        assert_eq!(fc.weak_only.len(), 1, "등급이 판정에 안 들어갔다");
    }

    /// pid를 함께 써도 같은 프로세스의 시계 충돌 위험은 남는다.
    #[test]
    fn the_macos_incident_shape_is_weak_even_though_a_pid_is_present() {
        let fc = classify_src(
            "fn f() {\n    let u = format!(\"{}-{}\", std::process::id(), nanos());\n    let p = std::env::temp_dir().join(format!(\"x-{u}\"));\n}",
        );
        assert_eq!(fc.uniquified.len(), 1, "유니크화이긴 하다");
        assert_eq!(
            fc.weak_only.len(),
            1,
            "pid만으로 같은 프로세스 안의 시계 값 충돌을 막을 수 없다."
        );
    }

    #[test]
    fn path_for_does_not_cover_the_within_process_axis() {
        let fc = classify_src(
            "fn f() {\n    let p = std::env::temp_dir().join(path_for(\"tasty\", nanos()));\n}",
        );
        assert_eq!(fc.uniquified.len(), 1);
        assert_eq!(
            fc.weak_only.len(),
            1,
            "path_for 는 prefix+pid+surface_id 라 같은 프로세스·같은 id 면 같은 경로다"
        );
    }

    /// 같은 성분의 나열 순서를 바꿔도 분류 결과는 같아야 한다.
    #[test]
    fn the_grade_does_not_depend_on_the_order_inside_the_binding() {
        let pid_first = classify_src(
            "fn f() {\n    let unique = format!(\n        \"{}-{}\",\n        \
             std::process::id(),\n        nanos(),\n    );\n    \
             let p = std::env::temp_dir().join(format!(\"x-{unique}\"));\n}",
        );
        let clock_first = classify_src(
            "fn f() {\n    let unique = format!(\n        \"{}-{}\",\n        \
             nanos(),\n        std::process::id(),\n    );\n    \
             let p = std::env::temp_dir().join(format!(\"x-{unique}\"));\n}",
        );
        assert_eq!(
            pid_first.weak_only, clock_first.weak_only,
            "같은 성분인데 `format!` 안 순서만으로 등급이 갈렸다 — 줄 순서는 안전성이 아니다"
        );
        assert!(
            !pid_first.weak_only.is_empty(),
            "pid와 시계만 쓰는 이름은 프로세스 내부에서 시간 해상도에 의존한다."
        );
    }

    #[test]
    fn a_monotonic_counter_clears_the_grade_where_a_pid_does_not() {
        let fc = classify_src(
            "fn f() {\n    let p = std::env::temp_dir().join(format!(\"x-{}-{}\", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));\n}",
        );
        assert_eq!(fc.uniquified.len(), 1);
        assert!(
            fc.weak_only.is_empty(),
            "카운터는 같은 프로세스의 호출을 구분한다"
        );
    }

    #[test]
    fn the_union_is_actually_the_union_of_the_four_axes() {
        let mut parts: Vec<&str> = Vec::new();
        parts.extend_from_slice(PER_CALL_TOKENS);
        parts.extend_from_slice(PROCESS_TOKENS);
        parts.extend_from_slice(COUNTER_TOKENS);
        parts.extend_from_slice(CLOCK_TOKENS);
        let mut union: Vec<&str> = UNIQ_TOKENS.to_vec();
        parts.sort_unstable();
        union.sort_unstable();
        assert_eq!(
            parts, union,
            "UNIQ_TOKENS 가 네 축의 합집합이 아니다 — 어느 쪽에 더했는지 확인해라"
        );
    }

    #[test]
    fn the_thread_axis_never_counts_as_uniquification() {
        for t in THREAD_TOKENS {
            assert!(
                !UNIQ_TOKENS.contains(t),
                "{t} 가 유니크화 성분으로 새어 들어갔다 — 스레드 id 는 재호출을 못 가른다"
            );
        }
    }

    /// pid와 스레드 ID를 함께 써도 같은 스레드의 재호출을 구분하지 못한다.
    #[test]
    fn a_thread_id_splits_tests_but_not_recalls() {
        let fc = classify_src(
            "fn f() {\n    let root = std::env::temp_dir().join(format!(\n        \
             \"tasty-cited-anchors-fixture-{}-{:?}\",\n        std::process::id(),\n        \
             std::thread::current().id()\n    ));\n}",
        );
        assert_eq!(fc.uniquified.len(), 1, "pid 가 있으니 유니크화이긴 하다");
        assert_eq!(
            fc.recall_blind.len(),
            1,
            "스레드 ID는 같은 스레드의 재호출을 구분하지 못한다."
        );
        assert_eq!(
            fc.recall_blind_with_thread.len(),
            1,
            "pid와 스레드 ID를 함께 쓰는 항목을 별도로 보고해야 한다."
        );
    }

    #[test]
    fn a_thread_id_alone_is_not_uniquification() {
        let fc = classify_src(
            "fn f() {\n    let root = std::env::temp_dir().join(format!(\"x-{:?}\", std::thread::current().id()));\n}",
        );
        assert_eq!(fc.sites.len(), 1);
        assert!(
            fc.uniquified.is_empty(),
            "스레드 id 는 프로세스 간에 그대로 겹친다"
        );
        assert_eq!(fc.silent.len(), 1, "사유도 없으니 위반으로 나가야 한다");
    }

    #[test]
    fn a_reason_at_the_site_clears_the_weak_grade() {
        let fc = classify_src(
            "fn f() {\n    // 이유: 같은 프로세스가 회차마다 다른 경로를 원한다.\n    let p = std::env::temp_dir().join(format!(\"x-{}\", nanos()));\n}",
        );
        assert_eq!(fc.uniquified.len(), 1);
        assert!(fc.weak_only.is_empty(), "그 자리 사유를 안 읽었다");
    }

    #[test]
    fn a_fixed_name_under_temp_dir_is_caught() {
        let fc = classify_src(
            "fn f() {\n    let p = std::env::temp_dir().join(\"tasty-thing.toml\");\n}",
        );
        assert_eq!(fc.sites.len(), 1);
        assert_eq!(
            fc.silent.len(),
            1,
            "유니크화·사유 없는 고정 이름을 잡아야 한다"
        );
    }

    /// 성분별로 독립된 입력을 둔다. 목록에서 픽스처를 자동 생성하면 같은 오타를 공유할 수 있다.
    #[test]
    fn every_recognized_uniquifier_is_actually_recognized() {
        let cases: [(&str, &str); 11] = [
            (
                "TempDir",
                "fn f() {\n    let base = std::env::temp_dir().join(\"tasty\");\n    let d = TempDir::new_in(&base).unwrap();\n}",
            ),
            (
                "tempfile",
                "fn f() {\n    let base = std::env::temp_dir().join(\"tasty\");\n    let b = tempfile::Builder::new().tempdir_in(&base).unwrap();\n}",
            ),
            (
                "tempdir(",
                "fn f() {\n    let base = std::env::temp_dir().join(\"tasty\");\n    let d = tempdir().unwrap();\n}",
            ),
            (
                "NamedTempFile",
                "fn f() {\n    let base = std::env::temp_dir().join(\"tasty\");\n    let h = NamedTempFile::new_in(&base).unwrap();\n}",
            ),
            (
                "process::id",
                "fn f() {\n    let p = std::env::temp_dir().join(format!(\"tasty-{}\", std::process::id()));\n}",
            ),
            (
                "fetch_add",
                "fn f() {\n    let p = std::env::temp_dir().join(format!(\"tasty-{}\", N.fetch_add(1, Ordering::Relaxed)));\n}",
            ),
            (
                "AtomicUsize",
                "fn f() {\n    let p = std::env::temp_dir().join(format!(\"tasty-{}\", AtomicUsize::new(0).load(Ordering::Relaxed)));\n}",
            ),
            (
                "pid",
                "fn f() {\n    let p = std::env::temp_dir().join(format!(\"tasty-{}\", pid));\n}",
            ),
            (
                "path_for",
                "fn f() {\n    let p = std::env::temp_dir().join(path_for(\"tasty\"));\n}",
            ),
            (
                "nanos",
                "fn f() {\n    let p = std::env::temp_dir().join(format!(\"tasty-{}\", nanos()));\n}",
            ),
            (
                "SystemTime",
                "fn f() {\n    let base = std::env::temp_dir().join(\"tasty\");\n    let t = SystemTime::now();\n}",
            ),
        ];
        assert_eq!(
            cases.len(),
            UNIQ_TOKENS.len(),
            "검증 입력 수와 성분 수가 다르다. 새 성분에도 독립된 검증 입력을 추가한다."
        );
        for (name, src) in cases {
            let fc = classify_src(src);
            assert_eq!(
                fc.sites.len(),
                1,
                "{name}: 픽스처에서 경로 생성 지점이 하나 나와야 한다."
            );
            assert!(
                fc.silent.is_empty(),
                "{name}을 유일성 성분으로 인식하지 못했다. 성분 목록과 판독기를 확인한다."
            );
            assert_eq!(fc.uniquified.len(), 1, "{name}: 유니크화 한 곳이어야 한다");
        }
    }

    #[test]
    fn a_pid_keyed_name_passes() {
        let fc = classify_src(
            "fn f() {\n    let p = std::env::temp_dir()\n        .join(format!(\"x-{}.txt\", std::process::id()));\n}",
        );
        assert!(fc.silent.is_empty());
        assert_eq!(fc.uniquified.len(), 1);
    }

    #[test]
    fn a_uniquifier_further_down_the_build_is_reached() {
        let fc = classify_src(
            "fn f() {\n    let dir = std::env::temp_dir().join(\"tasty-scrollback\");\n    let p = dir.join(format!(\n        \"surface-{}-{}\",\n        std::process::id(),\n        id\n    ));\n}",
        );
        assert!(fc.silent.is_empty(), "체인 아래 pid 를 창이 봐야 한다");
        assert_eq!(fc.uniquified.len(), 1);
    }

    #[test]
    fn an_inline_unique_var_bound_to_a_uniquifier_passes() {
        let fc = classify_src(
            "fn f() {\n    let unique = format!(\"{}-{}\", std::process::id(), nanos());\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let marker = std::env::temp_dir().join(format!(\"tasty-mark-{unique}.txt\"));\n}",
        );
        assert!(
            fc.silent.is_empty(),
            "창 밖 위에서 uniquifier 로 바인딩된 변수를 인라인 참조하면 통과해야 한다"
        );
        assert_eq!(fc.uniquified.len(), 1);
    }

    #[test]
    fn a_positional_unique_var_passes() {
        let fc = classify_src(
            "fn f() {\n    let unique = format!(\"{}\", std::process::id());\n    let p = std::env::temp_dir().join(format!(\"tasty-test-{}.port\", unique));\n}",
        );
        assert!(fc.silent.is_empty());
        assert_eq!(fc.uniquified.len(), 1);
    }

    #[test]
    fn a_var_not_bound_to_a_uniquifier_does_not_pass() {
        let fc = classify_src(
            "fn f() {\n    let scenario = read_name();\n    let p = std::env::temp_dir().join(format!(\"tasty-{scenario}\"));\n}",
        );
        assert_eq!(
            fc.silent.len(),
            1,
            "uniquifier 로 바인딩 안 된 변수는 유니크화가 아니다"
        );
    }

    #[test]
    fn a_reasoned_shared_path_passes() {
        let fc = classify_src(
            "fn f() {\n    // 이유: 사용자 config 라 의도된 공유다.\n    let p = std::env::temp_dir().join(\"tasty-config.toml\");\n}",
        );
        assert!(fc.silent.is_empty(), "사유가 붙으면 통과");
        assert_eq!(fc.reasoned.len(), 1);
    }

    #[test]
    fn an_english_reason_marker_also_passes() {
        let fc = classify_src(
            "fn f() {\n    // reason: shared on purpose.\n    let p = std::env::temp_dir().join(\"shared\");\n}",
        );
        assert!(fc.silent.is_empty());
    }

    #[test]
    fn a_korean_sayu_marker_also_passes() {
        let fc = classify_src(
            "fn f() {\n    // 사유: 프로필 사이에 일부러 공유한다.\n    let p = std::env::temp_dir().join(\"tasty-shared\");\n}",
        );
        assert!(
            fc.silent.is_empty(),
            "`사유:` 를 사유 마커로 인정하지 않는다 — 목록에서 뺐으면 모듈 문서의 \
             마커 목록도 함께 고쳐라"
        );
        assert_eq!(fc.reasoned.len(), 1);
    }

    #[test]
    fn a_bare_temp_dir_without_join_is_ignored() {
        let fc =
            classify_src("fn f() {\n    let dir = std::env::temp_dir();\n    read_only(dir);\n}");
        assert!(fc.sites.is_empty(), "join 이 없으면 자리로 세지 않는다");
    }

    #[test]
    fn a_join_on_the_bound_receiver_counts_however_far_it_sits() {
        let fc = classify_src(
            "fn f() {\n    let dir = std::env::temp_dir();\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let e = 5;\n    let g = 6;\n    let h = 7;\n    let p = dir.join(\"fixed-a\");\n}",
        );
        assert_eq!(fc.sites.len(), 1, "여덟 줄 아래의 수신자도 이 자리 것이다");
        assert_eq!(fc.silent.len(), 1, "고정 이름이라 위반으로 나와야 한다");
    }

    #[test]
    fn a_join_on_another_value_does_not_make_this_a_site() {
        let fc = classify_src(
            "fn f() {\n    let dir = std::env::temp_dir();\n    let valid = names();\n    let msg = valid.join(\", \");\n    read_only(dir, msg);\n}",
        );
        assert!(
            fc.sites.is_empty(),
            "문자열 join 은 경로를 짓는 것이 아니다"
        );
        assert_eq!(fc.unpaired.len(), 1);
    }

    #[test]
    fn a_same_named_binding_in_another_fn_is_not_this_receiver() {
        let fc = classify_src(
            "fn f() {\n    let dir = std::env::temp_dir();\n    read_only(dir);\n}\nfn g() {\n    let dir = other();\n    let p = dir.join(\"fixed-a\");\n}",
        );
        assert!(fc.sites.is_empty(), "수신자는 감싸는 함수 안에서만 찾는다");
    }

    #[test]
    fn a_reason_at_the_top_of_the_attached_block_counts() {
        let fc = classify_src(
            "fn f() {\n    // 이유: 공유가 의도다.\n    // 둘\n    // 셋\n    // 넷\n    // 다섯\n    // 여섯\n    // 일곱\n    let p = std::env::temp_dir().join(\"fixed-a\");\n}",
        );
        assert!(fc.silent.is_empty(), "붙은 블록의 첫 줄에 있는 사유");
    }

    #[test]
    fn a_reason_outside_the_attached_block_does_not_count() {
        let fc = classify_src(
            "fn f() {\n    // 이유: 공유가 의도다.\n\n    let q = 1;\n    let p = std::env::temp_dir().join(\"fixed-b\");\n}",
        );
        assert_eq!(fc.silent.len(), 1, "블록 밖의 사유는 안 센다");
    }

    #[test]
    fn a_reason_inside_a_string_does_not_count() {
        let fc = classify_src(
            "fn f() {\n    let msg = \"이유: not a real marker\";\n    let p = std::env::temp_dir().join(\"fixed\");\n}",
        );
        assert_eq!(fc.silent.len(), 1, "문자열 속 이유: 는 사유가 아니다");
    }
}
