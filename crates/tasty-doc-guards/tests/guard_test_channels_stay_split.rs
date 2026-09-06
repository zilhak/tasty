//! **`ci-gates.md` 가 가드 테스트에 대해 하는 두 주장을 워크플로에서 다시 읽는다.**
//!
//! 그 문서는 `crates/tasty-doc-guards/tests/*` 를 두고 축이 둘이라고 적는다 — 컴파일
//! 채널은 있고(Windows 잡의 `--all-targets`), 실행 채널은 없다(실행하는 자동 잡은
//! `doc-guards.yml` 하나이고 ubuntu). 그 두 문장은 **워크플로가 정하는 사실**인데
//! 지키는 것이 없었다.
//!
//! 이 레포는 그 형태를 이미 안다 — 문서·주석이 "이건 저것과 같다" 고 말하는데 그 같음을
//! 지키는 것이 없으면 둘은 갈리고, **갈린 뒤에도 문서는 계속 같다고 말한다.** 그리고 그
//! 문서를 정확히 따른 사람이 없는 결함을 판다.
//!
//! # 왜 이 파일이 따로인가
//!
//! 자매 가드([`ci_channel_claims_match_workflows`])는 **다른 물음**에 답한다 — "그 테스트가
//! 자동 잡의 사정거리 안에 있는가", 즉 *집행된다*는 거짓 주장을 잡는다. 여기 주장은 그
//! 반대 방향이다("실행 채널이 **없다**"). 없다는 주장은 그 가드의 그물에 안 걸린다 —
//! 그 그물은 있다고 적은 자리를 잡기 때문이다. 물음이 다르면 그물도 다르다.
//!
//! # 잡지 못하는 것
//!
//! 러너 라벨을 글자로 본다. 자기호스티드 라벨이 바뀌면(`Windows` → 다른 이름) 이 판정은
//! **덜 잡는 쪽**으로 틀린다 — 그 방향의 오차는 하한 대조가 잡는다.

use std::path::PathBuf;

use tasty_doc_guards::floored_walk::{Descend, Floor, walk_with_floor};
use tasty_doc_guards::workflow_triggers::automatic_job_bodies;

/// 이 주장이 사는 자리. 빨개졌을 때 고칠 곳을 실패문이 지목한다.
const DOC: &str = "docs/dev-guide/ci-gates.md";

/// 워크플로 순회의 하한. 이 아래로 모이면 순회가 죽은 것으로 본다.
const WORKFLOW_FLOOR: Floor = Floor {
    min: 8,
    measured: 11,
    measured_on: "2026-09-07",
    why_this_gap: "이 모수는 `.github/workflows/*.yml` 의 수다. 워크플로는 통합할 때 \
                   가끔 합쳐지므로 몇 개는 줄 수 있지만, 8 아래로 떨어지는 것은 파일이 \
                   줄어든 게 아니라 순회 루트가 어긋난 것이다.",
};

/// 잡 하한. [`WORKFLOW_FLOOR`] 와 **모수가 다르다** — 저쪽은 `.yml` 파일 수이고 여기는
/// 그 파일들에서 뽑아낸 **자동 잡의 수**다. 파일은 다 읽혔는데 잡 헤더 판독이 깨지면
/// 저쪽은 통과하고 여기가 잡는다(그 판독은 2 칸 들여쓰기 관례에 매달려 있다 —
/// `automatic_job_bodies` 의 doc 참조). 두 하한이 같은 8 인 것은 우연이다.
///
/// ☆ **이 하한도 읽기의 죽음까지는 못 막는다.** 파일이 **전부** 안 읽히면 잡이 0 이라
/// 여기서 걸리지만, 열한 중 셋만 못 읽히면 잡 수가 8 위에 남아 그대로 통과한다 —
/// 그 부분적 실명을 막는 것은 하한이 아니라 `automatic_jobs` 의 읽기 실패 패닉이다.
const JOB_FLOOR: Floor = Floor {
    min: 8,
    measured: 20,
    measured_on: "2026-09-07",
    why_this_gap: "잡은 워크플로마다 하나에서 넷까지라 파일 수보다 흔들린다. 그래도 \
                   8 아래는 파일이 줄어든 것이 아니라 잡 헤더 판독이 깨진 것이다.",
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("레포 루트")
        .to_path_buf()
}

