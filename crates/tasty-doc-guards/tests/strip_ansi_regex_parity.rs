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

/// 이 파일 자신의 자리(레포 루트 기준). 검출 패턴을 raw 리터럴로 들고 있어 자기 자신이
/// 사본으로 잡히므로 판정에서 뺀다 — 근거는 아래 사용처 주석에 있다.
const SELF_PATH: &str = "crates/tasty-doc-guards/tests/strip_ansi_regex_parity.rs";

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

/// 순회 뿌리마다 두는 **자기 하한** — 그 뿌리 아래에서 실제로 훑은 `.rs` 파일 수.
///
/// **뿌리가 둘 이상인데 하한이 하나뿐이면 그 가드는 가장 작은 뿌리만큼만 본다.** 여기는
/// 그보다 나빴다 — 하한이 아예 없었고, 아래의 공허 방지 단정(`before > found.len()`)은
/// `crates` 하나로 충족되므로 **`src` 순회가 통째로 죽어도 이 시험은 초록이었다.**
/// `collect` 이 `read_dir` 실패를 조용히 `return` 하므로 그 죽음은 소리도 안 낸다.
///
/// 하한을 **찾은 건수**가 아니라 **훑은 파일 수**에 거는 이유: 이 가드가 찾는 리터럴은
/// 설계상 한 자리뿐이라(`EXPECTED`) 건수로는 `src` 에 하한을 걸 수가 없다 — 정상 상태의
/// `src` 건수가 0 이다. 물어야 할 것은 "몇 건 찾았나" 가 아니라 **"그 뿌리를 봤나"** 다.
///
/// 실측 2026-09-07(두 계기 일치 — `find <뿌리> -name '*.rs'` 와 `git ls-files`):
/// crates **664** · src **598**. 하한은 550 · 500 으로 각각 여유 114 · 98(약 17%)이다.
/// **여유 0 을 베끼지 않는다** — 이 수는 크레이트 신설·파일 정리로 정상적으로 흔들리고,
/// 이 하한이 답해야 하는 물음은 "몇 개인가" 가 아니라 "뿌리가 죽었나" 이기 때문이다.
/// 뿌리 하나가 죽으면 그 값은 0 이 되므로 여유가 100 이든 0 이든 똑같이 잡힌다.
const MIN_SCANNED: [(&str, usize); 2] = [("crates", 550), ("src", 500)];

/// `.rs` 파일을 모아 ANSI escape 정규식 리터럴이 있는 자리를 전부 돌려준다.
///
/// 돌려주는 값은 **훑은 `.rs` 파일 수**다 — 호출자가 뿌리별 하한을 걸 수 있게 한다.
fn collect(dir: &Path, out: &mut Vec<(PathBuf, String)>) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut scanned = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scanned += collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            scanned += 1;
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
    scanned
}

#[test]
fn the_ansi_escape_regex_has_exactly_one_home() {
    // `CARGO_MANIFEST_DIR` 이 곧 레포 루트가 아니다(여기서는 크레이트 디렉토리다).
    // 공용 `repo_root()` 는 표지 파일 넷으로 자기가 잡은 경로를 검증한다.
    let root_buf = tasty_doc_guards::repo_root();
    let root = root_buf.as_path();
    let mut found = Vec::new();
    // 뿌리마다 따로 세고 따로 단정한다. 합으로 묶으면 한쪽이 죽어도 다른 쪽이 메운다.
    for (name, floor) in MIN_SCANNED {
        let scanned = collect(&root.join(name), &mut found);
        println!("[ANSI 정규식 한 집] 뿌리 {name} 아래 .rs {scanned} · 하한 {floor}");
        assert!(
            scanned >= floor,
            "순회 뿌리 `{name}` 이 {scanned} 개만 훑었다(하한 {floor}). 그 뿌리가 죽었거나 \
             자리가 바뀌었다 — `collect` 은 `read_dir` 실패를 조용히 넘기므로 이 단정이 \
             없으면 이 시험은 나머지 뿌리만 보고 초록이 난다. \
             **하한을 내려서 초록을 만들지 마라**: 내리면 이 가드가 보는 범위가 그만큼 준다."
        );
    }
    found.sort();

    // **자기 자신은 뺀다.** 이 가드는 루트 `tests/` 에 살 때 판정 대상(`crates/` · `src/`)
    // 밖이었다. 의존 0 크레이트로 옮기면서 대상 **안으로** 들어왔고, 검출 패턴을 raw
    // 리터럴로 들고 있으니 자기를 두 번째·세 번째 사본으로 센다. 여기서 빼는 것은 판정을
    // 좁히는 것이 아니라 **옮기기 전 모수를 그대로 유지하는 것**이다.
    //
    // 뺀 건수를 단정한다 — 파일이 옮겨지거나 이름이 바뀌면 이 면제가 조용히 0 건이 되고,
    // 그러면 자기 사본이 다시 세어져 이 가드가 영원히 빨개진다. 그때는 여기서 죽는 것이
    // 맞다(면제가 안 걸렸다는 것을 값으로 말한다).
    // 비교는 문자열이 아니라 `Path` 로 한다 — `Path` 의 동등성은 component 단위라
    // 구분자를 손으로 정규화할 필요가 없다(그 손 정규화는 별도 가드가 세는 자리다).
    let self_path = Path::new(SELF_PATH);
    let before = found.len();
    found.retain(|(path, _)| path.strip_prefix(root).unwrap_or(path) != self_path);
    assert!(
        before > found.len(),
        "자기 면제가 0 건이다 — `SELF_PATH`({SELF_PATH})가 이 파일의 실제 자리와 어긋났다. \
         옮겼거나 이름이 바뀐 것이니 그 상수를 고쳐라."
    );

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
