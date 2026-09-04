//! DPI 변환이 `PhysicalPx`/`LogicalPx` 의 변환 API 밖에서 일어나는 것을 막는 드리프트 가드.
//!
//! `docs/concepts/typed-length.md` 는 두 좌표계를 섞는 것을 타입이 막는다고 규정한다.
//! 타입이 실제로 막는 것은 **혼합**(`PhysicalPx + LogicalPx`)이고, **변환 누락**은 막지
//! 못한다 — `PhysicalPx(x * ppp)` 에서 `* ppp` 를 빠뜨려도 그대로 컴파일된다. 그래서
//! 변환은 `to_physical(sf)` / `to_logical(sf)` 로만 하고, 그 밖의 수동 산술을 이 가드가
//! 잡는다. 즉 정책의 절반은 컴파일러가, 나머지 절반은 이 스캔이 강제한다.
//!
//! # 왜 `tests/` 가 아니라 여기인가 — 되돌리지 마라
//!
//! 레포의 소스 스캔 가드는 관례상 `tests/*.rs` 에 모여 있다. 이 가드만 크레이트 안의
//! 유닛 테스트인 것은 관례를 몰라서가 아니라 **실행 채널이 다르기 때문**이다.
//!
//! 통합 테스트(`tests/*.rs`)와 크레이트 안 유닛 테스트는 **컴파일 축과 실행 축이 다르게**
//! 자동화돼 있다. 어느 축이 어디서 도는지는 [`docs/dev-guide/ci-gates.md`] 가 정본이다 —
//! 여기에 그 표를 복제하지 않는다(복제하면 정본이 바뀔 때 이 주석만 낡는다).
//!
//! 이 가드에 걸리는 것은 **두 축의 차이**다. 통합 테스트는 컴파일 축이 자동이지만 실행 축은
//! 그렇지 않고, 크레이트 안 유닛 테스트는 실행 축까지 자동이다. 일반 테스트에게 "컴파일은
//! 자동으로 본다" 는 부분적 안전망이 되지만, **소스를 런타임에 읽어 스캔하는 가드에게는
//! 0** 이다 — 스캔 로직은 컴파일돼도 실행되지 않으면 아무것도 안 본다. 드리프트 가드의
//! 값어치는 "지금 위반 0건" 이 아니라 "나중에 다시 새면 잡힌다" 이므로, 사람이 기억해야
//! 도는 채널에 두면 규칙이 문서에 관례로만 적혀 있는 상태와 실효가 같다.
//!
//! 그래서 여기에 있다. `tests/` 로 옮기면 **조용히 실행 축을 잃는다** — 옮긴 쪽은 초록으로
//! 보이고, 아무도 그것이 실행되지 않는다는 사실을 모른다.
//!
//! 소재를 루트 패키지로 잡은 이유는 둘이다. ① 스캔 범위가 워크스페이스 전역이라, 리프
//! 타입 크레이트가 자기 소비자들을 검사하면 의존 방향이 뒤집힌다. ② 루트 패키지의
//! `CARGO_MANIFEST_DIR` 이 곧 레포 루트라 상향 경로(`../..`)가 필요 없다 — 상향 경로가
//! 틀리면 스캔 대상이 0개가 되고 가드가 조용히 초록이 되는데, 그 위험 자체를 없앤다.
//!
//! # Windows 에서도 돈다
//!
//! 유닛 테스트를 도는 잡 중 하나는 Windows 러너다. 경로는 `Path` API 로 다루고 구분자를
//! `/` 로 정규화한다. 줄 끝의 `\r`(CRLF)은 판정 전에 떼어낸다. 이걸 놓치면 Windows
//! 잡만 빨개지고, 그 잡은 아무도 자기 변경 것으로 보지 않는다.
//!
//! [`docs/dev-guide/ci-gates.md`]: ../docs/dev-guide/ci-gates.md

use std::path::{Path, PathBuf};

