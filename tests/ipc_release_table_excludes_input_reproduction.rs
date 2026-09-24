//! 사용자 입력 재현을 debug 빌드에만 제공한다는 규칙을 CLI 이름·메서드 표·라우터 소스로 확인한다.
//! 이름이나 debug CLI에 단서가 없는 새 메서드의 의미는 사람이 검토해야 한다.
//! 로컬 호출자는 권한 표 검사에서 허용되므로 표에서 이름을 빼는 것만으로 라우터 접근이 차단되지는 않는다.
//! 등록된 함수 본문만 읽는다. 핸들러를 다른 함수로 옮기면 RELEASE_ROUTERS 목록도 확인한다.
//!
//! 이 검사는 DEBUG_METHODS와 비교하므로 debug 빌드에서만 실행한다.
//! 자동 실행은 헤드리스 조합에서만 일어난다(check-headless). 기본 조합의 --lib --bins 호출에는 포함되지 않는다.
//! Windows와 헤드리스의 clippy --all-targets는 컴파일 검사이며 실행과 구별한다.
#![cfg(debug_assertions)]

use tasty_doc_guards::cfg_predicate as cfg_span;
use tasty_doc_guards::match_arms::Source;

use std::path::Path;

use tasty_ipc::method_meta::METHOD_TABLE;

/// debug CLI에서 사용하지만 release IPC에도 필요한 메서드와 근거다.
const CLI_DEBUG_RELEASE_METHODS: &[(&str, &str)] = &[
    // 로컬 loopback attach 도구는 debug 전용이지만 강제 분리 메서드는 일반 원격 attach도 사용한다.
    (
        "attach.force_detach",
        "원격 attach 와 공용 — release 정식 표면",
    ),
    (
        "attach.force_detach_workspace",
        "원격 attach 와 공용 — release 정식 표면",
    ),
];

/// 입력 재현을 나타내는 이름 패턴이다. 대상 PTY에 바이트를 쓰는 surface.send_key와 구별한다.
const INPUT_REPRODUCTION_PATTERNS: &[(&str, &str)] = &[
    ("inject", "키·마우스 이벤트 주입"),
    ("raw_key", "OS 이벤트 스트림 키 주입"),
    ("switch_input_source", "OS 전역 입력 소스 전환"),
    ("ime_", "입력기 조합 상태 강제 세팅"),
    ("simulate", "입력 시뮬레이션"),
];

const FORBIDDEN_LAST_SEGMENTS: &[&str] = &["focus"];

fn manifest_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn debug_cli_methods_are_not_in_the_release_table() {
    let path = manifest_root().join("crates/tasty-cli/src/request/debug.rs");
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "debug CLI 요청 매핑을 읽을 수 없다: {}: {e}",
            path.display()
        )
    });

    let mut offenders: Vec<String> = Vec::new();
    let mut seen = 0usize;
    for raw in src.split('"').skip(1).step_by(2) {
        if !raw.contains('.')
            || !raw
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '_' || c == '.')
        {
            continue;
        }
        seen += 1;
        if CLI_DEBUG_RELEASE_METHODS.iter().any(|(m, _)| *m == raw) {
            continue;
        }
        if METHOD_TABLE.iter().any(|(m, _)| *m == raw) {
            offenders.push(raw.to_string());
        }
    }

    assert!(
        seen > 30,
        "debug CLI 메서드 리터럴을 {seen} 개밖에 못 찾았다 — 스캔 패턴이 깨졌을 가능성이 크다"
    );
    offenders.sort();
    offenders.dedup();
    assert!(
        offenders.is_empty(),
        "debug 전용 CLI(`DebugCommands`)가 부르는데 release 표(METHOD_TABLE)에 \
         등재된 메서드가 있다. 같은 기능이 CLI 에서는 debug, IPC 에서는 release 로 \
         갈라진 상태라 IPC 표면만 열려 있다. DEBUG_METHODS 로 옮기거나, release 가 \
         맞다면 CLI_DEBUG_RELEASE_METHODS 에 근거와 함께 등재하라:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn release_table_has_no_input_reproduction_method_names() {
    let mut offenders: Vec<String> = Vec::new();
    for (method, _) in METHOD_TABLE {
        for (pat, why) in INPUT_REPRODUCTION_PATTERNS {
            if method.contains(pat) {
                offenders.push(format!("{method} — '{pat}' ({why})"));
            }
        }
        let last = method.rsplit('.').next().unwrap_or(method);
        if FORBIDDEN_LAST_SEGMENTS.contains(&last) {
            offenders.push(format!("{method} — 마지막 세그먼트 '{last}' (포커스 전환)"));
        }
    }

    offenders.sort();
    offenders.dedup();
    assert!(
        offenders.is_empty(),
        "release 표(METHOD_TABLE)에 사용자 입력 재현 계열 메서드가 있다 \
         (docs/identity.md 원칙 1 ② — 입력 재현은 release 에 없다). \
         DEBUG_METHODS 로 옮겨라. 이름이 우연히 겹친 정상 에이전트 메서드라면 \
         이 파일의 패턴을 좁히고 그 근거를 남겨라:\n  {}",
        offenders.join("\n  ")
    );
}

