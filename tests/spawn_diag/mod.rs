//! 인스턴스 spawn 의 **공통 설정과 실패 판정**을 두 하네스가 공유하는 자리.
//!
//! `tests/common`(범용 인스턴스)과 `tests/webhook_common`(웹훅 인스턴스)은 같은
//! 바이너리를 같은 방식으로 띄운다([`instance_bin`] 이 그 하나를 정한다). 그런데 상한값이 서로 달랐고
//! (30/15 vs 40/20) 그 차이의 근거가 어디에도 없었다 — 같은 단계에 다른 잣대를
//! 대면 한쪽에서만 재현되는 flaky 가 생기고, 그때마다 "이 하네스는 원래 느린가" 를
//! 사람이 다시 판단해야 한다. 값을 여기 하나로 모은다. 자식의 로그 필터도 같다.
//!
//! 실패 원인 판정도 같이 둔다. spawn timeout 은 "느린 것" 과 "부팅이 아예 막힌 것"
//! 이 똑같은 메시지로 보였는데, 둘은 대응이 완전히 다르다(전자는 기다리면 되고
//! 후자는 기다려도 안 된다). 디스플레이 서버 부재는 stderr 시그니처로 갈리고,
//! 자식이 이미 죽은 경우는 조기 종료 감지로 갈린다.

#![allow(dead_code)]
// 두 하네스가 각자 일부만 쓴다

// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다 — 전수 가드
// (`tests/let_underscore_documented.rs`)가 테스트 본문을 제외하므로, 여기서 나는
// `let_underscore_must_use` 경고는 정책상 조치 대상이 될 수 없다. 끄지 않으면
// 프로덕션의 진짜 신호가 그 안에 묻힌다 — `docs/dev-guide/error-handling.md`.
#![allow(clippy::let_underscore_must_use)]

use std::time::Duration;

/// 제품이 로그 필터로 읽는 환경변수. **`RUST_LOG` 이 아니다** —
/// `src/platform/crash_report.rs` 의 `EnvFilter::try_from_env("TASTY_LOG")` 다.
/// 한 번 `RUST_LOG` 로 잘못 넣어 두 하네스가 몇 달 동안 필터 없이 돌았으므로,
/// 이름은 상수로 고정하고 `tests/harness_log_env.rs` 가 제품 소스와 대조한다.
pub const LOG_ENV: &str = "TASTY_LOG";

/// 필터 문자열의 유일한 정의 자리. 두 상수가 같은 리터럴을 각자 적으면 한쪽만
/// 고쳐져 어긋나므로 매크로로 한 번만 쓴다(`concat!` 은 리터럴만 받는다).
macro_rules! product_default_filter {
    () => {
        "warn,wgpu_hal=error,wgpu_core=error,naga=error,egui_winit::clipboard=off"
    };
}

/// 자식에게 주는 기본 로그 필터 — **제품 기본값과 같은 모양**이어야 한다.
///
/// `TASTY_LOG` 를 지정하는 순간 제품의 기본 필터는 통째로 대체된다. 그래서 `warn`
/// 한 단어만 주면 기본값에 들어 있던 억제(`wgpu_hal=error` 등)가 전부 풀려
/// **로그가 오히려 늘어난다** — 실측(격리 HOME, 정상 부팅, 12초): env 미지정 7줄 ·
/// `warn` 12줄(wgpu 5줄) · 이 값 7줄. 늘어난 줄은 `STDERR_TAIL_LINES`(30) 짜리
/// 진단 tail 을 그대로 밀어낸다. host 의 `TASTY_LOG=trace` 누수를 막으면서 노이즈는
/// 늘리지 않으려면 기본값과 같은 모양을 명시하는 수밖에 없다.
pub const LOG_FILTER: &str = product_default_filter!();

/// 웹훅 하네스용 필터 — [`LOG_FILTER`] 를 **그대로 앞에 두고** 뒤에만 덧붙인다.
///
/// 덧붙이는 것은 리스너 타깃의 `info` 한 줄(`webhook listener bound on {addr}`)이다.
/// 도난 **판정**에는 필요 없다 — 그건 제품이 `warn!` 으로 내므로 기본 필터에서도
/// 보인다. 필요한 것은 실패 보고를 사람이 읽을 때다: 실패 tail 에 `bound` 줄이
/// 있으면 "떴는데 connect 가 안 됐다", 없으면 "끝내 안 떴다" 로 갈린다.
pub const LOG_FILTER_WEBHOOK: &str =
    concat!(product_default_filter!(), ",tasty::webhook::listener=info");

/// libtest 캡처로 흘러가는 `tracing` subscriber 를 설치한다(프로세스당 1회, 실패는 무시).
///
/// 하네스 진단을 `eprintln!` 로 쓰면 훅 C.11 의 예외가 필요해진다. 예외를 만들지 않고도
/// 같은 자리에 출력이 남는다는 것을 실측으로 확인했다 — `with_test_writer()` 는 libtest 의
/// per-thread 캡처를 타므로, 실패한 테스트의 출력 블록에 `eprintln!` 과 나란히 찍힌다.
/// 이미 설치돼 있으면 `try_init` 이 `Err` 를 주고 그대로 두는 것이 맞다.
pub fn init_test_tracing() {
    // 두 번째 설치 시도는 정상 흐름이다(같은 바이너리의 다른 테스트가 이미 설치).
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
}