/// 스캔이 최소한 이만큼은 파일을 봐야 한다. 경로가 틀어져 대상이 줄면 위반이 0건이 되어
/// 가드가 **조용히 초록**이 된다 — 그 실패를 잡는 유일한 장치다. 현재 실측 1109개이고,
/// `src`(550) 또는 `crates`(559) 한쪽만 훑는 사고도 이 하한에 걸린다.
const MIN_SCANNED_FILES: usize = 800;

/// 수동 DPI 산술이 남아도 되는 자리 — `(경로, 건수, 사유)`.
///
/// 건수까지 고정하는 이유: 파일 단위로만 면제하면 그 파일이 새 위반을 들여도 안 잡힌다.
/// 실제 건수와 다르면 실패하므로, 이식으로 줄었을 때도 이 표를 갱신하게 된다.
const ALLOWED: &[(&str, usize, &str)] = &[
    (
        "crates/tasty-type-geometry/src/length.rs",
        2,
        "변환 API 본체 — `to_logical`/`to_physical` 이 실제로 나누고 곱하는 자리",
    ),
    (
        "src/host_api/webview.rs",
        8,
        "`WebViewBounds` 짝 타입의 변환 본체. 세 파일에 흩어져 있던 산술을 여기로 모은 \
         초크포인트라, 여기 산술이 있는 것이 정상이다",
    ),
    (
        "crates/tasty-settings/src/appearance.rs",
        1,
        "폰트 크기 스케일 — 길이가 아니라 글자 크기라 두 좌표계 어느 쪽도 아니다",
    ),
    (
        "crates/tasty-plugin-sdk/src/egui_surface.rs",
        2,
        "plugin SDK 는 `tasty-type-geometry` 에 의존하지 않는다(별도 프로세스의 공개 \
         API 표면). 타입 API 를 쓸 수 없어 산술이 유일한 수단이다",
    ),
];

/// 가드 자신 — 스캔 대상에서 뺀다. 이 파일은 금지 형태를 **데이터로** 담는 것이 본질이라
/// (탐지기 단위 테스트의 예제 문자열) 자기 자신을 세면 영원히 빨갛다. 여기엔 런타임 DPI
/// 코드가 없으므로 빼도 잃는 것이 없다. `no_todo_file_citation` 의 ALLOWLIST 가 금지
/// 형태를 담는 파일을 면제하는 것과 같은 처리다.
const SELF_PATH: &str = "src/dpi_conversion_guard.rs";

/// 아직 타입 API 로 옮기지 못한 자리 — `(경로, 건수)`. **지금은 비어 있다.**
///
/// 비었다는 것이 이 목록의 정상 상태다. 여기에 항목이 생겼다면 새 변환 경로가 타입 API
/// 밖에 만들어졌다는 뜻이므로, 등재하기 전에 먼저 옮길 수 있는지 본다.
///
/// [`ALLOWED`] 와 분리해 둔 이유는 성격이 반대이기 때문이다. `ALLOWED` 는 "여기 산술이
/// 있는 것이 정상" 이라 영구적이고, 이쪽은 "아직 못 옮겼다" 라 없어져야 한다. 한 표에
/// 섞으면 다음 사람이 둘을 구분하지 못해 미이식분이 영구 면제로 굳는다.
const PENDING_PORT: &[(&str, usize)] = &[];

