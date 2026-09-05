//! 출하되지 않는 줄을 **지운 사본**을 만든다 — 파일 SLOC 게이트가 잴 대상.
//!
//! ## 왜 사본인가
//!
//! 게이트의 계측기는 `tokei` 다. tokei 는 파일을 통째로 세고 "이 줄은 빼라" 를 모른다.
//! 그래서 판정(무엇이 출하되는가)과 계측(몇 줄인가)을 갈라, 판정은 여기서 하고 계측은
//! tokei 가 그대로 한다. **계측기를 하나 더 만들지 않으려는 것**이다 — 줄 수를 세는
//! 두 번째 구현이 생기면 게이트가 무엇을 재는지가 둘로 갈린다.
//!
//! 지운 줄은 **빈 줄로 남긴다.** 줄 번호가 보존돼야 tokei 의 보고를 원본 좌표로 읽을 수
//! 있고, 사본이 원본보다 짧아지면 "지운 결과" 와 "안 읽힌 결과" 가 구분되지 않는다.
//!
//! ## 지우는 것
//!
//! 파일 안의 인라인 `#[cfg(test)]` 범위만 지운다. 파일 **전체**가 출하 밖인 경우
//! (`#[cfg(test)] mod x;` 로 선언된 별도 파일 · cargo 통합 타깃)는 게이트 스크립트의
//! `skip()` 이 여전히 담당한다 — 그 축은 판정이 안 바뀌므로 여기서 건드리지 않는다.
//!
//! ## 종료코드
//!
//! 0 = 사본을 만들었다. 2 = 만들지 못했다(인자 부족·루트 없음·파일 0 개).
//! **1 은 쓰지 않는다** — 이 프로그램은 위반을 판정하지 않는다. 파일 0 개를 0 으로
//! 돌려주면 게이트가 빈 모수를 재고 조용히 초록이 된다.

use std::path::{Path, PathBuf};

use tasty_doc_guards::cfg_predicate::cfg_gated_lines;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!(
            "usage: strip-cfg-test <out-dir> <repo-root> <scan-root>...\n\
             출하되지 않는 줄을 빈 줄로 바꾼 사본을 <out-dir> 아래에 만든다."
        );
        std::process::exit(2);
    }
    let out_dir = PathBuf::from(&args[0]);
    let root = PathBuf::from(&args[1]);
    if !root.is_dir() {
        eprintln!("레포 루트가 아니다: {}", root.display());
        std::process::exit(2);
    }

    let mut files = Vec::new();
    for scan in &args[2..] {
        let dir = root.join(scan);
        if !dir.is_dir() {
            eprintln!("스캔 루트가 없다: {}", dir.display());
            std::process::exit(2);
        }
        gather_rs(&dir, &mut files);
    }
    if files.is_empty() {
        eprintln!("`.rs` 를 하나도 못 찾았다 — 빈 모수는 측정 실패다");
        std::process::exit(2);
    }

    for path in &files {
        let Ok(rel) = path.strip_prefix(&root) else {
            eprintln!("스캔 결과가 루트 밖이다: {}", path.display());
            std::process::exit(2);
        };
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s.replace("\r\n", "\n"),
            Err(e) => {
                eprintln!("소스를 읽을 수 없다: {} — {e}", path.display());
                std::process::exit(2);
            }
        };
        let dst = out_dir.join(rel);
        if let Some(parent) = dst.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            eprintln!("사본 디렉토리를 만들 수 없다: {} — {e}", parent.display());
            std::process::exit(2);
        }
        if let Err(e) = std::fs::write(&dst, strip(&src)) {
            eprintln!("사본을 쓸 수 없다: {} — {e}", dst.display());
            std::process::exit(2);
        }
    }
    println!("{}", files.len());
}

/// 인라인 `#[cfg(test)]` 가 덮는 줄을 빈 줄로 바꾼다. 줄 수는 그대로다.
fn strip(src: &str) -> String {
    let lines: Vec<&str> = src.split('\n').collect();
    let gated = cfg_gated_lines(&lines, "test");
    let mut out = String::with_capacity(src.len());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if !gated[i] {
            out.push_str(line);
        }
    }
    out
}

/// 빌드 캐시는 세지 않는다 — 이름이 아니라 표식으로 가른다(`CARGO_TARGET_DIR` 로 다른
/// 이름을 주면 이름 가지치기는 통째로 새어 들어간다).
fn gather_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() == "target" || tasty_doc_guards::is_build_cache_dir(&path) {
                continue;
            }
            gather_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}
