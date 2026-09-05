//! 코드가 아닌 부분을 덮은 **사본**을 만든다 — 소스를 텍스트로 훑는 셸 게이트가 잴 대상.
//!
//! ## 왜 사본인가 — 그리고 왜 셸이 이걸 부르는가
//!
//! "여기 코드에 X 가 있나" 를 텍스트로 묻는 게이트는 문자열 리터럴 안의 X 를 코드로 센다.
//! 그래서 **판정 대상 형태를 회귀로 박은 픽스처가 그 게이트의 모수에 들어간다** — 가드를
//! 쓰려면 그 형태를 소스에 써야 하고, 쓴 순간 다른 게이트가 그것을 실물로 센다.
//!
//! 러스트 층은 이 문제를 이미 풀었다([`tasty_doc_guards::source_text`]). 못 푼 것은 셸
//! 층이고, 셸이 그 판정을 **다시 구현**하면 같은 물음에 답이 둘이 된다(실제로 그렇게
//! 갈려 있었다 — awk 판 하나, 한 줄 정규식 판 하나). 그래서 판정은 여기 하나로 두고
//! 셸은 사본을 읽는다. 계측기를 하나 더 만들지 않으려는 것이지 함수를 줄이는 게 아니다.
//!
//! ## 두 물음은 한 마스크로 못 답한다
//!
//! - 기본(`mask_non_code`) — 주석·문자열·문자 리터럴을 전부 덮는다. "코드에 X 가 있나".
//! - `--keep-comments`(`mask_literals`) — 문자열·문자 리터럴만 덮는다. "사유 **주석**이
//!   달려 있나" 를 함께 묻는 게이트가 쓴다. 주석까지 덮으면 그 물음의 답이 사라진다.
//!
//! 덮은 자리는 **공백**이고 줄바꿈은 남는다. 줄 번호가 보존돼야 셸이 보고하는 좌표를
//! 원본으로 읽을 수 있다.
//!
//! ## 종료코드
//!
//! 0 = 사본을 만들었다. 2 = 만들지 못했다(인자 부족·루트 없음·파일 0 개).
//! **1 은 쓰지 않는다** — 이 프로그램은 위반을 판정하지 않는다. 파일 0 개를 0 으로
//! 돌려주면 게이트가 빈 모수를 재고 조용히 초록이 된다.

use std::path::PathBuf;

use tasty_doc_guards::source_text::{mask_literals, mask_non_code, rust_sources};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
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