/// 수동 변환으로 보는 식별자. `pixels_per_point` 는 메서드 호출 형태(`ppp()` )로도 쓰인다.
const CONVERSION_IDENTS: &[&str] = &["ppp", "scale_factor", "sf", "pixels_per_point"];

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `line` 안에서 `ident` 가 **낱말로** 나타나고 그 바로 앞이나 뒤에 `*` 또는 `/` 가 붙은
/// 횟수. `to_logical(scale_factor)` 처럼 인자로 넘기는 형태는 세지 않는다.
fn conversion_hits(line: &str, ident: &str) -> usize {
    let mut hits = 0;
    let mut from = 0;
    while let Some(rel) = line[from..].find(ident) {
        let start = from + rel;
        let end = start + ident.len();
        from = end;

        let before_is_ident = line[..start].chars().next_back().is_some_and(is_ident_char);
        let after_is_ident = line[end..].chars().next().is_some_and(is_ident_char);
        if before_is_ident || after_is_ident {
            continue; // `appp` / `scale_factor_x` 같은 다른 식별자의 일부
        }

        // 앞쪽: 공백을 건너뛴 첫 글자가 연산자인가. `x / ctx.pixels_per_point()` 처럼
        // 메서드 호출이면 수신자 경로(`ctx.`)를 먼저 건너뛰어야 연산자에 닿는다.
        let mut before = &line[..start];
        if before.ends_with('.') {
            before = before.trim_end_matches('.');
            before = before.trim_end_matches(|c| is_ident_char(c) || c == '.');
        }
        let prev_op = before
            .chars()
            .rev()
            .find(|c| !c.is_whitespace())
            .is_some_and(|c| c == '*' || c == '/');
        // 뒤쪽: 메서드 호출이면 `()` 를 건너뛴 뒤의 첫 글자를 본다.
        let mut rest = &line[end..];
        if rest.starts_with("()") {
            rest = &rest[2..];
        }
        let next_op = rest
            .chars()
            .find(|c| !c.is_whitespace())
            .is_some_and(|c| c == '*' || c == '/');

        if prev_op || next_op {
            hits += 1;
        }
    }
    hits
}

/// 한 줄이 주석이면 세지 않는다 — 정책을 설명하는 문장이 위반으로 잡히면 안 된다.
fn is_comment(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with("/*") || t.starts_with('*')
}