/// prefix 규칙은 개별 METHOD_TABLE 항목이 아니므로 별도로 cfg를 확인한다.
#[test]
fn ime_prefix_rule_stays_debug_only() {
    let path = manifest_root().join("crates/tasty-ipc/src/method_meta.rs");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("메서드 표 소스를 읽을 수 없다: {}: {e}", path.display()));
    let lines: Vec<&str> = src.lines().collect();

    let idx = lines
        .iter()
        .position(|l| l.contains("pub const PREFIX_RULES") && l.contains("surface.ime_"))
        .expect(
            "`surface.ime_` 를 담은 PREFIX_RULES 정의를 찾지 못했다 — 정의가 \
             여러 줄로 쪼개졌거나 규칙이 사라졌다. 사라졌다면 이 테스트도 함께 \
             정리하라(사각지대가 없어진 것이므로).",
        );

    assert!(
        idx > 0 && lines[idx - 1].trim() == "#[cfg(debug_assertions)]",
        "`surface.ime_` prefix 규칙이 `#[cfg(debug_assertions)]` 없이 정의돼 있다. \
         IME 조합 상태 강제 세팅은 사용자 입력 재현이라 release 표면에 두지 않는다 \
         (docs/identity.md 원칙 1 ②). 바로 앞 줄: {:?}",
        lines.get(idx.wrapping_sub(1)).copied().unwrap_or("<없음>")
    );
    assert!(
        src.contains("#[cfg(not(debug_assertions))]\npub const PREFIX_RULES"),
        "release 쪽 `PREFIX_RULES` (빈 슬라이스) 정의가 없다 — debug 쪽에만 cfg 를 \
         걸면 release 빌드가 컴파일되지 않는다."
    );
}

/// release에서 남는 라우터의 파일·함수 시그니처·메서드 후보 수 하한이다.
/// 함수나 모듈 전체가 debug 전용인 라우터는 제외한다.
const RELEASE_ROUTERS: &[(&str, &str, usize)] = &[
    (
        "src/adapters/ipc/handler.rs",
        "fn route_engine_handler(",
        200,
    ),
    ("src/adapters/ipc/handler.rs", "fn route_window_handler(", 1),
    (
        "src/app/ipc/app_methods.rs",
        "pub(crate) fn ipc_step_app_methods(",
        12,
    ),
    (
        "src/app/dispatch/list_global.rs",
        "pub(crate) fn dispatch_list_global(",
        3,
    ),
    // pump_ipc에도 메서드 분기가 다시 생길 수 있어 남긴다. 현재 하한 0으로는 그 함수의 부분 누락을 찾지 못한다.
    (
        "src/boot/headless_dispatch.rs",
        "pub(crate) fn pump_ipc(",
        0,
    ),
    // 함수 선언 바깥의 cfg는 본문 검사에서 보이지 않으므로 debug 전용 intercept_debug_app_layer는 등록하지 않는다.
    (
        "src/boot/headless_dispatch.rs",
        "fn intercept_app_layer(",
        5,
    ),
];

