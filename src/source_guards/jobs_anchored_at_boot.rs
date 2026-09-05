//! **트리거와 무관하게 필요한 일**이 조합마다 부팅 경로에 걸려 있는지 못 박는다.
//!
//! ## 무엇이 실제로 났나
//!
//! 헤드리스에서 번들 plugin 설치(`install_builtins_if_needed`)의 유일한 채널이
//! `ensure_plugin_manager` 였고, 그 함수의 실질 트리거는 "호스트가 모르는 이름을
//! 처음 부를 때" 였다 — 즉 **오타 하나가 설치와 기동을 함께 시켰다.** namespace 소속
//! 판정을 매니페스트로 옮겨(ADR-0173) 그 트리거를 좁히자 기동만 좁아진 것이 아니라
//! **설치 경로가 통째로 사라졌고**, 갓 만든 홈의 데몬이 package 0 인 채로 남았다.
//!
//! ## 왜 컴파일도 스위트도 못 잡았나
//!
//! 호출 하나가 안 불릴 뿐이라 컴파일은 통과하고, 두 조합의 전체 유닛 스위트도
//! 통과했다. **테스트가 "갓 만든 `TASTY_HOME`" 이라는 초기 상태를 안 만들기**
//! 때문이다. 잡은 채널은 격리 홈으로 데몬을 띄워 물어본 실행 확인 하나였다.
//!
//! ## 명부의 입장 기준
//!
//! **필요성이 트리거와 무관한 일**만 들어온다 — 설치, 그리고 부팅마다 한 번 돌아야
//! 하는 상태 정화·로드다. 반대로 **기동(프로세스·스레드를 띄우는 일)은 지연이 옳고**
//! 여기 들어오지 않는다: 안 쓰면 안 떠야 하는 것이 기동의 정의다. 이 갈래는 agent
//! runner 가 이미 그렇게 갈라 두었다 — 재시작 정화는 부팅 1 회고(`결정 2`), 러너
//! 스레드는 수동 start 전까지 안 뜬다(`결정 1`).
//!
//! 지연 자리에 같은 호출이 **남아 있는 것**은 결함이 아니라 재시도다. 결함은 지연이
//! **유일한** 채널일 때 생긴다.
//!
//! ## 판정
//!
//! 일마다 조합별 부팅 함수를 지목하고, 그 본문에 호출이 있는지 본다. 주석은 먼저
//! 지운다 — 그 일을 *설명하는* 주석이 호출로 오인되면 이 가드는 문서를 잘 쓸수록
//! 나빠진다. 지목한 함수를 못 자르면 **통과가 아니라 실패**다(이름이 바뀌었는데
//! 조용히 초록이 되는 것이 이 부류의 원래 사고다).

use std::collections::BTreeSet;

use super::{fn_body, repo_root, strip_comments};

/// 부팅에 걸려 있어야 하는 일 하나의, 한 조합에서의 자리.
struct Anchor {
    /// 사람이 읽는 일 이름 — 실패 메시지에만 쓴다.
    job: &'static str,
    /// 본문에서 찾을 호출.
    call: &'static str,
    /// 빌드 조합.
    combo: &'static str,
    /// 부팅 경로 파일과 함수 시그니처.
    file: &'static str,
    func: &'static str,
}

/// 이 저장소가 지금 가진 빌드 조합. 명부는 **모든 조합**을 덮어야 한다 — 원래 사고가
/// "한 조합에만 있었다" 였으므로, 한쪽만 적힌 명부는 그 사고를 그대로 통과시킨다.
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
        func: "fn build_plugin_manager",
    },
    Anchor {
        // 소유 표를 **해소하는 crate 에 넘기는** 일. 이 설치가 없으면 `method_meta` 는
        // 어떤 plugin prefix 도 모르는 채로 남아 plugin namespace 메서드가 권한 검사에서
        // "모르는 메서드" 가 된다 — 그리고 그 실패는 조합마다 다르게 난다.
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
        func: "fn build_plugin_manager",
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

/// 본문이 그 일을 부르는가. 주석은 세지 않는다.
fn calls(body: &str, call: &str) -> bool {
    strip_comments(body).contains(call)
}

/// 본문을 자르지 못했거나 너무 짧으면 판정이 죽은 것이다 — 통과로 세면 안 된다.
const MIN_BODY_LEN: usize = 200;

/// 명부의 일이 조합마다 부팅 경로에서 실제로 불린다.
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
                "{} 에서 `{}` 본문을 못 잘랐다 — 대조군이 죽었다. 함수 이름이 \
                 바뀌었으면 ANCHORS 를 같이 고쳐라",
                a.file, a.func
            )
        });
        assert!(
            body.len() >= MIN_BODY_LEN,
            "{} 의 `{}` 본문이 {} 바이트뿐이다(하한 {MIN_BODY_LEN}) — 자르기가 \
             엉뚱한 곳을 물었을 가능성이 크다",
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
        "트리거와 무관하게 필요한 일이 부팅 경로에 안 걸린 조합이 있다. 그 조합에서 \
         이 일의 유일한 채널은 요청이 되고, 요청 형태가 좁아지는 순간 함께 사라진다 \
         (실제로 났다 — ADR-0173).\n  {}",
        missing.join("\n  ")
    );
}

/// 명부 자체가 한 조합을 빠뜨리지 않는다.
///
/// 원래 사고의 형태가 "gui 에는 있고 헤드리스에는 없다" 였다. 명부에 gui 만 적으면
/// 위 판정은 초록인 채로 같은 사고를 통과시키므로, 결손을 여기서 따로 운다.
#[test]
fn the_roster_covers_every_combination_for_every_job() {
    let jobs: BTreeSet<&str> = ANCHORS.iter().map(|a| a.job).collect();
    assert!(!jobs.is_empty(), "명부가 비었다 — 판정이 죽었다");
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

/// 일이 **지연 자리에만** 있으면 잡는가 — 이 가드가 겨냥한 회귀 그대로다.
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

/// 주석으로만 언급된 일은 호출로 세지 않는다.
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
        "주석 안의 이름을 호출로 셌다 — 문서를 잘 쓸수록 나빠지는 판정이다"
    );
}
