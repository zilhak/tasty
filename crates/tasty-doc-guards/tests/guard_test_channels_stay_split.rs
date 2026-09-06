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

use tasty_doc_guards::workflow_triggers::automatic_job_bodies;

/// 이 주장이 사는 자리. 빨개졌을 때 고칠 곳을 실패문이 지목한다.
const DOC: &str = "docs/dev-guide/ci-gates.md";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("레포 루트")
        .to_path_buf()
}

/// 자동 회차에 도는 잡의 본문 전부.
fn automatic_jobs() -> Vec<(String, String)> {
    let dir = repo_root().join(".github/workflows");
    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("워크플로 디렉토리를 못 읽었다: {} — {e}", dir.display()));
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yml") {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for body in automatic_job_bodies(&text) {
            out.push((name.clone(), body));
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
        jobs.len() >= 8,
        "자동 잡을 {} 개밖에 못 읽었다(하한 8) — 파싱이 깨지면 아래 판정이 전부 공허하다",
        jobs.len()
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