/// 주석·리터럴을 가린 소스로 함수 경계를 찾고 동일 이름의 정의를 모두 읽는다.
/// 서로 다른 정의의 cfg 범위가 섞이지 않도록 본문을 따로 반환한다.
fn fn_body_lines(src: &str, sig: &str) -> Vec<Vec<String>> {
    let name = sig
        .split_once("fn ")
        .and_then(|(_, rest)| rest.strip_suffix('('))
        .unwrap_or_else(|| panic!("시그니처가 `… fn <이름>(` 모양이 아니다: {sig}"));
    let source = Source::new(src);
    let bodies = source.fn_bodies(name);
    assert!(!bodies.is_empty(), "함수 시그니처를 찾지 못했다: {sig}");
    bodies
        .iter()
        .map(|body| {
            let mut lines: Vec<String> = source.slice(body).lines().map(str::to_owned).collect();
            join_wrapped_arms(&mut lines);
            lines
        })
        .collect()
}

/// 여러 줄의 match 패턴·화살표·guard를 합쳐 한 줄 판독에 맞춘다.
/// guard는 앞줄이 닫는 따옴표로 끝날 때만 합친다. 병합한 줄은 비워 원래 줄 번호와 cfg 대응을 유지한다.
fn join_wrapped_arms(lines: &mut [String]) {
    for i in (0..lines.len()).rev() {
        let t = lines[i].trim_start();
        let guard = starts_with_guard(t);
        if !(t.starts_with("=>") || t.starts_with('|') || guard) {
            continue;
        }
        let Some(prev) = (0..i).rev().find(|&j| !lines[j].trim().is_empty()) else {
            continue;
        };
        if lines[prev].trim_start().starts_with("//") {
            continue;
        }
        if guard && !lines[prev].trim_end().ends_with('"') {
            continue;
        }
        let merged = format!("{} {}", lines[prev].trim_end(), t);
        lines[prev] = merged;
        lines[i] = String::new();
    }
}
struct Arm<'a> {
    name: &'a str,
    /// 메서드 하나가 아닌 접두사 비교로 추출한 항목이다.
    is_prefix: bool,
}

/// match 패턴, == 비교, starts_with·strip_prefix의 문자열을 메서드 후보로 읽는다.
/// 수신자 타입이나 실제 라우팅 동작까지 분석하지는 않는다.
fn dispatch_methods(line: &str) -> Vec<Arm<'_>> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = line[from..].find('"') {
        let open = from + rel;
        let Some(close_rel) = line[open + 1..].find('"') else {
            break;
        };
        let close = open + 1 + close_rel;
        let name = &line[open + 1..close];
        let after = line[close + 1..].trim_start();
        let before = line[..open].trim_end();
        // 대안 패턴과 guard 때문에 문자열 바로 뒤에 =>가 없어도 match 패턴일 수 있다.
        let is_arm = after.starts_with("=>")
            || starts_with_guard(after)
            || (after.starts_with('|')
                && (after.contains("=>") || alternation_ends_in_guard(after)));
        let is_eq = before.ends_with("==");
        let is_prefix = before.ends_with("starts_with(") || before.ends_with("strip_prefix(");
        let shaped = name.contains('.')
            && !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.');
        if shaped && (is_arm || is_eq || is_prefix) {
            out.push(Arm { name, is_prefix });
        }
        from = close + 1;
    }
    out
}

fn starts_with_guard(t: &str) -> bool {
    t.strip_prefix("if")
        .is_some_and(|r| r.starts_with(|c: char| c.is_whitespace() || c == '('))
}

fn alternation_ends_in_guard(mut rest: &str) -> bool {
    while let Some(r) = rest.strip_prefix('|') {
        let r = r.trim_start();
        let Some(r) = r.strip_prefix('"') else {
            return false;
        };
        let Some(close) = r.find('"') else {
            return false;
        };
        rest = r[close + 1..].trim_start();
    }
    starts_with_guard(rest)
}

