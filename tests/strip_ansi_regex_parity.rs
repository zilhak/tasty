//! ANSI escape 제거 정규식의 **사본 둘이 문자 단위로 같은지** 검증한다.
//!
//! 배경: 같은 정규식이 `crates/tasty-terminal/src/output_buffer.rs` 와
//! `crates/tasty-output/src/lib.rs` 에 각각 하나씩 있다. 두 크레이트는 서로
//! 의존하지 않아 상수를 공유할 자리가 없고, 실제로 한동안 갈라져 있었다 —
//! 한쪽은 `[0-9;?]`, 다른 쪽은 `[0-9;]` 였다. 이름도 달라서(`strip_ansi` vs
//! `strip_ansi_escapes`) grep 으로도 안 걸렸다.
//!
//! "둘을 같게 맞춰 둔다" 를 **주석으로만** 적어두면 다음 사람이 한쪽만 고친다.
//! 이 테스트가 그 집행 채널이다. `cargo test --workspace` 가 돌린다 — 그 잡은
//! 수동 전용이라 자동 채널은 아니다(`docs/dev-guide/ci-gates.md`).
//!
//! 하나로 합치는 것(공용 크레이트 신설)은 별개 작업이고, 합쳐지면 이 테스트는
//! 사본 수 1 을 보고 실패한다 — 그때 지우면 된다.

use std::path::{Path, PathBuf};

/// 사본이 있어야 하는 자리. 여기서 벗어난 사본이 생기면 실패한다.
const EXPECTED: [&str; 2] = [
    "crates/tasty-output/src/lib.rs",
    "crates/tasty-terminal/src/output_buffer.rs",
];

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
fn the_two_strip_ansi_regexes_are_character_identical() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
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
        "ANSI escape 정규식 사본의 자리가 바뀌었다. 새 사본이 생겼으면 여기에 등록하고 \
         동등을 유지하거나, 하나로 합쳤으면 이 테스트를 지워라. 발견: {paths:?}"
    );

    let first = &found[0].1;
    assert!(
        first.starts_with(r"\x1b\["),
        "리터럴 추출이 어긋났다(정규식 본문이 아님): {first:?}"
    );

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
