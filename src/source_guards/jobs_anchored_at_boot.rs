//! 번들 설치·상태 정리처럼 요청과 무관하게 필요한 작업이 각 빌드 조합의 초기화 함수에 있는지 확인한다.
//! 프로세스·러너 시작은 필요할 때까지 미뤄도 되지만 설치와 재시작 상태 정리는 초기화 때 필요하다.
//! 지연 경로에 재시도 호출이 남아 있어도 괜찮으며 그 경로에만 의존하지 않도록 한다.
//!
//! 등록된 함수 본문에서 주석을 지우고 함수 이름의 부분 문자열을 찾는다.
//! 실제 호출 문법·조건 분기·실행 횟수까지 검증하지는 않는다.

use std::collections::BTreeSet;

use super::{fn_body, repo_root, strip_comments};

struct Anchor {
    job: &'static str,
    call: &'static str,
    combo: &'static str,
    file: &'static str,
    func: &'static str,
}

/// 각 작업은 헤드리스와 GUI 두 조합에 모두 등록해야 한다.
const COMBOS: &[&str] = &["headless", "gui"];

const ANCHORS: &[Anchor] = &[
    Anchor {
        job: "번들 plugin 설치",
        call: "install_builtins_if_needed",
        combo: "headless",
        file: "src/boot.rs",
        func: "fn run_headless",
    },
    Anchor {
        job: "번들 plugin 설치",
        call: "install_builtins_if_needed",
        combo: "gui",
        file: "src/app/window_lifecycle.rs",
        func: "fn prepare_plugin_manager",
    },
    Anchor {
        // namespace 표를 전달하지 않으면 메서드 권한 조회가 플러그인 prefix를 인식하지 못한다.
        job: "namespace 소유 표 설치",
        call: "install_namespace_table",
        combo: "headless",
        file: "src/boot.rs",
        func: "fn run_headless",
    },
    Anchor {
        job: "namespace 소유 표 설치",
        call: "install_namespace_table",
        combo: "gui",
        file: "src/app/window_lifecycle.rs",
        func: "fn prepare_plugin_manager",
    },
    Anchor {
        // 엔진이 첫 셸을 만들기 전에 반영하지 않으면 복원 셸이 제품 세션 변수를 물려받고,
        // 복원된 작업 러너가 agent provider 자격을 판정하지 못한다.
        job: "공용 표 반영(상속 환경변수 제거 규칙·namespace 소유 상태)",
        call: "publish_shared_tables",
        combo: "headless",
        file: "src/boot.rs",
        func: "fn run_headless",
    },
    Anchor {
        job: "공용 표 반영(상속 환경변수 제거 규칙·namespace 소유 상태)",
        call: "publish_shared_tables",
        combo: "gui",
        file: "src/app/window_lifecycle.rs",
        func: "fn prepare_plugin_manager",
    },
    Anchor {
        job: "agent 재시작 정화·핸들 재적재",
        call: "purge_stale_agent_state_on_boot",
        combo: "headless",
        file: "src/boot.rs",
        func: "fn bootstrap_engine",
    },
    Anchor {
        job: "agent 재시작 정화·핸들 재적재",
        call: "purge_stale_agent_state_on_boot",
        combo: "gui",
        file: "src/app/boot_machine.rs",
        func: "fn finish_boot",
    },
];

fn calls(body: &str, call: &str) -> bool {
    strip_comments(body).contains(call)
}

/// 다른 구간이나 빈 본문을 잘못 읽는 경우를 찾기 위한 길이 하한.
const MIN_BODY_LEN: usize = 200;

#[test]
fn every_boot_anchored_job_runs_on_every_build_combination() {
    let mut missing: Vec<String> = Vec::new();
    for a in ANCHORS {
        let path = repo_root().join(a.file);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} 을 읽지 못했다: {e}", a.file))
            .replace("\r\n", "\n");
        let body = fn_body(&src, a.func).unwrap_or_else(|| {
            panic!(
                "{}에서 `{}` 본문을 읽지 못했다. 함수 이름과 ANCHORS를 확인한다.",
                a.file, a.func
            )
        });
        assert!(
            body.len() >= MIN_BODY_LEN,
            "{}의 `{}` 본문이 {}바이트로 하한 {MIN_BODY_LEN} 미만이다. 본문 추출을 확인한다.",
            a.file,
            a.func,
            body.len()
        );
        if !calls(&body, a.call) {
            missing.push(format!(
                "{} [{}]: {} 의 `{}` 가 {} 를 안 부른다",
                a.job, a.combo, a.file, a.func, a.call
            ));
        }
    }
    assert!(
        missing.is_empty(),
        "초기화 함수에서 필수 작업을 찾지 못한 조합이 있다. 요청이 와야만 설치·상태 정리가 수행되는 구조는 아닌지 확인한다(docs/dev-guide/plugin-development.md):\n  {}",
        missing.join("\n  ")
    );
}

