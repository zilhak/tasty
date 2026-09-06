//! 워크플로의 **채널 판정**을 밖에서 부를 수 있게 연다 — 레인이 미러를 못 만들게 하려고.
//!
//! ## 왜 바이너리인가
//!
//! 이 판정("이 워크플로가 매 push 도는가" · "이 잡이 자동 회차에 도는가" · "필터 없는
//! 채널이 무엇을 덮는가")은 러스트 층에 이미 하나로 있다
//! ([`tasty_doc_guards::workflow_triggers`]). 못 부르는 것은 **러스트 밖**이고, 못 부르면
//! 흉내 낸다. 흉내 낸 것은 갈린다 — 실측(2026-09-05): 하루에 세 레인이 각자 사본을
//! 만들었고 셋 다 원본과 다른 답을 냈다. 그중 하나는 `paths-ignore` 를 **가진** 워크플로를
//! "필터 없음" 으로 냈다.
//!
//! ★ **갈리는 방향은 대체로 조용한 쪽이다.** 사본은 원본보다 단순해서 **덜 잡는다** —
//! 모수를 줄이는 차이는 언제나 더 초록이라, 갈렸다는 사실 자체가 안 보인다.
//!
//! ## 답하는 물음
//!
//! 파일마다 한 줄: `<파일>\t<push>\t<path_filtered>\t<tags_only>\t<자동잡>\t<수동전용잡>`
//! 그리고 마지막 세 줄에 필터 없는 채널의 커버리지(`named=` · `packages=` ·
//! `whole_workspace=`). 값은 `yes`/`no` 와 정수뿐이라 셸이 그대로 쓴다.
//!
//! **이 프로그램은 위반을 판정하지 않는다.** 무엇이 사각인지, 어떤 서술이 거짓인지는
//! 부르는 쪽이 정한다 — 여기서는 **인구**만 낸다.
//!
//! ## 종료코드
//!
//! 0 = 판정을 냈다. 2 = 못 냈다(루트 없음 · 워크플로 0 개 · `on:` 을 못 읽음).
//! **1 은 쓰지 않는다** — 위반을 판정하지 않으므로 "위반 있음" 이라는 값이 없다.
//! 워크플로 0 개를 0 으로 돌려주면 부르는 쪽이 빈 모수를 재고 조용히 초록이 된다.

use std::path::{Path, PathBuf};

use tasty_doc_guards::workflow_triggers::{
    automatic_job_bodies, filter_free_coverage, push_trigger,
};

const OWN_SOURCE: &str = include_str!("workflow-channels.rs");
const OWN_REL: &str = "crates/tasty-doc-guards/src/bin/workflow-channels.rs";

fn check_fresh(root: &Path) -> ! {
    use tasty_doc_guards::freshness::{Freshness, check};
    match check(root, OWN_REL, OWN_SOURCE) {
        Freshness::Fresh => std::process::exit(0),
        Freshness::Stale(why) => {
            eprintln!("{why}");
            std::process::exit(1)
        }
        Freshness::Undecidable => std::process::exit(3),
    }
}

fn yn(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--check-fresh") {
        let root = args
            .get(1)
            .map_or_else(|| PathBuf::from("."), PathBuf::from);
        check_fresh(&root);
    }
    let root = args
        .first()
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let dir = root.join(".github/workflows");

    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("워크플로 디렉토리를 못 읽었다: {}", dir.display());
        std::process::exit(2);
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "yml" || x == "yaml"))
        .collect();
    paths.sort();
    if paths.is_empty() {
        eprintln!("워크플로가 0 개다 — 빈 모수를 0 으로 돌려주지 않는다");
        std::process::exit(2);
    }

    let mut out = String::new();
    for path in &paths {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("{name}: 못 읽었다");
            std::process::exit(2);
        };
        let Some(t) = push_trigger(&text) else {
            eprintln!("{name}: `on:` 을 못 읽었다 — 판정 불가는 통과가 아니다");
            std::process::exit(2);
        };
        let auto = automatic_job_bodies(&text).len();
        let all = total_job_count(&text);
        out.push_str(&format!(
            "{name}\t{}\t{}\t{}\t{auto}\t{}\n",
            yn(t.present),
            yn(t.path_filtered),
            yn(t.tags_only),
            all.saturating_sub(auto),
        ));
    }

    match filter_free_coverage(&dir) {
        Ok(c) => {
            out.push_str(&format!(
                "named={}\n",
                c.named.iter().cloned().collect::<Vec<_>>().join(",")
            ));
            out.push_str(&format!(
                "packages={}\n",
                c.packages.iter().cloned().collect::<Vec<_>>().join(",")
            ));
            out.push_str(&format!("whole_workspace={}\n", yn(c.whole_workspace)));
        }
        Err(bad) => {
            eprintln!("`on:` 을 못 읽은 워크플로: {}", bad.join(", "));
            std::process::exit(2);
        }
    }
    print!("{out}");
}

/// 잡 헤더 수. [`automatic_job_bodies`] 와 **같은 헤더 규칙**을 써야 두 수의 차가
/// "수동 전용이라 빠진 잡" 이 된다 — 규칙이 갈리면 그 차가 뜻을 잃는다.
fn total_job_count(yaml: &str) -> usize {
    let mut n = 0usize;
    let mut in_jobs = false;
    for line in yaml.replace("\r\n", "\n").lines() {
        if line.starts_with("jobs:") {
            in_jobs = true;
            continue;
        }
        if in_jobs
            && line.starts_with("  ")
            && !line.starts_with("   ")
            && line.trim_end().ends_with(':')
        {
            n += 1;
        }
    }
    n
}
