//! release IPC 표(`METHOD_TABLE`)에 **사용자 입력 재현** 메서드가 들어오면 실패한다.
//!
//! [`docs/identity.md`] 원칙 1 ②: 키/마우스 주입, popup 강제 조작, 메뉴 강제
//! invoke, 프로그래밍적 포커스 전환은 release IPC/CLI 표면에 존재하지 않고
//! `#[cfg(debug_assertions)]` 격리로만 제공된다. 정책·판단 기준은
//! [`docs/dev-guide/debug-ipc.md`], 이 가드가 생긴 결정은
//! [`docs/adr/0115-input-reproduction-ipc-debug-isolation.md`].
//!
//! # 판정 기준을 왜 이렇게 기계화했나
//!
//! "사용자 입력 재현인가" 는 의미 판단이라 완전 자동 판정이 불가능하다. 그래서
//! **자동으로 판정하지 않고, 이미 사람이 내려둔 판정과의 어긋남**을 잡는다 —
//! 어긋남은 기계적으로 판정 가능하고, 실제로 이 가드를 만든 결함이 정확히 그
//! 형태였다(`surface.raw_key`·`surface.switch_input_source`·`surface.ime_*` 는
//! CLI 에서는 debug 전용인데 IPC 표에서는 release 였다).
//!
//! - **가드 1 (CLI↔IPC 정합)** — debug CLI(`DebugCommands`, 모듈째
//!   `#![cfg(debug_assertions)]`)가 부르는 메서드는 IPC 에서도 debug 여야 한다.
//!   같은 기능의 두 표면이 서로 다른 빌드에 노출되는 것 자체가 결함이다.
//!   예외는 [`CLI_DEBUG_RELEASE_METHODS`] 에 근거와 함께 명시한다.
//! - **가드 2 (이름 규칙)** — 입력 재현 계열임이 이름에 드러나는 형태
//!   ([`INPUT_REPRODUCTION_PATTERNS`])는 release 표에 못 들어온다. 이름만으로
//!   판정되는 좁은 집합이라 오탐이 적고, 새 메서드에도 선제적으로 걸린다.
//! - **가드 3 (prefix 규칙)** — `surface.ime_*` 는 개별 등재가 아니라
//!   `PREFIX_RULES` 로 해소되므로 위 두 가드의 사각지대다. 그 규칙이
//!   `#[cfg(debug_assertions)]` 아래 있는지 소스에서 직접 확인한다.
//!
//! - **가드 4 (라우터 ↔ 표 정합)** — 위 셋은 전부 *등재 표* 를 지킨다. 그런데
//!   release 차단의 부하를 실제로 지는 곳은 **라우터의 cfg** 다: 권한 게이트는
//!   `CallerContext::Local` 을 무조건 통과시키고(`crates/tasty-ipc/src/caller.rs`),
//!   디스패치 경로는 `method_meta()` 를 아예 부르지 않는다. 즉 **표에서 빼도
//!   release 라우터에 팔이 남아 있으면 로컬 IPC 호출자에게 그대로 열린다.**
//!   그래서 release 라우터 함수의 팔이 release 표(`METHOD_TABLE`)에 없으면 실패한다.
//!
//!   기존 [`tests/ipc_router_table_parity.rs`] 는 이걸 못 잡는다 — 그 가드는
//!   "라우터에 팔이 있으면 어느 표에든 등재돼 있어야 한다" 를 보는데, 테스트는 항상
//!   debug 로 컴파일되므로 `DEBUG_METHODS` 가 채워져 있어 debug 표만으로 "등재됨" 이
//!   된다. **표는 debug, 팔은 release** 라는 정확히 이 결함 형태가 그 가드를 통과한다.
//!
//! 이 가드들이 **잡지 못하는 것**: 이름에 단서가 없고 debug CLI 진입점도 없는
//! 새 release 메서드의 의미 판단. 그건 사람이 리뷰에서 본다 — 자동화의 목표는
//! "이미 내려진 판정이 조용히 뒤집히는 것" 을 막는 데까지다. 가드 4 는 팔 바로 위의
//! `#[cfg]` 줄만 읽으므로, 팔을 `#[cfg(debug_assertions)] { .. }` **블록**으로 감싸는
//! 형태는 debug 로 인식하지 못하고 위양성이 난다 — 현재 라우터에는 그 형태가 없다.
//!
//! release 빌드에서는 `DEBUG_METHODS` 가 빈 슬라이스라 대조가 성립하지 않으므로
//! `#![cfg(debug_assertions)]` 로 debug 에서만 돈다.
//!
//! **채널 — 컴파일은 두 조합 모두 자동, 실행은 한 조합만이다.** `tests/*.rs` 라 이 파일이
//! 컴파일되는지는 CI 가 두 조합에서 본다(`crossplatform-check.yml` 의 Windows
//! `clippy --all-targets` 와 headless `clippy --all-targets --no-default-features`).
//! 실행 쪽은 자동 실행은 **헤드리스 조합**(`check-headless` 의 전체 스위트)에서만 일어난다
//! (기본 조합 잡은 `--lib --bins` 라 통합 타깃을 못 본다 — `docs/dev-guide/ci-gates.md`).
//! `test.yml` 의 기본 조합 전체 스위트는 `workflow_dispatch` 전용이라 거기엔 채널이 없다.
//! (채널 정본은 [ci-gates](../docs/dev-guide/ci-gates.md)).
//! 따라서 이 가드를 근거로 로컬 검증을 건너뛰지 않는다.
#![cfg(debug_assertions)]