/// 상속 환경변수 제거 규칙은 엔진이 첫 셸을 만들기 전에 반영해야 한다.
/// 주석을 지운 본문에서 두 이름의 첫 등장 위치만 비교하며 실제 실행 순서를 증명하지는 않는다.
#[test]
fn inherited_env_rules_are_published_before_the_engine_is_built() {
    const ORDER: &[(&str, &str, &str, &str)] = &[
        (
            "src/boot.rs",
            "fn run_headless",
            "publish_shared_tables",
            "bootstrap_engine(",
        ),
        (
            "src/app/window_lifecycle.rs",
            "fn ensure_engine_and_plugins",
            "prepare_plugin_manager(",
            "EngineSession::new_with_ids(",
        ),
        (
            "src/app/window_lifecycle.rs",
            "fn ensure_engine_and_plugins",
            "prepare_plugin_manager(",
            "build_core_state_first_boot(",
        ),
        (
            "src/app/window_lifecycle.rs",
            "fn build_engine_and_plugins",
            "prepare_plugin_manager(",
            "build_core_state_first_boot(",
        ),
    ];
    for (file, func, first, then) in ORDER {
        let src = std::fs::read_to_string(repo_root().join(file))
            .unwrap_or_else(|e| panic!("{file} 을 읽지 못했다: {e}"))
            .replace("\r\n", "\n");
        let body = strip_comments(
            &fn_body(&src, func)
                .unwrap_or_else(|| panic!("{file}에서 `{func}` 본문을 읽지 못했다")),
        );
        let a = body
            .find(first)
            .unwrap_or_else(|| panic!("{file} `{func}` 에 {first} 가 없다"));
        let b = body
            .find(then)
            .unwrap_or_else(|| panic!("{file} `{func}` 에 {then} 가 없다"));
        assert!(a < b, "{file} `{func}`: {first} 가 {then} 보다 뒤에 있다");
    }
}

#[test]
fn the_roster_covers_every_combination_for_every_job() {
    let jobs: BTreeSet<&str> = ANCHORS.iter().map(|a| a.job).collect();
    assert!(!jobs.is_empty(), "초기화 작업 명부가 비어 있다");
    for job in jobs {
        let covered: BTreeSet<&str> = ANCHORS
            .iter()
            .filter(|a| a.job == job)
            .map(|a| a.combo)
            .collect();
        for combo in COMBOS {
            assert!(
                covered.contains(combo),
                "명부의 `{job}` 에 {combo} 조합 항목이 없다 — 그 조합은 안 보고 있다"
            );
        }
    }
}

#[test]
fn a_job_that_lives_only_in_a_lazy_site_is_reported() {
    let boot = "\
fn run_headless(cli: Cli) -> Result<()> {
    let mut app = App::new_headless()?;
    let mut engine = bootstrap_engine(&mut app)?;
    tracing::info!(\"headless daemon ready\");
    loop { pump(&mut app, &mut engine); }
}
fn ensure_plugin_manager(app: &mut App) {
    install_builtins_if_needed(mgr);
    mgr.discover_and_start();
}
";
    let body = fn_body(boot, "fn run_headless").expect("본문을 잘라야 한다");
    assert!(
        !calls(&body, "install_builtins_if_needed"),
        "지연 자리에 있는 호출을 부팅 경로의 것으로 셌다 — 자르기가 함수 끝을 넘었다"
    );
    let lazy = fn_body(boot, "fn ensure_plugin_manager").expect("본문을 잘라야 한다");
    assert!(
        calls(&lazy, "install_builtins_if_needed"),
        "대조군: 지연 자리에는 실제로 있다"
    );
}

#[test]
fn a_mention_in_a_comment_is_not_a_call() {
    let src = "\
fn run_headless() {
    // 설치는 install_builtins_if_needed 가 한다 — 여기서는 안 부른다.
    boot();
}
";
    let body = fn_body(src, "fn run_headless").expect("본문을 잘라야 한다");
    assert!(
        !calls(&body, "install_builtins_if_needed"),
        "주석에 적힌 함수 이름을 호출 근거로 판단했다"
    );
}