/// 하네스가 띄울 tasty 바이너리를 정하는 **유일한 자리**.
///
/// 기본값은 `CARGO_BIN_EXE_tasty` — **테스트 자신과 같은 feature 로 빌드된 자기
/// 바이너리**다. 그래서 기본(gui) 조합에서는 창과 GPU 디바이스를 만드는 바이너리가
/// 뜨고, `--no-default-features` 조합에서는 같은 경로가 곧 headless 데몬이 된다.
/// IPC 만 쓰는 스위트가 GPU 를 통과해야 하는 이유는 여기에 있다
/// (`docs/adr/0127-e2e-harness-binary-selection.md`).
///
/// `TASTY_E2E_BIN` 이 설정돼 있으면 그 경로를 대신 띄운다. 용도는 **미리 빌드해 둔
/// headless 바이너리를 가리키는 것** — 워크트리 여러 개가 같은 GPU 를 다투는 상황에서
/// IPC 전용 스위트를 GPU 밖으로 빼는 로컬 탈출구다. 절차와 함정은
/// `docs/dev-guide/e2e-tests.md`.
///
/// **함정**: `CARGO_BIN_EXE_tasty` 와 headless 빌드는 `target/debug/tasty` 라는 같은
/// 경로를 다툰다. 따라서 override 는 반드시 **별도 `CARGO_TARGET_DIR` 로 빌드한
/// 산출물의 경로**여야 한다 — 같은 target 디렉토리에 headless 를 빌드하면 다음
/// `cargo test` 가 그것을 gui 로 덮어써서, 아무것도 바뀌지 않았는데 override 가
/// 듣는 것처럼 보인다. 경로로 확정하지 않으면 검증이 조용히 다른 것을 잰다.
///
/// **함정 2**: 그 target 디렉토리를 **레포 밖에 두면** plugin 번들이 안 만들어진다. host 는
/// `exe_dir/builtin-plugins` 를 먼저 보고, 없으면 exe 의 두 단계 위를 워크스페이스 루트로
/// 역산하는데 — 레포 밖 디렉토리에서는 그 역산이 `crates/` 없는 경로를 가리켜 실패하고
/// 데몬이 plugin namespace 없이 올라온다.
///
/// **함정 3**: `--workspace` 없이 빌드하면 그 target 에 `tasty-plugin-*` 바이너리가 **하나도
/// 안 생긴다**. 함정 2 와 독립이다 — 레포 안에 두어 역산이 맞아도 동기화할 바이너리가 없다.
///
/// 두 함정의 증상이 같다: `Method not found: <plugin namespace>.<method>`. 이는 §0 의 stale
/// plugin drift 및 "headless 에 아직 배선되지 않은 경로" 와도 **문구가 같아** 빌드 절차 결함이
/// IPC 표면 차이로 오독된다. 그래서 override 절차는 레포 안 target + `--workspace` 둘 다를
/// 요구한다 (`docs/dev-guide/e2e-tests.md` §0-1 — 세 팔 실측표가 두 조건을 따로 가른다).
///
/// **함정 4 (닫혔다)**: 이 override 는 **데몬만** 조합을 바꾼다. 테스트 바이너리는
/// 자기 조합으로 컴파일된 채라, 데몬 동작을 `cfg(feature = "gui")` 로 갈라 단언하는
/// 테스트는 단언이 구조적으로 뒤집힌다. 그래서 **그런 단언을 가진 스위트는 override 를
/// 받지 않는다** — [`daemon_kind`] 가 스위트별로 가르고, `e2e_tests` 가 그 하나다.
/// 실측 2026-09-05(닫기 전): gui 테스트 바이너리 + 헤드리스 데몬으로 11 스위트를 돌려
/// `e2e_tests` 만 5 건 깨졌고, 그중 4 건이 조합 교차였다. 지금은 그 스위트가 자기
/// 조합의 데몬을 그대로 띄우므로 그 4 건이 나지 않는다.
///
/// 존재하지 않는 경로를 주면 spawn 이 "그냥 실패" 하는 대신 **여기서** 죽는다 —
/// 30 초를 기다린 뒤 port file 미작성으로 오진되는 것을 막는다.
pub fn instance_bin() -> std::ffi::OsString {
    let from_env = std::env::var_os(INSTANCE_BIN_ENV);
    // 경로 검증은 **이 스위트가 override 를 쓰든 안 쓰든** 한다. 오타를 쓴 사람은
    // 어느 스위트를 돌리든 그 자리에서 알아야 하고, 안 그러면 "왜 안 듣지" 가 된다.
    if let Some(v) = from_env.as_deref()
        && !v.is_empty()
        && !std::path::Path::new(v).is_file()
    {
        panic!(
            "{INSTANCE_BIN_ENV} 가 가리키는 경로에 실행 파일이 없다: {}\n\
             별도 CARGO_TARGET_DIR 로 빌드한 산출물의 절대경로여야 한다 — \
             docs/dev-guide/e2e-tests.md",
            std::path::Path::new(v).display()
        );
    }
    let effective = effective_override(daemon_kind(), from_env);
    if let Some(v) = effective.as_deref()
        && !v.is_empty()
        && let Some(newer) = source_newer_than(std::path::Path::new(v), repo_roots())
    {
        panic!(
            "{INSTANCE_BIN_ENV} 가 가리키는 바이너리가 소스보다 낡았다.\n\
             \x20 바이너리: {}\n\x20 더 새 소스: {}\n\
             낡은 데몬은 **정상 부팅해 정상 응답한다** — 그래서 이 스위트는 옛 코드에 대해 \
             통과하거나 실패하고, 그 오진은 양방향이다(고친 것이 안 고쳐진 것처럼도, \
             되돌린 것이 여전히 고쳐진 것처럼도 보인다).\n\
             다시 빌드하거나(`scripts/build-e2e-headless.sh`) `{INSTANCE_BIN_ENV}=` 로 꺼라 \
             — docs/dev-guide/e2e-tests.md",
            std::path::Path::new(v).display(),
            newer.display()
        );
    }
    resolve_instance_bin(effective.as_deref(), env!("CARGO_BIN_EXE_tasty"))
}

/// override 가 **이 스위트에 실제로 적용되는가**. 순수 함수로 둔다 — 환경변수를
/// 건드리지 않고 두 갈래를 다 시험할 수 있어야 한다.
///
/// 조합 의존 단언을 가진 스위트가 override 를 안 받는 것이 **이 설계의 안전장치
/// 전부**다. 그것이 무너지면 데몬만 조합이 바뀌어 그 단언들이 구조적으로 뒤집힌다
/// (함정 4). 그래서 여기에 테스트가 붙어 있다.
fn effective_override(
    kind: DaemonKind,
    from_env: Option<std::ffi::OsString>,
) -> Option<std::ffi::OsString> {
    match kind {
        DaemonKind::SameCombo => None,
        DaemonKind::HeadlessOk => from_env,
    }
}

/// 낡음 판정이 훑을 소스 뿌리. 문서·워크플로는 데몬 동작을 안 바꾸므로 안 본다.
fn repo_roots() -> Vec<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    vec![root.join("src"), root.join("crates")]
}

/// `bin` 보다 **새로운** `.rs` 가 하나라도 있으면 그 경로를 준다.
///
/// 첫 하나에서 멈춘다 — 몇 개가 새것인지는 판정에 필요 없고, 전수로 훑으면 회차마다
/// 무는 비용이 된다. 실측 10~14 ms(`find` 등가).
///
/// **mtime 을 못 읽는 경로는 "새것 아님" 으로 넘긴다.** 판정 불가를 빨강으로 만들면
/// 권한·심볼릭 링크 같은 환경 차이가 곧바로 거짓 빨강이 되는데, 이 판정의 목적은
/// 낡은 것을 잡는 것이지 파일시스템을 검사하는 것이 아니다.
fn source_newer_than(
    bin: &std::path::Path,
    roots: Vec<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    let bin_mtime = std::fs::metadata(bin).and_then(|m| m.modified()).ok()?;
    let mut stack = roots;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if p.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let newer = std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .map(|m| m > bin_mtime)
                .unwrap_or(false);
            if newer {
                return Some(p);
            }
        }
    }
    None
}

/// 이 스위트가 어떤 데몬을 원하는가.
///
/// 두 값의 차이는 **조합 의존 단언을 가지는가** 하나다. 가진 스위트는 데몬이
/// 테스트 바이너리와 같은 조합이어야 그 단언이 뜻을 갖고, 안 가진 스위트는
/// 헤드리스 데몬으로 충분하다 — 그리고 그러면 그 스위트는 GUI 부팅을 통째로
/// 건너뛴다(창 + wgpu 디바이스 + boot 상태기계).
pub enum DaemonKind {
    /// 데몬이 **테스트 바이너리와 같은 조합**이어야 한다. override 를 무시한다.
    SameCombo,
    /// IPC / attach 스트림만 쓴다 — 헤드리스 데몬으로 충분하다.
    HeadlessOk,
}

/// 인스턴스를 띄우는 스위트 중 **헤드리스 데몬으로 충분한 것들.**
///
/// 여기 없는 스위트는 [`DaemonKind::SameCombo`] 로 떨어진다 — **모르는 것은 안전한
/// 쪽으로 보낸다.** 새 스위트가 조합 의존 단언을 갖고 들어왔는데 목록이 기본으로
/// 헤드리스면 override 를 켠 사람에게 **틀린 빨강**이 가지만, 반대 방향의 누락은
/// "최적화를 놓친다" 로 끝난다. 두 오류가 비대칭이라 기본값을 이쪽으로 둔다.
///
/// 명부가 `EXPECTED_INSTANCE_TESTS` 와 어긋나지 않는 것은
/// `tests/e2e_single_instance_guard.rs` 가 본다.
const HEADLESS_OK_SUITES: &[&str] = &[
    "attach_attention_loopback",
    "attach_convert_cwd_loopback",
    "attach_git_query_loopback",
    "attach_list_dir_loopback",
    "attach_local_creation_tap",
    "attach_silent_disconnect",
    "hook_env_integration",
    "hooks_detection_e2e",
    "shared_instance_harness",
    "soak_memory",
    "webhook_integration",
];

/// 이 스위트의 판정. `CARGO_CRATE_NAME` 은 통합 테스트에서 **test 타깃 이름**으로
/// 확장되고(실측), 이 모듈은 각 테스트 바이너리에 함께 컴파일되므로 스위트마다
/// 다른 값이 된다.
///
/// **`e2e_tests` 만 [`DaemonKind::SameCombo`] 다.** 실측 2026-09-05: 인스턴스를 띄우는
/// 11 스위트 중 `cfg(feature = "gui")` 계열 사이트를 가진 것은 `e2e_tests.rs` 하나였고
/// (10 사이트), 나머지 10 개는 각 0 이었다. 실행 쪽 확인도 있다 —
/// `docs/dev-guide/e2e-tests.md` §0-1 이 gui 테스트 바이너리 + 헤드리스 데몬으로
/// 11 스위트를 돌려 10 개가 통과하고 `e2e_tests` 만 깨진 것을 기록해 두었다.
/// `gui_tests` 는 애초에 이 경로를 안 쓴다(`BIN_SELECTION_ALLOWLIST`).
pub fn daemon_kind() -> DaemonKind {
    if HEADLESS_OK_SUITES.contains(&env!("CARGO_CRATE_NAME")) {
        DaemonKind::HeadlessOk
    } else {
        DaemonKind::SameCombo
    }
}