/// 앞의 연속 속성·주석·빈 줄에서 debug_assertions 문자열을 찾는다. 이 보조 검사는 조건식의 not·any 의미를 평가하지 않는다.
fn is_debug_gated(lines: &[String], idx: usize) -> bool {
    let mut i = idx;
    while i > 0 {
        i -= 1;
        let t = lines[i].trim();
        if t.starts_with("#[") {
            if t.contains("debug_assertions") {
                return true;
            }
            continue;
        }
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        return false;
    }
    false
}

fn debug_gated_lines(lines: &[String]) -> Vec<bool> {
    cfg_span::cfg_gated_lines(lines, "debug_assertions")
}

struct Scanned {
    name: String,
    is_prefix: bool,
    gated: bool,
}

fn scan_arms(lines: &[String]) -> Vec<Scanned> {
    let gated = debug_gated_lines(lines);
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        for arm in dispatch_methods(line) {
            out.push(Scanned {
                name: arm.name.to_string(),
                is_prefix: arm.is_prefix,
                gated: gated[i] || is_debug_gated(lines, i),
            });
        }
    }
    out
}

/// 접두사 항목은 같은 접두사로 시작하는 release 메서드가 하나만 있어도 등록된 것으로 본다.
fn registered_in_release_table(name: &str, is_prefix: bool) -> bool {
    if is_prefix {
        METHOD_TABLE.iter().any(|(m, _)| m.starts_with(name))
    } else {
        METHOD_TABLE.iter().any(|(m, _)| *m == name)
    }
}

#[test]
fn release_router_arms_are_registered_in_the_release_table() {
    let root = manifest_root();
    let mut offenders: Vec<String> = Vec::new();
    let mut scanned = 0usize;

    for (rel, sig, min_arms) in RELEASE_ROUTERS {
        let path = root.join(rel);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("라우터 소스를 읽을 수 없다: {}: {e}", path.display()));
        let arms: Vec<Scanned> = fn_body_lines(&src, sig)
            .iter()
            .flat_map(|lines| scan_arms(lines))
            .collect();
        let here = arms.len();
        for arm in arms {
            if arm.gated {
                continue;
            }
            if !registered_in_release_table(&arm.name, arm.is_prefix) {
                offenders.push(format!("{rel}: {}", arm.name));
            }
        }
        assert!(
            here >= *min_arms,
            "{rel}의 {sig}에서 메서드 후보를 {here} 개만 읽었다(하한 {min_arms}). 함수 경계와 판독 형식을 확인한다."
        );
        scanned += here;
    }

    assert!(
        scanned > 200,
        "release 라우터의 메서드 후보를 {scanned} 개만 읽었다. 수집 범위와 판독 형식을 확인한다."
    );
    offenders.sort();
    offenders.dedup();
    assert!(
        offenders.is_empty(),
        "release 라우터의 메서드 후보가 METHOD_TABLE에 없다. 표에서만 이름을 빼면 로컬 호출 접근을 막지 못한다. 입력 재현 기능이면 debug cfg를 적용하거나 debug 라우터로 옮긴다:\n  {}",
        offenders.join("\n  ")
    );
}

mod extractor_mutations {
    use super::*;

    fn lines(src: &str) -> Vec<String> {
        src.lines().map(str::to_owned).collect()
    }

    #[test]
    fn a_block_level_cfg_gates_the_arms_inside_it() {
        let src = "\
#[cfg(debug_assertions)]
{
    let rpc_id = id.clone();
    if method == \"debug.lua.eval\" { run(); }
    if method.starts_with(\"debug.event_bus.\") { run(); }
}
";
        let arms = scan_arms(&lines(src));
        assert_eq!(
            arms.len(),
            2,
            "세 형태 중 둘을 팔로 봐야 한다: {}",
            arms.len()
        );
        assert!(
            arms.iter().all(|a| a.gated),
            "블록 cfg 가 안쪽 팔에 상속되지 않았다 — 위양성이 그대로다"
        );
        assert!(
            arms.iter()
                .any(|a| a.is_prefix && a.name == "debug.event_bus."),
            "접두어 형을 못 봤다"
        );
    }

    #[test]
    fn removing_the_block_cfg_reopens_the_finding() {
        let src = "\
{
    let rpc_id = id.clone();
    if method == \"debug.lua.eval\" { run(); }
}
";
        let arms = scan_arms(&lines(src));
        assert!(
            arms.iter().any(|a| a.name == "debug.lua.eval" && !a.gated),
            "cfg 없는 블록의 팔을 gated 로 봤다 — 상속이 너무 넓다"
        );
    }