/// 자동 회차에 도는 잡의 본문 전부.
fn automatic_jobs() -> Vec<(String, String)> {
    let root = repo_root();
    let dir = root.join(".github/workflows");
    // 공용 순회를 쓴다. 직접 `read_dir` 하면 디렉토리가 비거나 못 읽혔을 때 "위반 0" 이
    // 나오고, 그것은 위반이 없다는 뜻이 아니라 아무것도 안 봤다는 뜻이다.
    let walked = walk_with_floor(&dir, &dir, &WORKFLOW_FLOOR, Descend::Everything, &|w| {
        w.rel.ends_with(".yml")
    })
    .unwrap_or_else(|why| panic!("{why}"));

    let mut out = Vec::new();
    for w in walked {
        // ★ **읽기 실패를 넘기지 않는다.** 하한은 **순회**의 죽음을 막지 **읽기**의 죽음을
        // 안 막는다 — `continue` 로 넘기면 파일 셋이 안 읽혀도 순회 하한은 그대로 통과하고
        // 본문만 빈다. 그러면 아래 단정들은 "그 잡이 없다" 가 아니라 "그 잡을 안 봤다" 를
        // 근거로 판정한다. 여기 오는 경로는 순회가 방금 찾아낸 것이라 못 읽는 것은 평범한
        // 조건이 아니다.
        let text = std::fs::read_to_string(&w.path).unwrap_or_else(|e| {
            panic!(
                "워크플로 {} 를 못 읽었다: {e}\n                 순회 하한은 통과했는데 본문이 비면 아래 단정은 미측정을 통과로 센다.",
                w.rel
            )
        });
        for body in automatic_job_bodies(&text) {
            out.push((w.rel.clone(), body));
        }
    }
    out
}

/// 주석을 뗀 사본. **명령에 있는가**를 묻는 자리에 원문을 쓰면 그 규칙을 *설명하는*
/// 주석이 명령으로 읽힌다 — 실측으로 그 함정을 밟았다: Windows 잡에서 `--all-targets`
/// 를 지우는 변이를 쐈는데 같은 잡의 주석이 그 낱말을 담고 있어 **변이가 살아남았다**.
/// 물음이 "명령인가" 이므로 사본도 명령만 남은 것이어야 한다.
fn commands_only(body: &str) -> String {
    body.lines()
        .map(|l| match l.find('#') {
            Some(at) => &l[..at],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn runs_on_windows(body: &str) -> bool {
    body.lines()
        .filter(|l| l.trim_start().starts_with("runs-on:"))
        .any(|l| l.contains("Windows") || l.contains("windows"))
}

fn runs_on_ubuntu(body: &str) -> bool {
    body.lines()
        .filter(|l| l.trim_start().starts_with("runs-on:"))
        .any(|l| l.contains("ubuntu"))
}

#[test]
fn the_compile_channel_for_guard_tests_still_exists_on_windows() {
    let jobs = automatic_jobs();
    assert!(
        jobs.len() >= JOB_FLOOR.min,
        "자동 잡을 {} 개밖에 못 읽었다(하한 {}) — {}",
        jobs.len(),
        JOB_FLOOR.min,
        JOB_FLOOR.why_this_gap
    );

    let compilers: Vec<&str> = jobs
        .iter()
        .filter(|(_, b)| runs_on_windows(b) && commands_only(b).contains("--all-targets"))
        .map(|(n, _)| n.as_str())
        .collect();
    assert!(
        !compilers.is_empty(),
        "Windows 러너에서 `--all-targets` 를 돌리는 자동 잡이 없다.\n\
         `{DOC}` 은 `crates/tasty-doc-guards/tests/*` 의 **컴파일 축에는 채널이 있다**고 \
         적는다 — 그 근거가 이 잡이다. 잡이 사라졌거나 명령이 좁아졌으면 그 문단이 \
         거짓이 된 것이니 **문서를 고쳐라.** 통합 테스트가 타깃에서 빠지면 분기 없는 \
         플랫폼 API 가 아무 데서도 안 잡힌다"
    );
}

#[test]
fn nothing_but_ubuntu_runs_the_guard_crate_tests() {
    let jobs = automatic_jobs();
    let runners: Vec<&(String, String)> = jobs
        .iter()
        .filter(|(_, b)| {
            let cmds = commands_only(b);
            cmds.contains("cargo test")
                && cmds.contains("-p tasty-doc-guards")
                && !cmds.contains("--bin ")
        })
        .collect();
    // 비영 대조 — 하나도 없으면 아래 단정이 공허하게 참이다. 그 0 은 초록보다 조용하다.
    assert!(
        !runners.is_empty(),
        "`cargo test -p tasty-doc-guards` 를 돌리는 자동 잡을 하나도 못 찾았다 — \
         술어가 죽었거나 그 채널이 통째로 사라졌다. 어느 쪽이든 `{DOC}` 의 문단이 낡았다"
    );

    let off_ubuntu: Vec<&str> = runners
        .iter()
        .filter(|(_, b)| !runs_on_ubuntu(b))
        .map(|(n, _)| n.as_str())
        .collect();
    assert!(
        off_ubuntu.is_empty(),
        "ubuntu 가 아닌 러너가 이제 가드 크레이트 테스트를 돌린다: {off_ubuntu:?}\n\
         `{DOC}` 은 그 통합 테스트의 **실행 축에는 채널이 없다**고 적는다(실행하는 자동 \
         잡은 ubuntu 하나). 채널이 늘었으면 그건 좋은 소식이고, **문단을 그에 맞게 \
         고쳐라** — 남겨 두면 다음 사람이 이미 재고 있는 것을 미측정으로 센다"
    );
}