/// [`instance_bin`] 의 선택 규칙만 떼어낸 것 — 환경변수를 건드리지 않고 시험할 수
/// 있게 순수 함수로 둔다(테스트가 병렬로 도는데 `set_var` 는 프로세스 전역이다).
///
/// 빈 문자열은 **미설정과 같게** 다룬다. 셸에서 `TASTY_E2E_BIN=` 로 비우는 것이
/// "기본으로 되돌린다" 는 뜻으로 읽히는 것이 자연스럽고, 빈 경로를 그대로 spawn 하면
/// 원인을 알 수 없는 실패가 된다.
fn resolve_instance_bin(
    from_env: Option<&std::ffi::OsStr>,
    default_bin: &str,
) -> std::ffi::OsString {
    match from_env {
        Some(v) if !v.is_empty() => v.to_os_string(),
        _ => std::ffi::OsString::from(default_bin),
    }
}

/// 하네스가 띄울 바이너리를 덮어쓰는 환경변수 이름.
pub const INSTANCE_BIN_ENV: &str = "TASTY_E2E_BIN";

/// S1 — `--port-file` 에 포트가 쓰이기까지. GUI 부팅(창 + GPU 디바이스 + boot
/// 상태기계)이 끝나야 IPC 가 시작되므로 이 단계가 가장 길다.
///
/// 값의 근거: dev cold path worst-case(GPU init + plugin discover/extract +
/// theme/db init, dev 프로필이 release 의 ~3.5 배) + self-hosted runner 변동 폭.
/// 두 하네스가 쓰던 30 s / 40 s 중 **큰 쪽**으로 맞춘다 — 웹훅 하네스의 상한을
/// 낮추는 것은 근거 없는 동작 축소이고, 반대로 올리는 쪽은 *이미 실패할 spawn* 이
/// 보고되기까지의 시간만 늘린다. 그 시간은 [`early_exit_message`] 경로가 대부분
/// 없앤다(자식이 죽었으면 상한을 기다리지 않는다).
pub const SPAWN_PORT_TIMEOUT: Duration = Duration::from_secs(40);

/// S2 — 첫 surface 의 PTY 가 프롬프트를 낼 때까지. S1 이 끝난 뒤라 GPU 와 무관하다.
pub const SPAWN_SHELL_TIMEOUT: Duration = Duration::from_secs(20);

/// 공유 인스턴스의 **첫 spawn 이 실패하면 다시 시도하지 않게** 막는 래치.
///
/// `OnceLock::get_or_init` 은 초기화 클로저가 panic 하면 **미초기화 상태로 남는다.**
/// 그래서 다음 테스트가 그 클로저를 그대로 다시 돈다 — 부팅이 막힌 조건(디스플레이
/// 부재·GPU 초기화 실패·포트 파일 미작성)에서는 **테스트 수만큼 프로세스가 실제로 더
/// 뜨고** 각각이 상한까지 기다린다. 실측(`gui_tests`, 디스플레이 없이 6 건):
/// 래치 없이 spawn 시도 **6 회**, 패닉 자리는 **1 곳**. 즉 실패 6 건이 사건 1 개다.
///
/// **왜 공유 static 이 아니라 타입인가.** 래치가 지켜야 하는 것은 "이 하네스의 공유
/// 인스턴스" 하나다. 한 test binary 가 하네스를 둘 이상 품으면 static 하나로는 한쪽의
/// 실패가 다른 쪽의 spawn 을 막아 **가짜 실패**를 만든다. 그래서 기전만 여기서 공유하고
/// 상태는 하네스가 각자 자기 `static` 으로 갖는다.
///
/// 사용법 — `get_or_init` 클로저의 **첫 줄**과 spawn 직후:
/// ```ignore
/// static SPAWN_LATCH: spawn_diag::SpawnOnceLatch = spawn_diag::SpawnOnceLatch::new();
/// SHARED_INSTANCE.get_or_init(|| {
///     SPAWN_LATCH.entering("gui 공유 인스턴스");
///     let inst = Instance::spawn();
///     SPAWN_LATCH.succeeded();
///     inst
/// })
/// ```
///
/// **이 처방을 재는 채널은 반만 있다 — 갈라서 적는다.**
///
/// | 무엇을 재나 | 채널 |
/// |---|---|
/// | 래치 **타입**이 계약대로 도는가 | `the_latch_blocks_the_second_spawn_and_a_success_releases_it` — 있다. 이 모듈을 들이는 **14 개 test binary** 에서 돈다 |
/// | 하네스가 래치를 **맞는 자리에 뒀는가** | ★ **없다** |
///
/// 뒤쪽이 없는 이유는 이 타입의 결함이 아니라 배선의 성질이다. 위 사용법이 요구하는 것은
/// "`entering` 이 `get_or_init` 클로저 **안** 첫 줄에 있을 것" 인데, 그 줄이 클로저 밖으로
/// 나가도 **컴파일되고 단위 시험도 초록**이다. 클로저 밖에서는 `OnceLock` 이 그 자리를 한
/// 번만 부르므로 래치가 영영 안 걸리고, 그 사실은 **부팅이 막힌 환경에서 벽시계로만**
/// 드러난다. 그 조건을 자동 잡이 만들지 않는다 — `gui_tests` 는 어떤 자동 채널도 돌리지
/// 않고(`check-headless` 도 안 본다), 나머지 두 하네스는 그 환경에서 부팅에 성공한다.
///
/// 그래서 배선의 판정은 **사람이 슬롯에서 벽시계를 재는 것**뿐이다. 재는 법: 부팅이 막힌
/// 환경에서 그 binary 를 통째로 돌리고 (1) spawn 시도 수가 **1** 인가, (2) 실패 건수는
/// 그대로인가(래치는 수를 안 줄인다 — 시간과 메시지를 바꾼다), (3) 두 번째 이후 실패가
/// [`SpawnOnceLatch::entering`] 의 문장을 달고 나오는가를 본다. 세 값이 다 맞아야 배선이
/// 확인된다. 실측 예: `gui_tests` 33 건이 래치 이전에 **546 s**(≈ 33 × 15 s 상한)였다.
pub struct SpawnOnceLatch {
    failed: std::sync::atomic::AtomicBool,
}

