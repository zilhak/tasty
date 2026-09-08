//! **`ci-gates.md` 가 가드 테스트에 대해 하는 두 주장을 워크플로에서 다시 읽는다.**
//!
//! 그 문서는 `crates/tasty-doc-guards/tests/*` 를 두고 축이 둘이라고 적는다 — 컴파일
//! 채널(Windows 잡의 `--all-targets`)과 실행 채널. 그 두 문장은 **워크플로가 정하는
//! 사실**인데 지키는 것이 없었다.
//!
//! **실행 축의 사실이 2026-09-07 에 바뀌었다.** 오래 `doc-guards.yml`(ubuntu) 하나였고
//! 이 파일은 "ubuntu 말고는 아무도 안 돌린다" 를 지켰다. 지금은 `crossplatform-check.yml`
//! 의 Windows 잡이 `-p tasty-doc-guards` 로 함께 돌린다 — 이 크레이트의 스캔 가드가
//! 경로를 문자열로 펴는 자리에서 구분자 때문에 Windows 에서만 **조용한 0** 이 되는
//! 형태를 열려는 것이다. 그래서 이 파일이 지키는 것도 **부재가 아니라 두 채널의 존재**로
//! 바뀌었다. 부재를 지키던 단정을 그대로 두면 채널이 는 날 영구히 빨갛다.
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

use std::path::{Path, PathBuf};

use tasty_doc_guards::floored_walk::{Descend, Floor, walk_with_floor};
use tasty_doc_guards::workflow_triggers::automatic_job_bodies;

/// 이 주장이 사는 자리. 빨개졌을 때 고칠 곳을 실패문이 지목한다.
const DOC: &str = "docs/dev-guide/ci-gates.md";

/// 워크플로 순회의 하한. 이 아래로 모이면 순회가 죽은 것으로 본다.
const WORKFLOW_FLOOR: Floor = Floor {
    min: 8,
    measured: 11,
    measured_on: "2026-09-07",
    counted_on: tasty_doc_guards::floored_walk::CountedOn::NEVER_COUNTED,
    why_this_gap: "이 모수는 `.github/workflows/*.yml` 의 수다. 워크플로는 통합할 때 \
                   가끔 합쳐지므로 몇 개는 줄 수 있지만, 8 아래로 떨어지는 것은 파일이 \
                   줄어든 게 아니라 순회 루트가 어긋난 것이다.",
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
    automatic_jobs_under(&repo_root().join(".github/workflows"), &WORKFLOW_FLOOR)
}