fn count_in_source(source: &str) -> usize {
    source
        .lines()
        // CRLF 체크아웃(Windows 러너)에서 줄 끝 `\r` 이 판정에 섞이지 않게 떼어낸다.
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !is_comment(l))
        .map(|l| {
            CONVERSION_IDENTS
                .iter()
                .map(|ident| conversion_hits(l, ident))
                .sum::<usize>()
        })
        .sum()
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// 레포 루트 기준 상대경로를 `/` 구분자로 정규화한다 — Windows 러너에서 `\` 로 나오는
/// 것을 그대로 비교하면 allowlist 가 전부 빗나간다.
fn relative_slash(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn manual_arithmetic_is_counted_but_passing_the_factor_is_not() {
        // 수동 변환 — 잡아야 한다.
        assert_eq!(count_in_source("x: PhysicalPx(rect.min.x * ppp),"), 1);
        assert_eq!(count_in_source("let a = r.x.value() / scale_factor;"), 1);
        assert_eq!(count_in_source("(p.x / ppp, p.y / ppp)"), 2);
        assert_eq!(
            count_in_source("let w = px as f32 / ctx.pixels_per_point();"),
            1
        );

        // scale factor 를 **인자로 넘기는** 것은 변환 API 를 쓰는 정상 형태다.
        assert_eq!(
            count_in_source("let l = physical.to_logical(scale_factor);"),
            0
        );
        assert_eq!(count_in_source("PhysicalPx(x).to_logical(sf).value()"), 0);
        assert_eq!(
            count_in_source("RawInput { pixels_per_point: ppp, ..d }"),
            0
        );

        // 다른 식별자의 일부를 낱말로 오인하지 않는다.
        assert_eq!(count_in_source("let v = happ * 2.0;"), 0);
        assert_eq!(count_in_source("let v = scale_factor_hint * 2.0;"), 0);
    }

    #[test]
    fn comments_and_crlf_do_not_change_the_verdict() {
        // 정책을 설명하는 주석이 위반으로 잡히면 문서를 못 쓴다.
        assert_eq!(count_in_source("// 여기서 x * ppp 를 하면 안 된다"), 0);
        assert_eq!(count_in_source("/// `PhysicalPx(x * ppp)` 는 금지"), 0);

        // Windows 러너의 CRLF 체크아웃에서 판정이 흔들리면 그 잡만 빨개진다.
        let lf = "let a = x * ppp;\nlet b = y / scale_factor;\n";
        let crlf = lf.replace('\n', "\r\n");
        assert_eq!(count_in_source(lf), 2);
        assert_eq!(count_in_source(&crlf), count_in_source(lf));

        // 주석 판정도 CRLF 에서 같아야 한다.
        assert_eq!(count_in_source("// x * ppp\r\n"), 0);
    }

    #[test]
    fn dpi_conversion_goes_through_the_typed_api() {
        let root = repo_root();
        let mut files = Vec::new();
        collect_rs(&root.join("src"), &mut files);
        collect_rs(&root.join("crates"), &mut files);

        assert!(
            files.len() >= MIN_SCANNED_FILES,
            "스캔한 .rs 파일이 {}개뿐이다(하한 {MIN_SCANNED_FILES}). 스캔 루트가 \
             어긋났을 때 위반 0건으로 조용히 통과하는 것을 막는 하한이다 — 위반이 \
             정말 없는 것이 아니라 아무것도 안 본 것이다.",
            files.len(),
        );

        let mut offenders: Vec<(String, usize)> = Vec::new();
        let mut seen: Vec<(String, usize)> = Vec::new();

        for path in &files {
            let rel = relative_slash(&root, path);
            if rel == SELF_PATH {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(path) else {
                continue;
            };
            let count = count_in_source(&source);
            let listed = ALLOWED.iter().any(|(p, _, _)| *p == rel)
                || PENDING_PORT.iter().any(|(p, _)| *p == rel);
            if listed {
                seen.push((rel, count));
            } else if count > 0 {
                offenders.push((rel, count));
            }
        }

        offenders.sort();
        assert!(
            offenders.is_empty(),
            "DPI 변환을 수동 산술로 하는 자리가 있다. `LogicalPx::to_physical(sf)` / \
             `PhysicalPx::to_logical(sf)` 를 거쳐라 — 산술이 정당한 자리면 사유와 함께 \
             이 파일의 ALLOWED 에 등재한다(docs/concepts/typed-length.md).\n위반: {offenders:#?}",
        );

        let actual_of = |path: &str| seen.iter().find(|(p, _)| p == path).map(|(_, c)| *c);

        // 역방향 ① — 영구 면제. 건수가 다르면(줄었든 늘었든) 표가 낡은 것이다.
        for (path, expected, reason) in ALLOWED {
            match actual_of(path) {
                None => panic!(
                    "ALLOWED 에 `{path}` 가 있는데 스캔 대상에 없다 — 파일이 옮겨졌거나 \
                     지워졌다. 등재를 지워라. (사유: {reason})"
                ),
                Some(actual) if actual != *expected => panic!(
                    "`{path}` 의 수동 산술이 {actual}건인데 ALLOWED 는 {expected}건으로 \
                     적혀 있다. 늘었으면 그 증가가 정말 사유({reason})에 해당하는지 \
                     확인하고, 줄었으면 숫자를 낮춰라."
                ),
                Some(_) => {}
            }
        }

        // 역방향 ② — 미이식 목록. 0 이 되면 행을 지워야 목록이 실제로 수렴한다.
        for (path, expected) in PENDING_PORT {
            match actual_of(path) {
                None => panic!(
                    "PENDING_PORT 에 `{path}` 가 있는데 스캔 대상에 없다 — 파일이 \
                     옮겨졌거나 지워졌다. 등재를 지워라."
                ),
                Some(0) => panic!(
                    "`{path}` 의 수동 산술이 0건이다 — 이식이 끝났으니 PENDING_PORT \
                     에서 그 행을 지워라. 남겨두면 이 파일이 나중에 다시 새도 안 잡힌다."
                ),
                Some(actual) if actual != *expected => panic!(
                    "`{path}` 의 수동 산술이 {actual}건인데 PENDING_PORT 는 {expected}건 \
                     으로 적혀 있다. 이식으로 줄었으면 숫자를 낮춰라."
                ),
                Some(_) => {}
            }
        }
    }
}