impl SpawnOnceLatch {
    pub const fn new() -> Self {
        Self {
            failed: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// 초기화 클로저에 **들어가면서** 부른다. 이미 한 번 실패했으면 여기서 죽는다 —
    /// 두 번째 프로세스를 띄우기 **전에** 막는 것이 이 함수의 전부다.
    ///
    /// 첫 실패의 panic 메시지에 stderr tail 과 실패 판정이 붙어 있으므로, 이 메시지는
    /// 원인을 다시 설명하지 않고 **어디를 보라고만** 말한다.
    pub fn entering(&self, what: &str) {
        assert!(
            !self.failed.swap(true, std::sync::atomic::Ordering::SeqCst),
            "{what} 의 첫 spawn 이 이미 실패했다 — 재시도하지 않는다. \
             원인과 stderr tail 은 이 binary 의 **첫 번째** 실패 메시지에 있다"
        );
    }

    /// spawn 이 성공했을 때 부른다. 이 호출이 없으면 래치가 내려간 채로 남아,
    /// 성공한 인스턴스를 쓰는 다음 호출이 잘못 막힌다.
    pub fn succeeded(&self) {
        self.failed
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Default for SpawnOnceLatch {
    fn default() -> Self {
        Self::new()
    }
}

/// 자식 stderr 의 **꼬리 N 줄**과 **마지막 줄의 시각**을 배경 스레드로 모은다.
///
/// 배경 스레드인 이유는 OS 파이프 역압이다 — 안 읽으면 자식이 stderr 쓰기에서 막힌다
/// (Linux 64 KB · macOS 16 KB). 이 두 값은 이 모듈의 판정 함수들이 그대로 소비한다
/// ([`spawn_timeout_message`] · [`stderr_silence_verdict`] · [`early_exit_message`]).
///
/// **왜 여기 있나.** 판정(소비자)은 이미 이 모듈에 모여 있었는데 포착(생산자)만 세 하네스에
/// 흩어져 있었고, 셋이 바이트 단위로 같았다. 같은 것이 셋이면 규칙도 셋이라, 하나를
/// 고쳐도 나머지 둘이 남는다.
///
/// ★ **[`Self::last_line_age`] 가 메서드인 것이 이 타입의 요점이다.** 예전에는 호출부가
/// `stderr_last_at.lock().unwrap().map(|t| t.elapsed())` 를 **`panic!` 의 인자 안**에서
/// 평가했고, 그러면 가드가 그 statement 끝까지 살아 있어 되감기 중에 Drop 되며 뮤텍스를
/// 오염시킨다. 그 뒤로는 배경 스레드가 다음 줄에서 죽어 **그 바이너리의 이후 실패가
/// stderr 꼬리를 잃는다** — 실패 수는 그대로인데 진단만 사라지는 형태다.
/// 실측(rustc, edition 2021·2024 동일): 인자 안이면 오염 `true`, 값을 먼저 꺼내
/// statement 밖에서 가드를 떨어뜨리면 `false`. 메서드로 감싸면 가드가 메서드 안에서
/// 떨어지므로 **오염될 자리가 애초에 생기지 않는다.**
pub struct StderrCapture {
    ring: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<String>>>,
    last_at: std::sync::Arc<std::sync::Mutex<Option<std::time::Instant>>>,
    drain: Option<std::thread::JoinHandle<()>>,
    tail_lines: usize,
}

/// 링이 붙드는 줄 수. 셋이 같은 값을 쓰고 있었으므로 정의를 하나로 모은다.
/// 꼬리로 **보여줄** 줄 수(`tail_lines`)는 이것과 별개이고 하네스마다 다르다.
const STDERR_RING_CAPACITY: usize = 256;

impl StderrCapture {
    /// `child.stderr.take()` 를 그대로 넘긴다. `None` 이면 포착 없이 빈 채로 산다 —
    /// 자식이 stderr 를 안 준 경우에도 실패 경로가 그대로 돌아야 한다.
    pub fn start(stderr: Option<std::process::ChildStderr>, tail_lines: usize) -> Self {
        let ring = std::sync::Arc::new(std::sync::Mutex::new(
            std::collections::VecDeque::with_capacity(STDERR_RING_CAPACITY),
        ));
        let last_at = std::sync::Arc::new(std::sync::Mutex::new(None));
        let drain = stderr.map(|stderr| {
            let ring = std::sync::Arc::clone(&ring);
            let last_at = std::sync::Arc::clone(&last_at);
            std::thread::spawn(move || {
                use std::io::BufRead as _;
                let reader = std::io::BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    *lock(&last_at) = Some(std::time::Instant::now());
                    let mut ring = lock(&ring);
                    if ring.len() == STDERR_RING_CAPACITY {
                        ring.pop_front();
                    }
                    ring.push_back(line);
                }
            })
        });
        Self {
            ring,
            last_at,
            drain,
            tail_lines,
        }
    }

    /// 꼬리 N 줄을 개행으로 이어 붙인 것. 진단 문구가 그대로 싣는다.
    pub fn tail(&self) -> String {
        let ring = lock(&self.ring);
        let start = ring.len().saturating_sub(self.tail_lines);
        ring.iter()
            .skip(start)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 이 포착이 보여주는 꼬리 줄 수 — 진단 문구가 같은 수를 함께 찍는다.
    pub fn tail_lines(&self) -> usize {
        self.tail_lines
    }

    /// 마지막 줄이 온 뒤 흐른 시간. [`stderr_silence_verdict`] 가 그대로 받는다.
    /// ★ 값을 돌려주고 가드는 여기서 떨어진다 — 타입 doc 의 이유 참조.
    pub fn last_line_age(&self) -> Option<std::time::Duration> {
        let at = *lock(&self.last_at);
        at.map(|t| t.elapsed())
    }

    /// 꼬리에서 술어에 맞는 첫 줄. 특정 실패 시그니처(bind 실패 등)를 집을 때 쓴다.
    pub fn find(&self, pred: impl Fn(&str) -> bool) -> Option<String> {
        let ring = lock(&self.ring);
        ring.iter().find(|line| pred(line)).cloned()
    }

    /// 배경 스레드를 거둔다. **자식이 끝난 뒤** 하네스의 `Drop` 이 부른다.
    ///
    /// ★ **"자식이 끝난 뒤" 는 주의사항이 아니라 계약이다 — 양쪽으로 위험하다.**
    /// 배출 스레드는 `reader.lines()` 가 `None` 을 줄 때 끝나고, 파이프의 EOF 는 자식이
    /// stderr 를 닫을 때(대개 종료할 때) 온다. 실측(3 초 사는 자식, 0.3 초 뒤 호출):
    ///
    /// | 부르는 시점 | 결과 |
    /// |---|---|
    /// | 자식을 거둔 뒤 (계약대로) | **3.84 µs** 만에 반환 |
    /// | 자식이 살아 있는데 | **2.70 s 막힌다** — 남은 수명만큼 |
    /// | 안 부르고 `drop` | 3.17 µs 반환, **배경 스레드는 남는다** |
    ///
    /// 앞쪽이 더 나쁘다. 예산이 있는 하네스에서 이르게 부르면 **조용히 멈춰 있고**
    /// 실패 문구는 자식이나 부팅 단계를 지목한다 — 멈춘 자리를 안 가리킨다.
    ///
    /// ★★ **이 순서를 지키는 자동 채널은 없다.** 위 표는 손으로 잰 값이고, 어떤 시험도
    /// 하네스가 `kill`/`wait` 뒤에 `join()` 을 부르는지 확인하지 않는다. 순서를 어겨도
    /// 컴파일되고, 어긴 결과는 **빨강이 아니라 지연**이라 초록 회차에서는 안 보이고
    /// 빨간 회차에서는 다른 자리를 지목한다. 그 사실을 적어 두는 이유는, 안 적으면
    /// 이 타입을 쓰는 초록이 순서까지 지켰다는 뜻으로 읽히기 때문이다.
    ///
    /// 채널을 만든다면 자리는 여기가 아니라 **하네스 쪽**이다 — 자식을 거뒀다는 사실을
    /// 타입이 알 방법이 없다(`Child` 를 안 들고 있다). 들게 만들면 이 타입이 프로세스
    /// 수명까지 소유하게 되어, 지금 세 하네스가 각자 다르게 하는 정리 순서를 하나로
    /// 강제한다. 그 결정은 이동을 하는 소유자가 내린다.
    ///
    /// ★ **그래서 `Drop` 으로 자동화하면 안 된다.** `Drop` 에서 무조건 join 하면 위
    /// 두 번째 줄이 **기본 동작**이 된다. 뒤쪽(스레드가 남는 것)은 프로세스 전역이라
    /// 공짜가 아니지만, 테스트 바이너리의 수명 안에서 자식이 죽으면 함께 끝난다.
    /// 두 위험의 크기가 달라서 수동으로 남긴다.
    pub fn join(&mut self) {
        if let Some(handle) = self.drain.take() {
            // 이유: 배출 스레드의 패닉은 이 자리에서 할 수 있는 일이 없고, 정리 경로라
            // 되던지면 다른 정리(포트 파일·격리 홈 삭제)가 안 돈다.
            let _ = handle.join();
        }
    }
}

/// 오염된 락에서 복구한다. **에러를 무시하는 것이 아니다.**
///
/// 보호 대상은 링 버퍼와 `Option<Instant>` 한 칸뿐이라 패닉이 그 둘의 불변식을 깨지
/// 않는다. 반대로 오염을 남기면 **진단 수집기 자신이 죽어** 이후 실패의 stderr 꼬리가
/// 통째로 사라진다 — 고치는 쪽이 정보가 는다.
fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// stderr 시그니처로 가릴 수 있는 것. **두 갈래의 확신 수준이 다르다.**
enum BootBlocker {
    /// 디스플레이 서버가 아예 없다 — winit 이 즉시 죽는다. 이 시그니처는 부팅에
    /// 성공한 인스턴스의 stderr 에는 **나오지 않으므로**(실측) 단독으로 원인이 된다.
    NoDisplay(&'static str),
    /// GPU 드라이버가 가속 경로를 못 잡고 폴백했다. **원인 판정이 아니다** —
    /// 아래 [`GPU_FALLBACK_MARKERS`] 의 주석 참조.
    GpuFallback(&'static str),
}

/// GPU 드라이버가 **가속 경로를 포기하고 폴백할 때** 스택이 남기는 문자열들.
///
/// 이름이 `..._FALLBACK_...` 인 것이 핵심이다. 이 줄들은 부팅 실패의 증거가 아니다 —
/// mesa/turnip 이 `/dev/dri/renderD128` 을 못 열고 소프트웨어 경로로 내려가는 **정상
/// 절차**의 흔적이고, 그렇게 내려간 뒤 부팅은 대개 성공한다. 실측(2026-09-04, 이
/// 개발 머신): port file 을 정상적으로 쓴 인스턴스의 stderr 에 여섯 개가 **전부** 있었다.
///
/// 그래서 이 마커는 "GPU 때문이다" 를 단정하는 데 쓸 수 없고, 판정문도 단정하지
/// 않는다([`boot_blocker_verdict`]). 정상 부팅에 안 나오는 GPU 시그니처를 대신 쓰고
/// 싶었으나, 이 머신에서는 GPU 초기화가 **항상** 소프트웨어 폴백으로 성공해서 그런
/// 로그를 채집할 수 없었다. 그런 시그니처를 실제로 관측하면 이 목록을 그것으로
/// 바꾸고 판정문의 단정을 되살릴 수 있다.
const GPU_FALLBACK_MARKERS: &[&str] = &[
    "renderD128", // DRM 렌더 노드 — 열지 못했다(점유 중이거나 접근 불가)
    "VK_ERROR_",  // Vulkan 초기화 실패 전반
    "DRI3",       // X 서버가 DRI3 를 못 주는 경우(가속 경로 상실)
    "libEGL",     // EGL 경고 — 위 둘과 함께 나오는 것이 보통
    "tu_knl",     // mesa/turnip 커널 인터페이스 오류
    "failed to open device",
];

/// 디스플레이 서버 부재를 가리키는 문자열들. 실측으로 정상 부팅 stderr 에는 없고
/// `DISPLAY`/`WAYLAND_*` 를 모두 지운 부팅에서만 나온다.
const NO_DISPLAY_MARKERS: &[&str] = &[
    "neither WAYLAND_DISPLAY nor WAYLAND_SOCKET nor DISPLAY is set",
    "cannot open display",
];

fn detect_blocker(stderr_tail: &str) -> Option<BootBlocker> {
    if let Some(m) = NO_DISPLAY_MARKERS
        .iter()
        .find(|m| stderr_tail.contains(**m))
    {
        return Some(BootBlocker::NoDisplay(m));
    }
    GPU_FALLBACK_MARKERS
        .iter()
        .find(|m| stderr_tail.contains(**m))
        .map(|m| BootBlocker::GpuFallback(m))
}

/// stderr tail 에서 실패 원인의 단서를 찾아 한 줄로 만든다.
///
/// 이 줄이 없으면 "느린 건지 환경이 막힌 건지" 를 매번 사람이 stderr 을 읽어 판정해야
/// 한다. 다만 **단서의 강도가 다르면 문장의 강도도 달라야 한다** — 확신에 찬 오답은
/// 조용한 타임아웃보다 나쁘다. 디스플레이 부재는 단정하고, GPU 폴백은 단정하지 않는다.
pub fn boot_blocker_verdict(stderr_tail: &str) -> Option<String> {
    match detect_blocker(stderr_tail)? {
        BootBlocker::NoDisplay(marker) => Some(format!(
            "디스플레이 서버가 없다 — 코드 인과가 아니다(시그니처: `{marker}`). 이 하네스는 실제 \
             GUI 를 띄우므로 `xvfb-run -a` 같은 디스플레이 위에서 돌려야 한다."
        )),
        BootBlocker::GpuFallback(marker) => Some(format!(
            "GPU 가속 경로 폴백 흔적이 있다(시그니처: `{marker}`) — **이것만으로는 원인 판정이 \
             되지 않는다.** 같은 줄이 부팅에 성공한 인스턴스의 stderr 에도 그대로 나온다. \
             먼저 다른 워크트리·인스턴스가 같은 GPU 디바이스를 쓰고 있는지 확인하고, 그것이 \
             아니면 코드 쪽(부팅 지연·plugin·셸 설정)을 그대로 본다."
        )),
    }
}

fn verdict_or_default(tail: &str, fallback: &str) -> String {
    if let Some(verdict) = boot_blocker_verdict(tail) {
        return verdict;
    }
    // ★ **빈 tail 은 "시그니처가 없다" 와 다른 세계다.** 폴백 문구는 둘 다
    // ("stderr 에 내용이 있었는데 안 걸렸다" 와 "stderr 이 아예 없었다") 에 쓰이는데,
    // 두 세계의 처방이 반대다 — 앞은 "부팅 지연·설정을 본다"(들어와서 느리다), 뒤는
    // "부팅에 들어가지도 못한 쪽을 본다". 한 메시지에 둘이 같이 실리면 서로를 지운다.
    //
    // 이 모듈의 존재 이유가 느린 것과 멈춘 것을 가르는 것인데, 그 아래 침묵 판정이
    // 가른 것을 이 줄이 도로 흐리고 있었다. 그래서 여기서는 **주장하지 않는다.**
    if tail.trim().is_empty() {
        return "stderr 이 비어 있다 — 시그니처가 없는 것이 아니라 볼 것이 없다.".to_string();
    }
    fallback.to_string()
}

/// 자식이 이미 죽었을 때의 panic 메시지. 상한을 다 기다릴 이유가 없는 경우다 —
/// 부팅 실패는 대부분 즉사(디스플레이 없음·설정 오류)라, 이 경로가 실제 대기 시간을
/// 수십 초에서 1 초 미만으로 줄인다.
pub fn early_exit_message(status: &str, tail_lines: usize, tail: &str) -> String {
    let verdict = verdict_or_default(tail, "stderr 의 마지막 오류를 그대로 읽는다.");
    format!(
        "tasty 프로세스가 부팅 중 종료했다 ({status}) — 상한을 기다리지 않고 즉시 실패시킨다.\n{verdict}\n--- stderr (last {tail_lines} lines) ---\n{tail}"
    )
}

/// spawn timeout panic 메시지. 단계 이름·상한·판정·stderr tail 을 한 형식으로 묶어
/// 두 하네스가 같은 모양으로 실패하게 한다.
/// **느린 것과 멈춘 것을 가른다.** 상한을 넘긴 부팅은 두 사건일 수 있다: 자식이 마감
/// 직전까지 진행 중이었거나(예산 부족 — 상한이 얇다), 한참 전부터 아무 말도 없었거나
/// (멈춤 — 상한을 올려도 그대로다). 종전 문구는 둘 다 `failed to start within 40s` 였다.
///
/// 가르는 값은 **마지막 stderr 이후 경과**다. 문턱은 상한의 절반 — 그 정도 침묵이면
/// 남은 예산을 더 줘도 같은 자리에 서 있을 것이라는 뜻이다. 순수 함수라 아래 단위
/// 테스트가 세 방향을 다 찌른다.
pub fn stderr_silence_verdict(last_line_age: Option<Duration>, limit: Duration) -> String {
    match last_line_age {
        None => "자식이 stderr 에 한 줄도 내지 않았다 — 부팅에 들어가지도 못한 쪽을 먼저 본다."
            .to_string(),
        Some(age) if age * 2 >= limit => format!(
            "마지막 stderr 이후 {age:?} 조용했다 — 느린 것이 아니라 멈춘 쪽이다. \
             상한 인상은 이 사건의 처방이 아니다."
        ),
        Some(age) => format!(
            "마지막 stderr 이 {age:?} 전이다 — 마감 직전까지 진행 중이었다. \
             예산 부족 쪽이라, 무엇이 그 시간을 쓰는지를 재라."
        ),
    }
}

pub fn spawn_timeout_message(
    stage: &str,
    limit: Duration,
    tail_lines: usize,
    tail: &str,
    last_line_age: Option<Duration>,
) -> String {
    let verdict = verdict_or_default(
        tail,
        "부팅 차단 시그니처는 없다 — 부팅 지연이나 설정 경로를 본다.",
    );
    let silence = stderr_silence_verdict(last_line_age, limit);
    format!(
        "{stage} within {limit:?}.\n{verdict}\n{silence}\n\
         --- stderr (last {tail_lines} lines) ---\n{tail}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_binary_is_the_one_cargo_built_for_this_test() {
        let picked = resolve_instance_bin(None, "/built/by/cargo");
        assert_eq!(picked, std::ffi::OsString::from("/built/by/cargo"));
    }

    #[test]
    fn an_override_path_replaces_the_cargo_built_binary() {
        let picked = resolve_instance_bin(
            Some(std::ffi::OsStr::new("/prebuilt/headless/tasty")),
            "/built/by/cargo",
        );
        assert_eq!(picked, std::ffi::OsString::from("/prebuilt/headless/tasty"));
    }

    /// **조합 의존 단언을 가진 스위트는 override 를 안 받는다** — 이 설계의 안전장치
    /// 전부가 이 한 줄이다. 무너지면 데몬만 조합이 바뀌어 `..._answers_in_both_combos`
    /// 계열의 단언이 구조적으로 뒤집힌다(함정 4).
    ///
    /// **양방향으로 본다** — 무시하는 쪽만 보면 "전부 무시" 도 통과한다.
    #[test]
    fn only_the_combo_dependent_suites_ignore_the_override() {
        let given = || Some(std::ffi::OsString::from("/some/headless/tasty"));

        assert_eq!(
            effective_override(DaemonKind::SameCombo, given()),
            None,
            "자기 조합의 데몬이 필요한 스위트는 override 를 받으면 안 된다"
        );
        assert_eq!(
            effective_override(DaemonKind::HeadlessOk, given()),
            given(),
            "IPC 만 쓰는 스위트는 override 를 그대로 받아야 한다 — 안 받으면 이 설계가 \
             아무것도 안 하는 것과 같다"
        );
    }

    /// 낡은 override 바이너리를 잡는가. **이 판정이 죽으면 아무 소리도 안 난다** —
    /// 낡은 데몬은 정상 부팅해 정상 응답하고 스위트는 옛 코드에 대해 판정한다.
    ///
    /// 양방향: 새 소스가 있으면 잡고, 없으면 안 잡는다. 그리고 `.rs` 가 아닌 새 파일은
    /// 데몬 동작을 안 바꾸므로 잡지 않는다 — 그것까지 잡으면 문서만 고쳐도 빨개진다.
    #[test]
    fn a_stale_override_binary_is_detected_and_a_fresh_one_is_not() {
        let dir = std::env::temp_dir().join(format!(
            "tasty-stale-probe-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let src = dir.join("src");
        std::fs::create_dir_all(&src).expect("탐침 디렉토리를 만들 수 있어야 한다");
        let bin = dir.join("tasty");
        std::fs::write(&bin, b"bin").expect("가짜 바이너리를 쓸 수 있어야 한다");

        assert_eq!(
            source_newer_than(&bin, vec![src.clone()]),
            None,
            "소스가 하나도 없으면 낡지 않았다"
        );

        // 바이너리보다 확실히 새것이 되게 한다 — 파일시스템 mtime 해상도가 거칠 수 있다.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(src.join("notes.md"), b"x").expect("문서를 쓸 수 있어야 한다");
        assert_eq!(
            source_newer_than(&bin, vec![src.clone()]),
            None,
            "`.rs` 가 아닌 파일은 데몬 동작을 안 바꾼다 — 이것까지 잡으면 문서만 고쳐도 \
             빨개진다"
        );

        std::fs::write(src.join("app.rs"), b"fn main() {}").expect("소스를 쓸 수 있어야 한다");
        assert_eq!(
            source_newer_than(&bin, vec![src.clone()]).as_deref(),
            Some(src.join("app.rs").as_path()),
            "바이너리보다 새로운 `.rs` 가 있으면 그 경로를 대야 한다"
        );

        // 탐침 디렉토리는 판정에 안 쓰이므로 정리 실패를 무시한다 — 남아도 temp 이고,
        // 여기서 실패를 올리면 판정과 무관한 이유로 빨개진다.
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_override_means_the_default_not_an_empty_path() {
        // 셸에서 `TASTY_E2E_BIN=` 로 비우는 것은 "기본으로 되돌린다" 로 읽힌다.
        // 빈 경로를 그대로 spawn 하면 원인을 알 수 없는 실패가 된다.
        let picked = resolve_instance_bin(Some(std::ffi::OsStr::new("")), "/built/by/cargo");
        assert_eq!(picked, std::ffi::OsString::from("/built/by/cargo"));
    }

    /// 2026-09-04 실측 로그 — **부팅에 성공한** 인스턴스의 stderr 이다(port file 작성
    /// 확인, port=43499). 처음에는 이것을 "워크트리 4 곳이 GPU 를 경합한 증거" 로 읽고
    /// 판정문이 "코드 인과가 아니다" 를 단정했는데, 같은 세 줄이 아무 경합 없이 단독으로
    /// 띄운 정상 부팅에도 한 글자 다르지 않게 나온다. turnip 이 `/dev/dri/renderD128` 을
    /// 못 열고 소프트웨어로 내려가는 드라이버 폴백 상용구다.
    const SUCCESSFUL_BOOT_GPU_FALLBACK_TAIL: &str = "\
libEGL warning: DRI3 error: Could not get DRI3 device
libEGL warning: Ensure your X server supports DRI3 to get accelerated rendering
TU: error: ../src/freedreno/vulkan/tu_knl.cc:387: failed to open device /dev/dri/renderD128 (VK_ERROR_INCOMPATIBLE_DRIVER)";

    /// 이 tail 로는 **코드를 무죄로 만들 수 없다.** 정상 부팅에도 그대로 나오는 줄이라,
    /// 이걸로 "코드 인과가 아니다" 를 말하면 어떤 원인의 타임아웃이든 전부 GPU 탓이 된다.
    #[test]
    fn a_gpu_fallback_tail_does_not_rule_out_code() {
        let verdict = boot_blocker_verdict(SUCCESSFUL_BOOT_GPU_FALLBACK_TAIL)
            .expect("단서는 실어야 한다 — 다만 단정하지 않는다");
        assert!(
            !verdict.contains("코드 인과가 아니다"),
            "정상 부팅에도 나오는 시그니처로 코드를 무죄 판정하면 안 된다: {verdict}"
        );
        assert!(
            verdict.contains("원인 판정이 되지 않는다"),
            "단정하지 않는다는 사실을 문장에 실어야 한다: {verdict}"
        );
        assert!(
            verdict.contains("다른 워크트리"),
            "먼저 확인할 것을 알려야 한다: {verdict}"
        );
    }

    /// 디스플레이 부재는 반대다 — 정상 부팅 stderr 에는 나오지 않으므로(실측) 단정한다.
    /// 두 갈래의 확신 수준이 실제로 다르다는 것을 여기서 고정한다.
    #[test]
    fn the_two_verdicts_do_not_carry_the_same_certainty() {
        let display = boot_blocker_verdict(
            "Error: neither WAYLAND_DISPLAY nor WAYLAND_SOCKET nor DISPLAY is set.",
        )
        .expect("디스플레이 부재는 판정 대상이다");
        let gpu = boot_blocker_verdict(SUCCESSFUL_BOOT_GPU_FALLBACK_TAIL).expect("단서는 실린다");
        assert!(display.contains("코드 인과가 아니다"), "{display}");
        assert!(!gpu.contains("코드 인과가 아니다"), "{gpu}");
    }

    #[test]
    fn missing_display_is_reported_as_display_not_gpu() {
        let tail = "Error: os error at winit/src/platform_impl/linux/mod.rs:765: \
                    neither WAYLAND_DISPLAY nor WAYLAND_SOCKET nor DISPLAY is set.";
        let verdict = boot_blocker_verdict(tail).expect("디스플레이 부재도 판정 대상이다");
        assert!(verdict.contains("디스플레이 서버가 없다"), "{verdict}");
        assert!(
            verdict.contains("xvfb-run"),
            "다음 사람이 바로 조치할 수 있게 방법을 실어야 한다: {verdict}"
        );
    }

    #[test]
    fn an_ordinary_slow_boot_gets_no_false_verdict() {
        let tail = "INFO tasty: plugin discovery finished\nINFO tasty: theme loaded";
        assert!(boot_blocker_verdict(tail).is_none());
        let msg = spawn_timeout_message(
            "tasty failed to start",
            SPAWN_PORT_TIMEOUT,
            30,
            tail,
            Some(Duration::from_millis(200)),
        );
        assert!(msg.contains("부팅 차단 시그니처는 없다"), "{msg}");
    }

    /// ★ 빈 stderr 에 대고 "시그니처가 없다" 를 말하면, 바로 아래 침묵 판정("한 줄도
    /// 내지 않았다 — 부팅에 들어가지도 못한 쪽을 먼저 본다")과 **반대 방향을 가리킨다.**
    /// 한 메시지가 두 처방을 순서 없이 싣는 것이 이 모듈이 없애려는 실패 형태다.
    #[test]
    fn an_empty_tail_is_not_reported_as_an_absent_signature() {
        let empty =
            spawn_timeout_message("tasty failed to start", SPAWN_PORT_TIMEOUT, 30, "", None);
        assert!(
            !empty.contains("부팅 차단 시그니처는 없다"),
            "stderr 이 비었는데 시그니처 부재를 주장한다 — 아래 침묵 판정과 반대를 가리킨다: {empty}"
        );
        assert!(empty.contains("볼 것이 없다"), "{empty}");
        // 침묵 판정은 제 자리를 지킨다 — 이 줄이 그 세계를 소유한다.
        assert!(empty.contains("한 줄도 내지 않았다"), "{empty}");

        // ★ 반대 방향 — 내용이 **있는데** 안 걸리는 tail 은 여전히 시그니처 부재를 말해야
        // 한다. 이게 없으면 위 초록은 "그 문장을 통째로 지웠다" 로도 설명된다.
        let noisy = spawn_timeout_message(
            "tasty failed to start",
            SPAWN_PORT_TIMEOUT,
            30,
            "INFO tasty: plugin discovery finished",
            Some(Duration::from_millis(200)),
        );
        assert!(
            noisy.contains("부팅 차단 시그니처는 없다"),
            "내용이 있는 tail 에서는 시그니처 부재가 여전히 정보다: {noisy}"
        );
        assert!(!noisy.contains("볼 것이 없다"), "{noisy}");
    }

    /// ★★ 두 마커 계열이 **함께** 나오는 것이 예외가 아니라 통상이다 — 디스플레이가 없는
    /// 부팅도 `libEGL`/`DRI3` 경고를 그대로 뱉는다. 그때 `detect_blocker` 의 검사 순서가
    /// 판정의 **확신 수준**을 정한다: 앞쪽은 단정하고("코드 인과가 아니다") 뒤쪽은
    /// 단정하지 않는다. 그런데 그 순서를 지키는 단정이 **하나도 없었다** — 기존 시험의
    /// 디스플레이 tail 에 GPU 마커가 없어서, 순서를 뒤집어도 전부 초록이었다.
    #[test]
    fn a_display_failure_that_also_logs_gpu_noise_is_still_a_display_failure() {
        let both = format!(
            "{SUCCESSFUL_BOOT_GPU_FALLBACK_TAIL}\nError: neither WAYLAND_DISPLAY nor \
             WAYLAND_SOCKET nor DISPLAY is set."
        );
        let verdict = boot_blocker_verdict(&both).expect("둘 다 있으면 판정 대상이다");
        assert!(
            verdict.contains("디스플레이 서버가 없다"),
            "GPU 잡음이 섞였다고 디스플레이 부재가 강등되면 안 된다: {verdict}"
        );
        assert!(
            !verdict.contains("원인 판정이 되지 않는다"),
            "확신 수준이 낮은 쪽 문장이 나왔다 — 검사 순서가 뒤집혔다: {verdict}"
        );

        // ★ 반대 방향 — GPU 마커만 있으면 디스플레이를 말하면 안 된다. 위 단정이
        // "무엇이든 디스플레이라고 한다" 로 통과하는 것을 막는다.
        let gpu_only =
            boot_blocker_verdict(SUCCESSFUL_BOOT_GPU_FALLBACK_TAIL).expect("단서는 실린다");
        assert!(!gpu_only.contains("디스플레이 서버가 없다"), "{gpu_only}");
    }

    /// 세 갈래가 서로 다른 문장을 내고, **남의 문장을 안 낸다**(양방향).
    #[test]
    fn the_silence_verdict_separates_stalled_from_merely_slow() {
        let limit = Duration::from_secs(40);

        let none = stderr_silence_verdict(None, limit);
        assert!(none.contains("한 줄도 내지 않았다"), "{none}");

        let stalled = stderr_silence_verdict(Some(Duration::from_secs(30)), limit);
        assert!(stalled.contains("멈춘 쪽"), "{stalled}");
        assert!(stalled.contains("처방이 아니다"), "{stalled}");

        let slow = stderr_silence_verdict(Some(Duration::from_millis(200)), limit);
        assert!(slow.contains("예산 부족"), "{slow}");
        assert!(
            !slow.contains("멈춘 쪽") && !slow.contains("한 줄도"),
            "갈래가 안 갈렸다: {slow}"
        );

        // 문턱은 상한의 절반이다 — 경계 양쪽을 함께 박는다.
        assert!(stderr_silence_verdict(Some(limit / 2), limit).contains("멈춘 쪽"));
        assert!(
            stderr_silence_verdict(Some(limit / 2 - Duration::from_millis(1)), limit)
                .contains("예산 부족")
        );
    }

    #[test]
    fn both_harnesses_share_one_bound_for_the_same_stage() {
        // 값 자체보다 "두 하네스가 같은 상수를 본다" 는 사실이 중요하다. 이 모듈이
        // 유일한 정의 자리이므로, 어느 한쪽이 자기 값을 되살리면 여기 상수가 안 쓰여
        // dead_code 로 드러난다. 상한 순서(S1 > S2)만 여기서 고정한다.
        assert!(SPAWN_PORT_TIMEOUT > SPAWN_SHELL_TIMEOUT);
    }

    /// 줄을 많이 뱉는 자식. 링 용량을 넘겨야 링이 도는지 볼 수 있다.
    fn child_that_prints_stderr_lines(n: usize) -> std::process::Child {
        #[cfg(windows)]
        let mut cmd = {
            let mut c = std::process::Command::new("cmd");
            c.arg("/C")
                .arg(format!("for /L %i in (1,1,{n}) do @echo line %i 1>&2"));
            c
        };
        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = std::process::Command::new("sh");
            c.arg("-c").arg(format!(
                "i=1; while [ $i -le {n} ]; do echo \"line $i\" 1>&2; i=$((i+1)); done"
            ));
            c
        };
        cmd.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("stderr 를 뱉는 자식을 못 띄웠다")
    }

    /// ★ 이 타입은 세 하네스가 그 위로 옮겨 탈 자리인데 **한 번도 안 돌았다.**
    /// 링이 실제로 돌고, 꼬리가 `tail_lines` 로 잘리고, 시각이 찍히는지를 잰다.
    #[test]
    fn the_capture_rings_at_capacity_and_shows_only_the_tail_it_promises() {
        let emitted = STDERR_RING_CAPACITY + 44;
        let mut child = child_that_prints_stderr_lines(emitted);
        let mut cap = StderrCapture::start(child.stderr.take(), 7);
        child.wait().expect("자식을 못 거뒀다");
        cap.join();

        let tail = cap.tail();
        let lines: Vec<&str> = tail.lines().collect();

        // ① 약속한 줄 수만 보여준다 — 링 용량(256)이 아니라 tail_lines(7).
        assert_eq!(
            lines.len(),
            cap.tail_lines(),
            "꼬리 줄 수가 약속과 다르다: {tail}"
        );

        // ② 링이 **돌았다** — 마지막 줄이 남고 첫 줄은 밀려났다. 이것이 없으면
        //    용량을 넘겼을 때 오래된 줄이 남는지 새 줄이 남는지 아무도 모른다.
        assert_eq!(
            lines.last().copied(),
            Some(format!("line {emitted}").as_str())
        );
        assert!(
            cap.find(|l| l == "line 1").is_none(),
            "용량을 {STDERR_RING_CAPACITY} 넘겨 {emitted} 줄을 넣었는데 첫 줄이 남아 있다"
        );
        // ③ 양성 대조 — `find` 가 늘 `None` 이라서 ②가 통과한 것이 아니다.
        assert!(cap.find(|l| l == format!("line {emitted}")).is_some());

        // ④ 시각이 찍힌다. 이 값이 없으면 침묵 판정이 통째로 무정보다.
        assert!(cap.last_line_age().is_some());
    }

    /// stderr 를 안 준 자식에서도 살아야 한다 — 실패 경로가 그대로 돌아야 하기 때문이다.
    #[test]
    fn a_capture_without_a_pipe_stays_empty_instead_of_dying() {
        let mut cap = StderrCapture::start(None, 9);
        assert_eq!(cap.tail(), "");
        assert_eq!(cap.last_line_age(), None);
        assert!(cap.find(|_| true).is_none());
        cap.join(); // 거둘 스레드가 없어도 막히지 않는다
    }

    /// ★★ [`StderrCapture::last_line_age`] 가 **메서드인 이유**를 여기서 잰다.
    ///
    /// 타입 doc 이 "가드를 `panic!` 인자 안에서 만들면 오염된다" 를 주장하는데, 그 주장은
    /// 지금까지 이 레포 **밖**에서만 측정됐다. 기전이 바뀌면(에디션·컴파일러) 주장만 남고
    /// 아무도 모른다. 그래서 두 형태를 여기서 **양방향으로** 고정한다.
    ///
    /// `StderrCapture` 자신으로는 못 잰다 — 내부 `lock()` 이 오염을 복구하므로 밖에서
    /// 관측되지 않는다. 그것이 이 시험이 지역 뮤텍스로 기전을 잡는 이유다.
    #[test]
    fn a_guard_born_inside_a_panic_argument_poisons_and_one_dropped_before_it_does_not() {
        // ① 인자 안에서 만든 가드 — statement 끝까지 살아 되감기 중에 Drop 된다.
        let inside = std::sync::Mutex::new(7u32);
        let hit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            panic!("나이 {:?}", *inside.lock().expect("첫 lock 은 성해야 한다"));
        }));
        assert!(hit.is_err(), "패닉이 안 났으면 아래 판정이 무정보다");
        assert!(
            inside.lock().is_err(),
            "인자 안의 가드가 오염을 안 만든다 — 그러면 last_line_age 를 메서드로 둔 근거가 사라진다"
        );

        // ② 값을 먼저 꺼내고 가드를 statement 밖에서 떨어뜨린다 = 메서드가 하는 일.
        let outside = std::sync::Mutex::new(7u32);
        let hit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let v = *outside.lock().expect("첫 lock 은 성해야 한다");
            panic!("나이 {v:?}");
        }));
        assert!(hit.is_err());
        assert!(
            outside.lock().is_ok(),
            "가드를 먼저 떨어뜨렸는데도 오염됐다 — 처방이 안 듣는다는 뜻이다"
        );
    }

    /// 래치가 **실제로 두 번째를 막는가.** 막는 것이 이 타입의 전부인데 안 재고 있었다.
    #[test]
    fn the_latch_blocks_the_second_spawn_and_a_success_releases_it() {
        let latch = SpawnOnceLatch::new();

        // ① 첫 진입은 통과한다.
        latch.entering("시험용 하네스");
        // ② 성공을 안 알리면 두 번째 진입에서 죽는다 — 프로세스를 띄우기 **전에** 막는다.
        let blocked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            latch.entering("시험용 하네스");
        }));
        assert!(blocked.is_err(), "래치가 두 번째 진입을 안 막았다");

