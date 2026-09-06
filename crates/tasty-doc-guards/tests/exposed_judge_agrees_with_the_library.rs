//! 노출한 판정기(`workflow-channels`)가 **가드가 쓰는 라이브러리와 같은 답**을 내는가.
//!
//! 이 테스트가 없으면 노출본 자신이 **네 번째 미러**가 된다. 바이너리는 라이브러리를
//! 부르지만 출력 형태를 만들면서 판정을 조금씩 다시 쓰게 되고(예: 잡 헤더 규칙을
//! 한 번 더 구현한 `total_job_count`), 그 사본이 갈리면 밖에서 부르는 사람은 **갈린
//! 쪽**을 정본으로 읽는다. 미러를 없애려고 연 것이 미러를 하나 더 만드는 셈이다.
//!
//! **왜 이 형태가 필요한가**: 실측(2026-09-05) — 이 레포에서 하루에 세 레인이 각자
//! 판정을 흉내 냈고 셋 다 원본과 다른 답을 냈다. 갈리는 방향은 대체로 **덜 잡는 쪽**이라
//! 조용하다. 그래서 판정을 부를 수 있게 열었고, 열었으면 그 노출본이 원본과 같은지를
//! 여기서 계속 묻는다.
//!
//! ## ★ 이 테스트가 답하지 **않는** 것 — 일치는 옳음이 아니다
//!
//! 여기서 보는 것은 **갈렸는가**뿐이다. 노출본과 라이브러리가 **함께 틀리면 일치하므로
//! 초록이다.** 실측으로 확인했다(2026-09-05): 라이브러리의 잡 헤더 규칙을 2 칸에서
//! 3 칸으로 바꾸는 변이에서 이 파일의 세 단정은 전부 통과했다 — 양쪽이 같은 규칙을
//! 쓰니 당연하다. 그 변이를 잡은 것은 **내용을 단정하는** 다른 둘이다
//! (`workflow_triggers::coverage_tests::a_folded_scalar_narrowing_is_seen` 와
//! `no_filtered_scan_guard_reads_only_ignored_paths`).
//!
//! 그러니 이 파일을 "판정이 옳다" 의 근거로 쓰지 마라. 옳음은 내용 단정이 지고,
//! 여기는 **밖에서 부르는 길이 안쪽과 같은 답을 내는가**만 진다. 둘 다 필요하다.
//!
//! **하한을 둔다.** 노출이 죽어 출력이 비면 "모든 행이 일치한다" 는 **언제나 참**이 된다.
//! 행 수 하한과 "양쪽 답이 다 나온다"(필터 있는 것과 없는 것이 각각 하나 이상)를 함께
//! 단정해, 판정기가 한쪽으로만 답하는 상태를 통과로 읽지 않는다.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

use tasty_doc_guards::workflow_triggers::{
    automatic_job_bodies, filter_free_coverage, push_trigger,
};

