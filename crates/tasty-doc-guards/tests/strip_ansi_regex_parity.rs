//! ANSI escape 제거 정규식의 **사본이 하나뿐인지** 검증한다.
//!
//! 배경: 같은 정규식이 `crates/tasty-terminal` 과 `crates/tasty-output` 에 하나씩
//! 있었다. 두 크레이트는 서로 의존하지 않아 상수를 공유할 자리가 없었고, 실제로
//! 갈라졌다 — 한쪽은 `[0-9;?]`, 다른 쪽은 `[0-9;]` 였다. 이름도 달라서
//! (`strip_ansi` vs `strip_ansi_escapes`) grep 으로도 안 걸렸다.
//!
//! 지금은 `crates/tasty-ansi` 한 곳에 있다. 이 테스트가 남아 있는 이유는 **합치는
//! 것과 합쳐진 채로 있는 것이 다르기 때문**이다 — 두 소비자 중 한쪽이 "여기서만
//! 살짝 다르게" 를 이유로 자기 사본을 다시 만들면, 그건 컴파일도 되고 테스트도
//! 통과하며 리뷰에서도 안 걸린다. 갈라진 뒤에야 증상이 나온다.
//!
//! 그래서 검사 대상은 동등이 아니라 **자리와 개수**다. `crates/` 와 `src/` 어디에든
//! 두 번째 리터럴이 나타나면 실패한다.
//!
//! ## 왜 `tasty` 가 아니라 여기 사는가
//!
//! 판정 대상이 `crates/` 와 `src/` **레포 전체**인데, 루트 `tests/` 에 있으면 그 판정을
//! 받으려면 `-p tasty` 를 돌려야 한다. 그건 본 바이너리를 링크하는 패키지라 **어느
//! 크레이트를 고친 lane 도 자기 작업 중에는 안 돈다** — 크레이트를 고치고 그 크레이트를
//! 돌려 초록을 본 lane 이 조립에서 처음 빨강을 만나는 형태가 실제로 났다.
//!
//! 여기(의존 0 크레이트)로 옮기면 lane 이 `cargo test -p tasty-doc-guards` 로 초 단위에
//! 같은 판정을 받는다. `doc-guards.yml` 이 **경로 필터 없이** main push·PR 마다
//! `cargo test -p tasty-doc-guards --locked --no-fail-fast` 를 돌린다.
//! 이 배치는 ADR-0138 이 세운 선례를 그대로 따른 것이다.

use std::path::{Path, PathBuf};

/// 사본이 있어야 하는 **유일한** 자리. 여기서 벗어난 사본이 생기면 실패한다.
const EXPECTED: [&str; 1] = ["crates/tasty-ansi/src/lib.rs"];

/// raw 문자열 리터럴 본문을 뽑는다. 여는 `r"` 부터 다음 `"` 까지 — 정규식 안에
/// `"` 가 없다는 전제이며, 그 전제가 깨지면 아래 `starts_with` 검사에서 걸린다.
///
/// `Regex::new(...)` 호출에 한정하지 않는다. 사본이 `const PATTERN: &str = r"..."`
/// 로 떨어져 나가는 형태도 사본이고, 호출 형태만 보면 그것을 놓친다.
fn extract_literal(line: &str) -> Option<String> {
    let start = line.find("r\"")? + 2;
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// `.rs` 파일을 모아 ANSI escape 정규식 리터럴이 있는 자리를 전부 돌려준다.
fn collect(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for line in text.lines() {
                if !line.contains(r"\x1b\[") {
                    continue;
                }
                if let Some(lit) = extract_literal(line) {
                    out.push((path.clone(), lit));
                }
            }
        }
    }
}

#[test]
fn the_ansi_escape_regex_has_exactly_one_home() {
    // `CARGO_MANIFEST_DIR` 이 곧 레포 루트가 아니다(여기서는 크레이트 디렉토리다).
    // 공용 `repo_root()` 는 표지 파일 넷으로 자기가 잡은 경로를 검증한다.
    let root_buf = tasty_doc_guards::repo_root();
    let root = root_buf.as_path();
    let mut found = Vec::new();
    collect(&root.join("crates"), &mut found);
    collect(&root.join("src"), &mut found);
    found.sort();

    // 모수를 먼저 확정한다. 0 건이면 "전부 같다" 가 공허하게 참이 된다.
    let paths: Vec<String> = found
        .iter()
        .map(|(p, _)| {
            p.strip_prefix(root)
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert_eq!(
        paths, EXPECTED,
        "ANSI escape 정규식이 `tasty-ansi` 밖에도 있다. 두 번째 사본은 언젠가 갈라지고 \
         그때까지 조용하다 — 새로 만들지 말고 `tasty_ansi::strip_ansi` 를 불러라. \
         거처를 옮긴 것이라면 여기 경로를 고쳐라. 발견: {paths:?}"
    );

    let first = &found[0].1;
    assert!(
        first.starts_with(r"\x1b\["),
        "리터럴 추출이 어긋났다(정규식 본문이 아님): {first:?}"
    );

    // 사본이 하나면 이 루프는 돌지 않는다. 위 `assert_eq!(paths, EXPECTED)` 가 그것을
    // 보장하므로 여기 남겨 두는 것은 **거처를 둘로 늘리기로 결정한 날**을 위한 것이다 —
    // 그때 EXPECTED 만 늘리면 동등 검사가 자동으로 살아난다.
    for (path, lit) in &found[1..] {
        assert_eq!(
            lit,
            first,
            "ANSI escape 정규식 사본이 갈라졌다.\n  {}\n    {lit}\n  {}\n    {first}",
            path.display(),
            found[0].0.display()
        );
    }
}