        // ③ 양성 대조 — 래치가 **늘** 막는 것이 아니다. 성공을 알리면 다시 열린다.
        //    이것이 없으면 ②는 "두 번째는 무조건 죽는다" 로도 설명되고, 그러면 성공한
        //    인스턴스를 쓰는 다음 호출까지 잘못 막힌다.
        let opened = SpawnOnceLatch::new();
        opened.entering("시험용 하네스");
        opened.succeeded();
        opened.entering("시험용 하네스"); // 안 죽어야 한다
    }
}

/// 번들 plugin 을 **실제로 호출하는** 테스트 바이너리.
///
/// 여기 없는 스위트는 빈 번들 루트로 부팅한다 — host 는 spec 마다 원본을 못 찾아
/// 조용히 건너뛰고, 격리 홈으로 가는 복사가 통째로 사라진다. 실측(2026-09-06,
/// `attach_silent_disconnect`, 번들만 바꾼 대조): 홈 최대 1148 MB → 4 MB.
///
/// **판정은 성질로, 저장은 이름으로 한다.** 이 명부는 바이너리 이름으로 매칭하지만
/// 무엇을 넣을지는 이름 모양이 아니라 *그 스위트가 plugin 네임스페이스를 호출하거나
/// plugin 이 뒷받침하는 surface 타입을 여는가* 로 정했다. 이름으로 판정하면 샌다 —
/// `explorer` 는 plugin 처럼 생겼지만 호스트 view 이고, `webhook.*` 는 권한 등급에
/// `plugin` 이 붙어 있지만 핸들러는 host 다. 같은 층 구분을 다른 자리에서 이름 붙인
/// 예가 `crates/tasty-doc-guards/tests/ci_channel_claims_match_workflows.rs` 에 있다.
///
/// **극성이 opt-in 인 것도 의도다.** 빠뜨리면 그 스위트의 plugin 호출이
/// `-32601 Method not found` 로 실패해 **그 자리에서 빨개진다**. 반대 극성(기본
/// 스테이징 + 예외 명부)은 명부가 낡아도 초록이라 비용만 조용히 자란다 — 이 비용이
/// 오래 안 보였던 이유가 정확히 그것이다.
///
/// 새 스위트가 plugin 을 쓰기 시작하면 여기 추가한다.
pub const SUITES_THAT_CALL_BUNDLED_PLUGINS: &[&str] = &["e2e_tests", "soak_memory"];

