//! ANSI escape 제거 정규식이 tasty-ansi에만 있는지 확인한다.
//! src와 crates에서 대상 리터럴의 위치·개수를 비교해 소비자가 별도 사본을 만들지 못하게 한다.
//! 정규식의 모든 동등한 표현을 찾는 검사는 아니며 아래 표기 형식을 기준으로 검색한다.
use std::path::{Path, PathBuf};
use tasty_doc_guards::temp_scratch::Scratch;

const EXPECTED: [&str; 1] = ["crates/tasty-ansi/src/lib.rs"];

/// 검출용 정규식 조각을 가진 이 파일은 제외한다.
const SELF_PATH: &str = "crates/tasty-doc-guards/tests/strip_ansi_regex_parity.rs";

/// 한 줄의 r 따옴표 문자열 하나를 읽는다. 내용에 따옴표가 없다고 가정하며 const 선언도 대상이다.
fn extract_literal(line: &str) -> Option<String> {
    let start = line.find("r\"")? + 2;
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// 한 루트의 수집 실패를 다른 루트가 가리지 않도록 각각 하한을 둔다.
/// 정규식은 한 위치에만 있어야 하므로 발견 건수가 아니라 훑은 Rust 파일 수를 센다.
/// 2026-09-07 측정 crates664/src598에서 파일 정리를 허용한 하한 550/500이다.
const MIN_SCANNED: [(&str, usize); 2] = [("crates", 550), ("src", 500)];

/// 정규식 리터럴을 모으고 하한 검사에 쓸 Rust 파일 수를 반환한다.
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
    let root_buf = tasty_doc_guards::repo_root();
    let root = root_buf.as_path();
    let mut found = Vec::new();
    for (name, floor) in MIN_SCANNED {
        let scanned = collect(&root.join(name), &mut found);
        println!("[ANSI 정규식 한 집] 뿌리 {name} 아래 .rs {scanned} · 하한 {floor}");
        assert!(
            scanned >= floor,
            "{name}에서 Rust 파일을 {scanned}개만 읽었다(하한 {floor}). collect는 읽기 실패를 건너뛰므로 경로와 실제 파일 감소를 대조한다. 검사 누락을 하한 변경으로 숨기지 않는다."
        );
    }
    found.sort();

    // 검출 패턴을 포함한 검사 파일 자신을 제외한다. 제외가 실제로 적용됐는지도 확인한다.
    // 경로 비교는 플랫폼 구분자를 고려하는 Path의 동등성을 사용한다.
    let self_path = Path::new(SELF_PATH);
    let before = found.len();
    found.retain(|(path, _)| path.strip_prefix(root).unwrap_or(path) != self_path);
    assert!(
        before > found.len(),
        "자기 면제가 0 건이다 — `SELF_PATH`({SELF_PATH})가 이 파일의 실제 자리와 어긋났다. \
         옮겼거나 이름이 바뀐 것이니 그 상수를 고쳐라."
    );

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
        "ANSI escape 정규식 위치가 다르다: {paths:?}. 별도 사본 대신 tasty_ansi::strip_ansi를 사용한다. 의도한 이동이라면 EXPECTED를 갱신한다."
    );

    let first = &found[0].1;
    assert!(
        first.starts_with(r"\x1b\["),
        "리터럴 추출이 어긋났다(정규식 본문이 아님): {first:?}"
    );

    // 현재는 사본이 하나여서 이 루프가 돌지 않는다. EXPECTED를 늘리면 값의 일치도 확인한다.
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

/// 하위 Rust 파일만 수집하고 없는 경로는 0개로 반환하는지 합성 트리에서 확인한다.
#[test]
fn a_dead_root_scans_zero_and_only_rs_files_count() {
    let probe = Scratch::new("ansi-parity");
    let dir = probe.path();
    std::fs::create_dir_all(dir.join("nested")).expect("합성 트리를 만들지 못했다");

    // 검출 표지를 조립해 합성 입력 자체가 추가 사본으로 세어지지 않게 한다.
    let needle = format!("{}{}", r"\x1b", r"\[");
    std::fs::write(
        dir.join("alpha.rs"),
        format!("const P: &str = r\"{needle}0-9;m\";\n"),
    )
    .expect("합성 소스를 쓰지 못했다");
    std::fs::write(dir.join("nested").join("beta.rs"), "fn f() {}\n")
        .expect("합성 소스를 쓰지 못했다");
    std::fs::write(dir.join("gamma.txt"), format!("r\"{needle}\"\n"))
        .expect("합성 소스를 쓰지 못했다");

    let mut found = Vec::new();
    let scanned = collect(dir, &mut found);
    assert_eq!(
        scanned, 2,
        "합성 트리에서 하위 디렉터리를 포함한 Rust 파일 2개를 수집해야 한다"
    );
    assert_eq!(
        found.len(),
        1,
        "합성 트리의 정규식 리터럴을 찾지 못했다: {found:?}"
    );

    let mut none = Vec::new();
    assert_eq!(
        collect(&dir.join("does-not-exist"), &mut none),
        0,
        "없는 경로의 수집 결과가 0개가 아니다"
    );
    assert!(
        none.is_empty(),
        "없는 디렉터리에서 사용처를 수집했다: {none:?}"
    );
}