/// 한 디렉토리의 `.yml` 에서 자동 잡을 뽑는다 — **순회 뿌리와 하한이 인자다.**
///
/// 뿌리와 하한은 이 파일의 성질이 아니라 **모수의 성질**이다. 상수로 박아 두면 이
/// 판독을 레포 말고 다른 워크플로 묶음에 태울 방법이 없다.
fn automatic_jobs_under(dir: &Path, floor: &Floor) -> Vec<(String, String)> {
    // 공용 순회를 쓴다. 직접 `read_dir` 하면 디렉토리가 비거나 못 읽혔을 때 "위반 0" 이
    // 나오고, 그것은 위반이 없다는 뜻이 아니라 아무것도 안 봤다는 뜻이다.
    let walked = walk_with_floor(dir, dir, floor, Descend::Everything, &|w| {
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

/// Windows 러너에서 `--all-targets` 를 돌리는 잡의 이름.
///
/// 두 물음이 곱해진 자리다 — **어디서 도는가**(`runs-on:` 줄)와 **무엇을 돌리는가**
/// (주석 뗀 명령). 어느 한쪽을 넓히면 이 목록은 안 비고, 안 비면 아래 단정은 초록이다.
fn windows_compile_jobs(jobs: &[(String, String)]) -> Vec<&str> {
    jobs.iter()
        .filter(|(_, b)| runs_on_windows(b) && commands_only(b).contains("--all-targets"))
        .map(|(n, _)| n.as_str())
        .collect()
}

/// 가드 크레이트의 **테스트를 돌리는** 잡.
///
/// `--bin` 을 빼는 것이 요점이다. `cargo test -p tasty-doc-guards --bin mask-source` 는
/// 판정기를 짓는 명령이지 가드를 돌리는 명령이 아니다 — 그것을 실행 채널로 세면 채널이
/// 통째로 사라져도 이 목록이 안 빈다.
fn guard_test_jobs(jobs: &[(String, String)]) -> Vec<&(String, String)> {
    jobs.iter()
        .filter(|(_, b)| {
            let cmds = commands_only(b);
            cmds.contains("cargo test")
                && cmds.contains("-p tasty-doc-guards")
                && !cmds.contains("--bin ")
        })
        .collect()
}

/// 잡 헤더 판독이 **살아 있는가** — 하한이 아니라 갈래 확인이다.
///
/// 한때 여기 잡 수의 하한(8)이 있었고, 실측 2026-09-08 에 그것이 **겨냥이 어긋나 있음**이
/// 드러났다. 잰 값: 워크플로 파일 11 개 · 자동 잡 20 개, 그중 이 파일이 판정하는 두 채널이
/// 사는 파일은 둘(각각 1 과 5, 합 6)이다. 네 갈래로 갈라 보면:
///
/// - 두 채널 파일 중 하나만 판독이 죽는다 → 잡 수는 19 또는 15 라 **하한이 침묵**하고,
///   아래 채널 단정들이 운다. 하한이 더한 것 0.
/// - 채널을 안 담은 아홉 파일의 판독이 죽는다 → 잡 수가 6 이라 **하한만 운다.**
///   그런데 이 파일이 주장하는 두 채널은 **멀쩡하다** — 즉 그 발화는 거짓 경보다.
/// - 판독이 통째로 죽는다 → 하한도 울고 채널 단정도 전부 운다.
///
/// 즉 그 하한이 **홀로 잡는 진짜 결함은 없고, 홀로 내는 거짓 경보는 있었다.** 원인은
/// 좌변이 물음과 안 맞은 것이다 — 하한의 모수는 레포 전체의 자동 잡인데 이 파일이
/// 판정하는 것은 이름 붙은 두 채널이다. 그리고 그 여유(8 대 20)는 잰 적이 없다.
/// 안 잰 수를 하한 옆에 적으면 다음 사람이 실측으로 읽는다.
///
/// 그래서 남기는 것은 **0 과 1 만 가르는 확인** 하나다: 아무것도 못 읽었으면 아래
/// 단정들은 "그 잡이 없다" 가 아니라 "그 잡을 안 봤다" 를 근거로 판정한다. 그 둘은
/// 다른 사실이고 고치는 곳도 다르다.
///
/// ☆ **여전히 안 닫는 것 — 부분 실명.** 열한 중 아홉의 판독이 죽어도 이 확인은 통과한다.
/// 그것을 닫으려면 잡 **헤더** 수를 파일마다 묻는 판독이 필요한데(자동/수동을 가르기
/// 전의 수), 지금 `workflow_triggers` 에는 그 갈래가 없다. 하한을 되살려도 안 닫힌다 —
/// 위 넷째 갈래가 그 이유다. 필요한 것은 수가 아니라 **파일마다의 0 대 1** 이다.
#[test]
fn the_compile_channel_for_guard_tests_still_exists_on_windows() {
    let jobs = automatic_jobs();
    assert!(
        !jobs.is_empty(),
        "자동 회차에 도는 잡을 하나도 못 읽었다 — 잡 헤더 판독이 죽었다는 뜻이다 \
         (그 판독은 2 칸 들여쓰기 관례에 매달려 있다 — `automatic_job_bodies` 의 doc 참조). \
         이 상태에서 아래 단정들은 \"그 잡이 없다\" 가 아니라 \"그 잡을 안 봤다\" 를 근거로 \
         판정한다. 워크플로 파일 자체가 안 읽힌 것이라면 `WORKFLOW_FLOOR` 가 먼저 운다 — \
         저쪽이 조용한데 여기가 울면 파일은 읽혔고 **잡 헤더 판독만** 깨진 것이다."
    );

    let compilers = windows_compile_jobs(&jobs);
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
fn the_execution_channel_for_guard_tests_spans_ubuntu_and_windows() {
    let jobs = automatic_jobs();
    let runners = guard_test_jobs(&jobs);
    // 비영 대조 — 하나도 없으면 아래 단정이 공허하게 참이다. 그 0 은 초록보다 조용하다.
    assert!(
        !runners.is_empty(),
        "`cargo test -p tasty-doc-guards` 를 돌리는 자동 잡을 하나도 못 찾았다 — \
         술어가 죽었거나 그 채널이 통째로 사라졌다. 어느 쪽이든 `{DOC}` 의 문단이 낡았다"
    );

    // ubuntu 채널 — `doc-guards.yml`. 이것이 **문서만 바뀐 push 에서 도는 유일한 채널**이라
    // (그 잡에만 `paths-ignore` 가 없다) 사라지면 실행 축이 조건부가 된다.
    assert!(
        runners.iter().any(|(_, b)| runs_on_ubuntu(b)),
        "ubuntu 러너가 가드 크레이트 테스트를 하나도 안 돌린다. `{DOC}` 은 그 채널을 \
         **문서만 바뀐 push 에서 도는 유일한 테스트 채널**로 적는다 — 그 문단이 거짓이 \
         됐으면 문서를 고치고, 아니면 `doc-guards.yml` 이 좁아진 것이다"
    );

    // Windows 채널 — `crossplatform-check.yml`. **이쪽이 OS 축을 연다.** 없어지면
    // 경로 구분자·줄끝 축이 다시 Linux 초록 뒤로 숨는다(그 실패는 예외가 아니라 조용한 0 이다).
    assert!(
        runners.iter().any(|(_, b)| runs_on_windows(b)),
        "Windows 러너가 가드 크레이트 테스트를 하나도 안 돌린다. `{DOC}` 은 실행 축에 \
         **ubuntu 와 Windows 두 채널**이 있다고 적는다 — 스캔 가드가 경로를 문자열로 펴는 \
         자리는 Windows 에서만 어긋나고 그 어긋남은 예외가 아니라 **조용한 0**(위반을 \
         못 찾고 통과)이라, 이 채널이 없으면 Linux 초록이 그것을 영영 가린다. 채널을 \
         뺐으면 그 문단도 함께 고쳐라"
    );
}

/// 합성 잡 명부 — **디스크도 워크플로 파일도 안 만진다.**
///
/// 이 축이 묻는 것은 파일이 실재하는가가 아니라 **본문에서 무엇을 읽어 내는가**라,
/// 잡 이름과 본문 두 칸만 있으면 된다. 이름은 실재하지 않는 `zone-*.yml` 로 짓는다 —
/// 레포 경로처럼 생긴 합성 좌표는 주석·문자열에 적히는 순간 좌표 가드를 깨운다.
fn jobs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(n, b)| ((*n).to_string(), (*b).to_string()))
        .collect()
}

/// 명령 판독과 러너 판독의 **극성** — 무엇을 세고 무엇을 안 세는가.
///
/// 두 술어가 다 "본문에 그 낱말이 있는가" 로 넓어질 수 있고, 넓어지면 아래 두 단정은
/// 영영 초록이다. 넓힌 쪽이 무엇을 삼키는지를 여기서 태운다.
#[test]
fn the_readers_look_at_commands_and_at_the_runs_on_line_only() {
    let roster = jobs(&[
        // ① 진짜 컴파일 채널. 러너도 명령도 맞다. 스텝 이름이 **다른 쪽 러너 이름**을
        //    담는다 — 러너 판독이 본문 전체를 보면 이 잡이 ubuntu 로도 세어진다.
        (
            "zone-alpha.yml",
            "    runs-on: [self-hosted, Windows]\n    steps:\n      - name: parity with the ubuntu job\n        run: cargo clippy --workspace --all-targets --locked\n",
        ),
        // ② 낱말은 있는데 **주석 안**에 있다. 명령이 아니다.
        (
            "zone-beta.yml",
            "    runs-on: windows-latest\n    steps:\n      - run: cargo check   # --all-targets 는 여기선 안 쓴다\n",
        ),
        // ③ 명령은 맞는데 **러너가 아니다**. `Windows` 는 스텝 이름에만 있다.
        (
            "zone-gamma.yml",
            "    runs-on: ubuntu-latest\n    steps:\n      - name: mirror of the Windows job\n        run: cargo clippy --all-targets\n",
        ),
    ]);

    assert_eq!(
        windows_compile_jobs(&roster),
        vec!["zone-alpha.yml"],
        "주석 안의 낱말과 스텝 이름 속 러너 이름은 둘 다 안 세야 한다"
    );

    // 러너 판독을 따로 본다 — 위 목록이 비지 않는 이유가 둘 중 어느 쪽인지 갈린다.
    assert!(
        runs_on_windows(&roster[1].1),
        "runs-on 줄의 windows 를 못 셌다"
    );
    assert!(
        !runs_on_windows(&roster[2].1),
        "스텝 이름 속 `Windows` 를 러너로 셌다"
    );
    assert!(
        runs_on_ubuntu(&roster[2].1),
        "runs-on 줄의 ubuntu 를 못 셌다"
    );
    assert!(
        !runs_on_ubuntu(&roster[0].1),
        "runs-on 줄이 아닌 곳의 ubuntu 를 러너로 셌다"
    );

    // 명령 판독을 따로 본다.
    assert!(
        !commands_only(&roster[1].1).contains("--all-targets"),
        "주석을 안 뗐다 — 규칙을 *설명하는* 주석이 명령으로 읽힌다"
    );
    assert!(
        commands_only(&roster[0].1).contains("--all-targets"),
        "주석을 떼면서 명령까지 지웠다"
    );
}

/// 실행 채널 명부가 **판정기 빌드를 채널로 세지 않는가.**
///
/// `cargo test -p tasty-doc-guards --bin mask-source` 는 판정기를 짓는 명령이다. 그것을
/// 채널로 세면 가드를 돌리는 잡이 통째로 사라져도 이 목록이 안 비고, 안 비면 위
/// 비영 대조가 공허하게 참이 된다.
#[test]
fn the_runner_roster_excludes_the_judge_build_and_the_other_package() {
    let roster = jobs(&[
        (
            "zone-ubuntu.yml",
            "    runs-on: ubuntu-latest\n    steps:\n      - run: cargo test -p tasty-doc-guards --locked\n",
        ),
        (
            "zone-windows.yml",
            "    runs-on: [self-hosted, Windows]\n    steps:\n      - run: cargo test -p tasty-doc-guards --locked --no-fail-fast\n",
        ),
        // 판정기를 짓는 잡. 같은 크레이트지만 가드를 안 돌린다.
        (
            "zone-judge.yml",
            "    runs-on: ubuntu-latest\n    steps:\n      - run: cargo test -p tasty-doc-guards --bin mask-source\n",
        ),
        // 다른 크레이트.
        (
            "zone-other.yml",
            "    runs-on: ubuntu-latest\n    steps:\n      - run: cargo test -p tasty --locked\n",
        ),
    ]);

    let names: Vec<&str> = guard_test_jobs(&roster)
        .iter()
        .map(|(n, _)| n.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["zone-ubuntu.yml", "zone-windows.yml"],
        "판정기 빌드와 다른 크레이트는 실행 채널이 아니다"
    );

    // 두 OS 축이 실제로 갈리는지 — 이 명부에서 각각 하나씩 나와야 한다.
    let runners = guard_test_jobs(&roster);
    assert!(
        runners.iter().any(|(_, b)| runs_on_ubuntu(b)),
        "ubuntu 채널을 못 골랐다"
    );
    assert!(
        runners.iter().any(|(_, b)| runs_on_windows(b)),
        "Windows 채널을 못 골랐다"
    );

    // 컴파일 채널과 실행 채널은 다른 물음이다 — 이 명부에는 `--all-targets` 가 없다.
    assert!(
        windows_compile_jobs(&roster).is_empty(),
        "실행 채널을 컴파일 채널로 셌다"
    );
}