    #[test]
    fn an_arm_after_the_gated_block_is_still_release() {
        let src = "\
#[cfg(debug_assertions)]
{
    if method == \"debug.lua.eval\" { run(); }
}
if method == \"leaked.method\" { run(); }
";
        let arms = scan_arms(&lines(src));
        let leaked = arms
            .iter()
            .find(|a| a.name == "leaked.method")
            .expect("블록 뒤의 팔을 못 봤다");
        assert!(!leaked.gated, "블록의 게이트가 블록 밖으로 새어 나갔다");
    }

    #[test]
    fn braces_inside_strings_do_not_close_the_gated_block() {
        let src = "\
#[cfg(debug_assertions)]
{
    log(\"}\");
    if method == \"debug.lua.eval\" { run(); }
}
";
        let arms = scan_arms(&lines(src));
        assert!(
            arms.iter().any(|a| a.name == "debug.lua.eval" && a.gated),
            "문자열 안 `}}` 에 속아 블록이 일찍 닫혔다"
        );
    }

    #[test]
    fn a_guarded_arm_is_still_an_arm() {
        let src = "\
match method {
    \"leaked.guarded\" if matches!(caller, CallerContext::Local) => run(),
    \"leaked.alt_a\" | \"leaked.alt_b\" if ok => run(),
    \"leaked.multiline\" if matches!(
        caller,
        CallerContext::Local
    ) => run(),
    _ => {}
}
";
        let names: Vec<String> = scan_arms(&lines(src)).into_iter().map(|a| a.name).collect();
        for want in [
            "leaked.guarded",
            "leaked.alt_a",
            "leaked.alt_b",
            "leaked.multiline",
        ] {
            assert!(
                names.iter().any(|n| n == want),
                "guard 가 붙은 팔 `{want}` 을 못 봤다: {names:?}"
            );
        }
    }

    /// 실제 검사처럼 줄 병합 뒤 메서드를 추출한다.
    #[test]
    fn a_guard_wrapped_to_the_next_line_is_still_an_arm() {
        let src = "\
match method {
    \"leaked.wrapped\"
        if matches!(caller, CallerContext::Local) =>
    {
        run()
    }
    \"leaked.wrapped_a\"
    | \"leaked.wrapped_b\"
        if ok => run(),
    _ => {}
}
";
        let mut ls = lines(src);
        join_wrapped_arms(&mut ls);
        let names: Vec<String> = scan_arms(&ls).into_iter().map(|a| a.name).collect();
        for want in ["leaked.wrapped", "leaked.wrapped_a", "leaked.wrapped_b"] {
            assert!(
                names.iter().any(|n| n == want),
                "다음 줄로 내린 guard 의 팔 `{want}` 을 못 봤다: {names:?}"
            );
        }
    }

    #[test]
    fn a_plain_if_statement_is_not_joined_as_a_guard() {
        let src = "\
let x = run();
if method == \"kept.eq\" { run(); }
";
        let mut ls = lines(src);
        join_wrapped_arms(&mut ls);
        assert_eq!(ls[1].trim(), "if method == \"kept.eq\" { run(); }");
        assert_eq!(scan_arms(&ls).len(), 1);
    }

    #[test]
    fn a_prefix_arm_is_registered_only_if_the_table_has_something_under_it() {
        assert!(
            !registered_in_release_table("no.such.namespace.", true),
            "아무것도 없는 접두어를 등재됐다고 봤다"
        );
        let (first, _) = METHOD_TABLE[0];
        assert!(
            registered_in_release_table(first, false),
            "release 표에 있는 메서드를 미등록으로 판정했다"
        );
    }
}