use std::path::Path;

use tasty_ipc::method_meta::METHOD_TABLE;

/// debug CLI 가 부르지만 **release IPC 로 두는 것이 맞는** 메서드.
///
/// 여기 추가하는 것은 "debug CLI 진입점이 있는데 IPC 는 release" 를 의도로
/// 선언하는 일이다. 근거를 한 줄로 남기지 않을 거면 추가하지 마라.
const CLI_DEBUG_RELEASE_METHODS: &[(&str, &str)] = &[
    // `tasty debug attach` 는 로컬 loopback self-attach 라 debug 전용이지만,
    // 그 안에서 쓰는 force-detach 자체는 원격 attach(release)와 공용 메커니즘이라
    // release JSON-RPC 다. docs/dev-guide/debug-ipc.md "tasty debug attach" 참조.
    (
        "attach.force_detach",
        "원격 attach 와 공용 — release 정식 표면",
    ),
    (
        "attach.force_detach_workspace",
        "원격 attach 와 공용 — release 정식 표면",
    ),
];

/// release 표에 있으면 안 되는 이름 형태. `(패턴, 무엇을 겨냥하나)`.
///
/// 이름만으로 입력 재현임이 드러나는 좁은 집합이다 — 넓히면 정상 에이전트
/// 메서드를 오탐한다. 대조: `surface.send_key` 는 대상 surface ID 를 받아 그
/// PTY 에 바이트를 쓰는 정상 에이전트 동작이라 어느 패턴에도 걸리지 않는다.
const INPUT_REPRODUCTION_PATTERNS: &[(&str, &str)] = &[
    ("inject", "키·마우스 이벤트 주입"),
    ("raw_key", "OS 이벤트 스트림 키 주입"),
    ("switch_input_source", "OS 전역 입력 소스 전환"),
    ("ime_", "입력기 조합 상태 강제 세팅"),
    ("simulate", "입력 시뮬레이션"),
];

/// 메서드 이름의 마지막 세그먼트가 이것이면 release 표에 있으면 안 된다.
/// (`window.focus` / `view.focus` — 프로그래밍적 포커스 전환.)
const FORBIDDEN_LAST_SEGMENTS: &[&str] = &["focus"];

