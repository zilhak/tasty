//! 셸 검사에 사용할 소스 사본을 만든다. 판독기는 source_text 모듈에서 공유한다.
//! 기본은 주석·문자열·문자 리터럴을 공백으로 바꾸고, --keep-comments는 주석을 남긴다.
//! 줄바꿈을 유지하므로 검사 결과의 줄 번호로 원본을 찾을 수 있다.
//!
//! 종료코드 0은 사본 생성 완료, 2는 인자·루트·파일 수집/쓰기 실패다.
//! 파일이 하나도 없으면 성공으로 처리하지 않는다. 이 도구는 위반 여부를 판정하지 않는다.

use std::path::PathBuf;

use tasty_doc_guards::source_text::{mask_literals, mask_non_code, rust_sources};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--check-fresh") {
        let root = args
            .get(1)
            .map_or_else(|| PathBuf::from("."), PathBuf::from);
        check_fresh(&root);
    }
    let keep_comments = args.iter().any(|a| a == "--keep-comments");
    args.retain(|a| a != "--keep-comments");
    if args.len() < 3 {
        eprintln!(
            "usage: mask-source [--keep-comments] <out-dir> <repo-root> <scan-root>...\n\
             코드가 아닌 부분을 공백으로 덮은 사본을 <out-dir> 아래에 만든다.\n\
             --keep-comments: 주석은 원문 그대로 남긴다(사유 주석을 묻는 게이트용)."
        );
        std::process::exit(2);
    }
    let out_dir = PathBuf::from(&args[0]);
    let root = PathBuf::from(&args[1]);
    if !root.is_dir() {
        eprintln!("레포 루트가 아니다: {}", root.display());
        std::process::exit(2);
    }

    let scan_roots: Vec<&str> = args[2..].iter().map(String::as_str).collect();
    for scan in &scan_roots {
        if !root.join(scan).is_dir() {
            eprintln!("스캔 루트가 없다: {}", root.join(scan).display());
            std::process::exit(2);
        }
    }
    let files = rust_sources(&root, &scan_roots);
    if files.is_empty() {
        eprintln!("`.rs` 를 하나도 못 찾았다 — 빈 모수는 측정 실패다");
        std::process::exit(2);
    }

    for (rel, src) in &files {
        let body = if keep_comments {
            mask_literals(src)
        } else {
            mask_non_code(src)
        };
        let dst = out_dir.join(rel);
        if let Some(parent) = dst.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            eprintln!("사본 디렉토리를 만들 수 없다: {} — {e}", parent.display());
            std::process::exit(2);
        }
        if let Err(e) = std::fs::write(&dst, body) {
            eprintln!("사본을 쓸 수 없다: {} — {e}", dst.display());
            std::process::exit(2);
        }
    }
    println!("{}", files.len());
}

/// 공통 라이브러리 지문과 함께 이 바이너리의 소스도 비교한다.
const OWN_SOURCE: &str = include_str!("mask-source.rs");
const OWN_REL: &str = "crates/tasty-doc-guards/src/bin/mask-source.rs";

/// --check-fresh: 0은 소스 일치, 1은 재빌드 필요, 3은 소스 부재로 비교할 수 없음.
fn check_fresh(root: &std::path::Path) -> ! {
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
