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
///
/// ★ **다만 그 "빈" 은 두 갈래이고 여기가 지키는 것은 한쪽뿐이다**(실측 2026-09-07,
/// [`the_row_floor_sees_a_collapsed_collection`] 의 세 칸):
/// · 코퍼스가 빔(워크플로 0 개) — **여기까지 안 온다.** 판정기가 "워크플로가 0 개다 —
///   빈 모수를 0 으로 돌려주지 않는다" 로 rc 2 를 내고, 위 시험은 그 앞의 `rc == 0`
///   단정에서 죽는다.
/// · 파일은 있는데 행이 덜 나옴(출력 배선이 죽음) — 그때 rc 는 **0** 이라 이 하한 말고는
///   아무도 안 본다. **이 하한의 실제 몫이 그쪽이다.**
///
/// **판별식** — 이 수의 독립 출처는 **워크플로 파일 개수**다. 노출 바이너리는 파일 하나에
/// 행 하나를 내므로 둘이 같아야 한다:
///
/// ```text
/// ls .github/workflows/*.yml | wc -l                                   # 파일
/// cargo test -p tasty-doc-guards --test exposed_judge_agrees_with_the_library -- --nocapture
///   → [노출 판정기] 워크플로 행 <N> · 하한 8                              # 바이너리가 낸 행
/// ```
///
/// 실측 2026-09-07(`de0572359`): 파일 **11** · 행 **11** · 하한 8(여유 3).
///
/// ★ **두 수가 갈리면 그것이 이 파일이 잡으려는 결함 자체다.** 이 테스트의 존재 이유는
/// 노출본이 네 번째 미러가 되는 것을 막는 것인데, 행이 파일보다 적으면 바이너리가 어떤
/// 워크플로를 통째로 빠뜨린 것이고 그 침묵은 하한으로는 안 잡힌다(11 → 9 여도 하한 8 은
/// 통과한다). **하한은 "출력이 비지 않았다" 까지만 말한다** — 파일 수와의 대조가 그 위를 본다.
///
/// **이 수를 내려서 초록을 만들지 마라.** 아래 대조는 행을 순회하므로 행이 줄면 그만큼
/// 적게 대조하면서 초록이 된다.
///
/// 정당한 수선: 워크플로를 실제로 지웠으면 이 수를 함께 내려라 — 그리고 그때 위 두 수가
/// 여전히 같은지 확인해라.
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

    println!("[노출 판정기] 워크플로 행 {rows} · 하한 {MIN_ROWS}");
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