fn manifest_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// 가드 1 — debug CLI 가 부르는 메서드는 release 표에 없어야 한다.
#[test]
fn debug_cli_methods_are_not_in_the_release_table() {
    let path = manifest_root().join("crates/tasty-cli/src/request/debug.rs");
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "debug CLI 요청 매핑을 읽을 수 없다: {}: {e}",
            path.display()
        )
    });

    // `"ns.method"` 형태의 리터럴만. i18n 키(`cli.debug.*`)는 표에 없어서
    // 자연히 걸러진다 — 별도 예외 목록이 필요 없다.
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

/// 가드 2 — 입력 재현임이 이름에 드러나는 메서드는 release 표에 없어야 한다.
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

/// 가드 3 — `surface.ime_*` prefix 규칙은 debug 빌드에만 존재해야 한다.
///
/// prefix 로 해소되는 메서드는 `METHOD_TABLE` 을 훑는 위 두 가드에 걸리지 않아
/// 사각지대다. `PREFIX_RULES` 정의에 cfg 격리가 붙어 있는지 소스에서 확인한다.
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

/// release 빌드에서도 컴파일되는 디스패치 함수 — `(파일, 함수 시그니처 시작, 최소 팔 수)`.
///
/// 여기 없는 라우터(`route_debug_handler` · `ipc_step_debug` ·
/// `ipc_step_window_required` · `handler/ime.rs`)는 함수·모듈째
/// `#[cfg(debug_assertions)]` 라 release 바이너리에 존재하지 않는다.
///
/// 세 번째 값은 그 함수에서 최소한 이만큼은 팔을 찾아야 한다는 하한이다 — 중괄호
/// 스캔이 깨져 본문을 짧게 읽으면 가드가 **아무것도 검사하지 않은 채 통과**하므로,
/// 실측값보다 조금 낮게 잡아 미스캔을 실패로 드러낸다.
const RELEASE_ROUTERS: &[(&str, &str, usize)] = &[
    (
        "src/adapters/ipc/handler.rs",
        "fn route_engine_handler(",
        200,
    ),
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
    (
        "src/boot/headless_dispatch.rs",
        "pub(crate) fn pump_ipc(",
        1,
    ),
];

/// `sig` 로 시작하는 함수의 본문 줄 범위를 돌려준다(중괄호 짝 세기, 문자열 리터럴 제외).
fn fn_body_lines(src: &str, sig: &str) -> Vec<String> {
    let start = src
        .find(sig)
        .unwrap_or_else(|| panic!("함수 시그니처를 찾지 못했다: {sig}"));
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    let mut end = src.len();
    let mut opened = false;
    for (i, c) in src[start..].char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => {
                depth += 1;
                opened = true;
            }
            '}' => {
                depth -= 1;
                if opened && depth == 0 {
                    end = start + i;
                    break;
                }
            }
            _ => {}
        }
    }
    let mut lines: Vec<String> = src[start..end].lines().map(str::to_owned).collect();
    join_wrapped_arms(&mut lines);
    lines
}

/// `=>` 나 `|` 가 다음 줄로 넘어간 match 팔을 **원래 줄로 끌어올린다.**
///
/// 팔 인식이 한 줄 단위라, 아래 같은 형태는 그대로면 팔로 보이지 않는다.
///
/// ```text
/// "surface.raw_key"
///     => handle(..),
/// ```
///
/// rustfmt 가 이 형태를 한 줄로 되돌리므로(`cargo fmt --check` 가 CI·pre-commit 에서
/// 강제한다) 실제로 레포에 들어올 수는 없다. 그래도 정규화해 둔다 — 가드의 회피
/// 난이도를 포매터 하나에만 의존시키지 않기 위함이다.
///
/// 병합한 줄은 **빈 줄로 남긴다.** 줄 번호가 밀리면 `is_debug_gated` 가 팔 위의
/// `#[cfg]` 를 잘못 짚는다.
fn join_wrapped_arms(lines: &mut [String]) {
    for i in (0..lines.len()).rev() {
        let t = lines[i].trim_start();
        if !(t.starts_with("=>") || t.starts_with('|')) {
            continue;
        }
        // 위로 올라가며 가장 가까운 비어있지 않은 줄에 붙인다.
        let Some(prev) = (0..i).rev().find(|&j| !lines[j].trim().is_empty()) else {
            continue;
        };
        if lines[prev].trim_start().starts_with("//") {
            continue;
        }
        let merged = format!("{} {}", lines[prev].trim_end(), t);
        lines[prev] = merged;
        lines[i] = String::new();
    }
}

