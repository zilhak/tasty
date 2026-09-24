//! workflow_triggers의 자동 실행 판정을 셸에서도 같은 구현으로 사용하도록 제공한다.
//!
//! 파일별 출력은 탭으로 구분한다:
//! <파일> <push> <path_filtered> <tags_only> <자동잡> <수동전용잡>
//! 마지막에는 필터 없는 채널의 named=, packages=, whole_workspace=를 출력한다.
//! yes/no와 정수·목록을 읽어 위반 여부를 판단하는 것은 호출자의 역할이다.
//!
//! 종료코드 0은 결과 출력, 2는 루트 부재·워크플로 없음·on 선언 판독 실패다.

use std::path::{Path, PathBuf};

use tasty_doc_guards::workflow_triggers::{
    automatic_job_bodies, filter_free_coverage, job_header_count, push_trigger,
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
        let all = job_header_count(&text);
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