/// 워크플로 행 수의 하한. 실측 11 (2026-09-05). 출력이 비면 대조가 공허해진다.
const MIN_ROWS: usize = 8;

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_workflow-channels"))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("판정기를 실행할 수 없다: {e}"));
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn the_exposed_judge_reports_the_same_population_as_the_library() {
    let root = repo_root();
    let (rc, stdout, stderr) = run(&[&root.to_string_lossy()]);
    assert_eq!(rc, 0, "판정기가 실패했다(rc {rc}): {stderr}");

    let mut rows = 0usize;
    let mut filtered = 0usize;
    let mut unfiltered = 0usize;
    for line in stdout.lines() {
        if !line.contains('\t') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f.len(), 6, "행 형태가 바뀌었다: {line}");
        let (name, present, path_filtered, tags_only, auto, manual) =
            (f[0], f[1], f[2], f[3], f[4], f[5]);
        rows += 1;

        let text = std::fs::read_to_string(root.join(".github/workflows").join(name))
            .unwrap_or_else(|e| panic!("read {name}: {e}"));
        let t = push_trigger(&text)
            .unwrap_or_else(|| panic!("{name}: 라이브러리가 `on:` 을 못 읽었다"));
        assert_eq!(present == "yes", t.present, "{name}: push 판정이 갈렸다");
        assert_eq!(
            path_filtered == "yes",
            t.path_filtered,
            "{name}: 경로 필터 판정이 갈렸다 — 노출본이 미러가 됐다"
        );
        assert_eq!(
            tags_only == "yes",
            t.tags_only,
            "{name}: 태그전용 판정이 갈렸다"
        );

        let lib_auto = automatic_job_bodies(&text).len();
        assert_eq!(
            auto.parse::<usize>().unwrap_or(usize::MAX),
            lib_auto,
            "{name}: 자동 잡 수가 갈렸다 — 노출본이 잡 헤더 규칙을 다시 쓴 자리다"
        );
        // 수동 전용으로 빠진 수는 **전체 − 자동** 이어야 한다. 노출본이 전체를 다른
        // 헤더 규칙으로 세면 이 차가 뜻을 잃는다.
        assert!(
            manual.parse::<usize>().is_ok(),
            "{name}: 수동 전용 잡 수가 정수가 아니다: {manual}"
        );

        if t.path_filtered {
            filtered += 1;
        } else if t.present {
            unfiltered += 1;
        }
    }

    assert!(
        rows >= MIN_ROWS,
        "워크플로 행이 {rows} 개뿐이다(하한 {MIN_ROWS}) — 출력이 비면 '모든 행이 \
         일치한다' 는 언제나 참이다"
    );
    assert!(
        filtered > 0 && unfiltered > 0,
        "판정기가 한쪽으로만 답한다(필터 {filtered} · 필터없음 {unfiltered}) — 그 \
         상태에서는 위 대조가 판정을 안 본다"
    );
}

#[test]
fn the_exposed_judge_reports_the_same_coverage_as_the_library() {
    let root = repo_root();
    let (rc, stdout, stderr) = run(&[&root.to_string_lossy()]);
    assert_eq!(rc, 0, "판정기가 실패했다(rc {rc}): {stderr}");

    let field = |key: &str| -> String {
        stdout
            .lines()
            .find_map(|l| l.strip_prefix(key))
            .unwrap_or_else(|| panic!("출력에 `{key}` 줄이 없다"))
            .to_string()
    };
    let split = |v: &str| -> BTreeSet<String> {
        v.split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    };

    let lib = filter_free_coverage(&root.join(".github/workflows"))
        .unwrap_or_else(|bad| panic!("`on:` 을 못 읽은 워크플로: {bad:?}"));

    assert_eq!(
        split(&field("named=")),
        lib.named,
        "`--test <이름>` 커버리지가 갈렸다 — 노출본이 미러가 됐다"
    );
    assert_eq!(
        split(&field("packages=")),
        lib.packages,
        "`-p <패키지>` 커버리지가 갈렸다 — 노출본이 미러가 됐다"
    );
    assert_eq!(
        field("whole_workspace=") == "yes",
        lib.whole_workspace,
        "`--workspace` 판정이 갈렸다"
    );
    // 하한: 둘 다 비면 위 세 단정이 전부 빈 집합끼리의 비교가 된다.
    assert!(
        !lib.named.is_empty() || !lib.packages.is_empty() || lib.whole_workspace,
        "필터 없는 채널을 하나도 못 읽었다 — 대조가 공허하다"
    );
}

/// 신선도 배선이 실제로 도는가. 낡은 판정기는 없는 판정기보다 나쁘고, 부르는 쪽은
/// `--check-fresh` 의 종료코드로만 그것을 안다.
#[test]
fn the_freshness_probe_answers() {
    let root = repo_root();
    let (rc, _, stderr) = run(&["--check-fresh", &root.to_string_lossy()]);
    assert_eq!(
        rc, 0,
        "방금 지은 판정기가 신선하지 않다고 답했다 — 지문 배선이 깨졌다: {stderr}"
    );
}