/// 디스패치 위치의 메서드 이름 리터럴만 뽑는다 — `"ns.method" =>` (match 팔) 과
/// `method == "ns.method"` (if 형) 두 형태. 응답 payload 의 문자열은 이 두 문법
/// 위치가 아니라서 걸리지 않는다.
fn dispatch_methods(line: &str) -> Vec<&str> {
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
        // `"a" | "b" => ..` 의 앞쪽 팔은 `=>` 가 아니라 `|` 가 뒤따른다. 같은 줄
        // 뒤쪽에 `=>` 가 있으면 그것도 팔로 센다.
        let is_arm = after.starts_with("=>") || (after.starts_with('|') && after.contains("=>"));
        let is_eq = before.ends_with("==");
        let shaped = name.contains('.')
            && !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.');
        if shaped && (is_arm || is_eq) {
            out.push(name);
        }
        from = close + 1;
    }
    out
}

/// `lines[idx]` 바로 위의 연속 attribute/주석/빈 줄을 훑어 `debug_assertions` cfg 가
/// 붙어 있는지 본다. attribute·주석·빈 줄이 아닌 줄을 만나면 멈춘다.
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

/// 가드 4 — release 라우터의 팔은 release 표에 등재돼 있어야 한다.
#[test]
fn release_router_arms_are_registered_in_the_release_table() {
    let root = manifest_root();
    let mut offenders: Vec<String> = Vec::new();
    let mut scanned = 0usize;

    for (rel, sig, min_arms) in RELEASE_ROUTERS {
        let path = root.join(rel);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("라우터 소스를 읽을 수 없다: {}: {e}", path.display()));
        let lines = fn_body_lines(&src, sig);
        let mut here = 0usize;
        for (i, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for name in dispatch_methods(line) {
                here += 1;
                if is_debug_gated(&lines, i) {
                    continue;
                }
                if !METHOD_TABLE.iter().any(|(m, _)| m == &name) {
                    offenders.push(format!("{rel}: {name}"));
                }
            }
        }
        assert!(
            here >= *min_arms,
            "`{rel}` 의 `{sig}` 에서 팔을 {here} 개밖에 못 찾았다(하한 {min_arms}) — \
             중괄호 스캔이나 팔 인식이 깨졌을 가능성이 크다. 조용한 미스캔은 \
             위양성보다 나쁘므로 여기서 실패시킨다."
        );
        scanned += here;
    }

    assert!(
        scanned > 200,
        "release 라우터 팔을 {scanned} 개밖에 못 찾았다 — 스캔 패턴이 깨졌을 가능성이 크다"
    );
    offenders.sort();
    offenders.dedup();
    assert!(
        offenders.is_empty(),
        "release 빌드에서도 컴파일되는 라우터에 팔이 있는데 release 표(METHOD_TABLE)에 \
         없는 메서드가 있다. **표에서 빼는 것만으로는 release 표면이 닫히지 않는다** — \
         권한 게이트가 로컬 호출자를 무조건 통과시키고 디스패치는 method_meta() 를 \
         부르지 않으므로, 팔이 남아 있으면 로컬 IPC 로 그대로 도달한다. 팔에 \
         `#[cfg(debug_assertions)]` 를 걸거나 debug 라우터로 옮겨라:\n  {}",
        offenders.join("\n  ")
    );
}