/// 지금 도는 테스트 바이너리 이름 (`.../deps/e2e_tests-1a2b3c4d` → `e2e_tests`).
///
/// **바이너리 단위여야 한다.** 공유 인스턴스는 프로세스당 `OnceLock` 이라
/// (`tests/common` 의 `shared()`) 테스트 함수 안에서 opt-in 을 부르면 먼저 도는
/// 테스트가 초기화를 가져가 경합한다. 같은 사실이 "테스트 N 개 = 인스턴스 1 개" 라
/// 위 실측의 1148 MB 도 부팅 하나의 값이다.
pub fn current_suite_name() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let stem = exe.file_stem()?.to_str()?;
    // cargo 가 붙이는 `-<hex>` 만 벗긴다. 스위트 이름 자체에 `-` 는 안 쓴다.
    match stem.rsplit_once('-') {
        Some((name, hash))
            if !name.is_empty()
                && !hash.is_empty()
                && hash.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            Some(name.to_string())
        }
        _ => Some(stem.to_string()),
    }
}

/// 이 스위트가 번들 plugin 을 안 쓰면 빈 디렉터리를 번들 루트로 지정한다.
///
/// 제품의 `bundle_root()` 는 `TASTY_BUILTIN_PLUGINS_DIR` 를 **최우선**으로 보므로,
/// 이 한 줄이 workspace 스테이징 탐색과 격리 홈 복사를 **둘 다** 건너뛰게 한다.
/// 제품 코드는 건드리지 않는다 — 설치 경로에는 서명·업그레이드 판정이 얹혀 있다.
pub fn apply_bundle_opt_in(command: &mut std::process::Command) {
    let needs = current_suite_name()
        .is_some_and(|s| SUITES_THAT_CALL_BUNDLED_PLUGINS.contains(&s.as_str()));
    if needs {
        return;
    }
    // 만들기에 실패하면 아무것도 안 한다 — 없는 경로를 넘기면 제품이 그 분기를
    // 무시하고 진짜 번들을 찾으므로, 실패는 "변경 전" 동작으로 되돌아간다.
    // `create_dir_all` 은 멱등이라 동시 생성도 안전하고, 유니크화하면 내용이 같은
    // 빈 디렉터리만 완주 수만큼 늘 뿐이다. 격리가 사는 자리는 여기가 아니라 홈이다.
    //
    // 이유: **비어 있다는 것이 이 디렉터리 내용의 전부**라 아무도 쓰지 않고 아무도
    // 지우지 않는다 — 고정 이름이 위험한 근거(동시 완주가 서로의 파일을 truncate
    // 하거나 디렉터리를 지운다)가 성립할 대상이 없다. 공유가 의도다.
    let empty = std::env::temp_dir().join("tasty-test-empty-plugin-bundle");
    if std::fs::create_dir_all(&empty).is_ok() && empty.is_dir() {
        command.env("TASTY_BUILTIN_PLUGINS_DIR", &empty);
    }
}