/// `MIN_ROWS` 의 **양성 대조** — 모수가 하한 아래로 떨어지는 코퍼스를 만들고, 이 파일의
/// 두 단정(행 하한 · "양쪽 답이 다 나온다")이 실제로 그것을 말하는지 묻는다.
///
/// **꺼내는 리팩터가 필요 없었다.** 판정기는 뿌리를 이미 인자로 받는다(프로세스 경계라
/// 함수 인자보다 먼저 열려 있었다). 그래서 이 자리는 대조만 붙이면 된다.
///
/// 두 단정은 **독립이고 서로를 안 가려 준다.** 그것을 네 칸으로 보인다 — 한 칸만 세우면
/// 둘 중 어느 쪽이 잡았는지 못 가른다:
///
/// ```text
///                     행 하한(8)   짝(필터 있음·없음 각 1 이상)
///   레포 뿌리            통과        통과      ← 실물, 위 본 시험이 도는 상태
///   ① 필터유 1 + 무 1    실패        통과      ← 하한만 잡는다
///   ② 필터유 9           통과        실패      ← 짝만 잡는다
///   ③ 빈 디렉터리        (도달 안 함)          ← 판정기 자신이 rc 2 로 먼저 막는다
/// ```
///
/// ★ ③ 이 상수 doc 을 한 칸 좁힌다. "출력이 비면 대조가 공허하다" 는 맞지만, **코퍼스가
/// 빈 경로는 여기까지 안 온다** — 판정기가 "워크플로가 0 개다 — 빈 모수를 0 으로 돌려주지
/// 않는다" 로 rc 2 를 낸다. 그러니 이 하한이 실제로 지키는 것은 **부분 붕괴**다: 파일은
/// 있는데 행이 덜 나오는 상태(출력 배선이 죽는 형태)이고, 그때 rc 는 0 이라 하한 말고는
/// 아무도 안 본다.
#[test]
fn the_row_floor_sees_a_collapsed_collection() {
    let root =
        std::env::temp_dir().join(format!("tasty-rowfloor-{}-{}", std::process::id(), line!()));
    let dir = root.join(".github/workflows");

    // 앞선 실행의 잔여를 치운다 — 없는 것이 정상이라 실패가 정보가 아니다.
    let reset = || {
        // 지우고 다시 만든다. 없어서 나는 실패는 정보가 아니다.
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&dir).expect("임시 디렉토리를 못 만들었다");
    };

    // 워크플로 하나를 놓는다. `filtered` 가 경로 필터 유무를 가른다.
    let plant = |name: &str, filtered: bool| {
        let on = if filtered {
            "  push:\n    paths:\n      - 'src/**'\n"
        } else {
            "  push:\n    branches: [main]\n"
        };
        std::fs::write(
            dir.join(name),
            format!(
                "name: {name}\non:\n{on}jobs:\n  a:\n    runs-on: ubuntu-latest\n    \
                 steps:\n      - run: cargo test --workspace\n"
            ),
        )
        .expect("쓰기 실패");
    };

    // 위 본 시험과 **같은 독법**으로 센다 — 여기서 두 번째 독법을 만들면 이 대조가
    // 무엇을 재는지가 본 시험과 갈린다.
    let measure = || -> (i32, usize, usize, usize) {
        let (rc, stdout, _) = run(&[&root.to_string_lossy()]);
        let (mut rows, mut filtered, mut unfiltered) = (0usize, 0usize, 0usize);
        for line in stdout.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() != 6 {
                continue;
            }
            rows += 1;
            if f[2] == "yes" {
                filtered += 1;
            } else if f[1] == "yes" {
                unfiltered += 1;
            }
        }
        (rc, rows, filtered, unfiltered)
    };

    // ③ 빈 코퍼스 — 하한이 아니라 **판정기 자신**이 막는다. 이 칸이 없으면 상수 doc 이
    //    말하는 "출력이 비면" 을 이 하한의 몫으로 잘못 읽게 된다.
    reset();
    let (rc, rows, _, _) = measure();
    assert_eq!(
        rc, 2,
        "빈 모수를 0 으로 돌려주면 대조가 공허해진다 — 판정기가 거절해야 한다"
    );
    assert_eq!(
        rows, 0,
        "거절했는데 행이 나오면 rc 와 출력이 다른 말을 하는 것이다"
    );

    // ① 하한만 잡는 칸 — 짝은 만족하는데 수가 모자란다.
    reset();
    plant("unfiltered.yml", false);
    plant("filtered.yml", true);
    let (rc, rows, filtered, unfiltered) = measure();
    assert_eq!(
        rc, 0,
        "필터 유무가 하나씩 있는 코퍼스는 판정기가 답해야 한다"
    );
    assert_eq!(
        rows, 2,
        "놓은 만큼 행이 나와야 한다 — 아니면 이 대조가 판정기를 안 본다"
    );
    assert!(
        filtered > 0 && unfiltered > 0,
        "짝 단정은 이 칸에서 **통과**해야 한다 — 그래야 아래 하한 미달이 짝 덕이 아님이 갈린다"
    );
    assert!(
        rows < MIN_ROWS,
        "이 칸이 하한 위면 '하한이 무너진 상태' 를 한 번도 안 만든 것이다"
    );

    // ② 짝만 잡는 칸 — 수는 하한을 넘는데 한쪽 답만 나온다. 하한 하나로는 못 보는 형태다.
    reset();
    for i in 1..=9 {
        plant(&format!("f{i}.yml"), true);
    }
    let (rc, rows, filtered, unfiltered) = measure();
    assert_eq!(rc, 0, "판정기가 답해야 한다");
    assert!(
        rows >= MIN_ROWS,
        "이 칸은 하한을 **넘어야** 한다 — 넘지 않으면 아래 짝 미달이 하한에 가린다"
    );
    assert!(
        filtered > 0 && unfiltered == 0,
        "필터 있는 것만 놓았는데 필터 없는 것이 세어지면 이 대조가 종류를 안 가르는 것이다 \
         (필터 {filtered} · 필터없음 {unfiltered})"
    );

    // 뒷정리 실패는 무시한다 — 임시 디렉토리라 남아도 다음 실행이 먼저 지우고, 여기서
    // 죽으면 위 단정의 결과가 정리 오류에 가린다.
    let _ = std::fs::remove_dir_all(&root);
}
