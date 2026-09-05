//! ADR 번호가 **식별자로 성립하는지** 못 박는다.
//!
//! ADR 은 번호로 인용된다 — 소스 주석, 다른 ADR 의 References, `docs/` 본문, 커밋
//! 메시지가 전부 "ADR-0140" 같은 형태로 가리킨다. 그래서 한 번호가 두 문서를 가리키는
//! 순간 그 인용들이 전부 모호해진다. 그런데 번호를 **여러 lane 이 병렬로 집는다** —
//! 각자 자기 브랜치에서 "다음 빈 번호" 를 보고 고르므로, 둘이 같은 것을 보면 둘 다
//! 옳게 골랐는데 충돌한다. 병합은 서로 다른 줄을 더한 것이라 conflict 없이 통과한다.
//!
//! 실제로 그렇게 됐다(2026-09-05): `0149` 가 서로 다른 두 ADR 에 붙은 채 main 에
//! 얹혔고, 기존 문서 가드 어느 것도 빨개지지 않았다. 이 파일은 그 자리를 메운다.
//!
//! 네 가지를 본다 — **번호 유일성**, 파일↔인덱스 **양방향** 대응, 그리고 문서 안의
//! `# ADR-NNNN` 제목이 자기 파일명과 같은지. 마지막 것은 번호를 옮길 때 파일명만 바꾸고
//! 본문 제목을 안 고치는 형태를 잡는다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

const ADR_DIR: &str = "docs/adr";
const INDEX: &str = "docs/adr/index.md";

/// ADR 수의 하한 — **연기 검사**다. 목록이 비면 아래 집합 대조는 빈 집합끼리라
/// 그냥 통과한다. 값의 근거: 2026-09-05 실측 153 건.
const MIN_ADRS: usize = 120;

fn repo_root() -> PathBuf {
    // 이 크레이트는 `crates/tasty-doc-guards` 다 — 레포 루트는 두 단계 위.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/<crate> 아래여야 한다")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let p = repo_root().join(rel);
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
        .replace("\r\n", "\n")
}

/// 앞 네 자리가 숫자인 `.md` 만 ADR 로 센다 — `index.md` · `template.md` 는 빠진다.
fn adr_files() -> BTreeMap<String, String> {
    let dir = repo_root().join(ADR_DIR);
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).expect("docs/adr 를 읽을 수 없다") {
        let name = entry.expect("디렉터리 항목").file_name();
        let name = name.to_string_lossy().to_string();
        if !name.ends_with(".md") {
            continue;
        }
        let num: String = name.chars().take(4).collect();
        if num.len() == 4 && num.chars().all(|c| c.is_ascii_digit()) {
            let prev = out.insert(num.clone(), name.clone());
            assert!(
                prev.is_none(),
                "파일 두 개가 같은 번호를 쓴다: {num} — {prev:?} 와 {name}"
            );
        }
    }
    out
}

/// 인덱스 표의 `| NNNN | [제목](파일명) | …` 행에서 (번호, 파일명) 을 뽑는다.
fn index_rows() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in read(INDEX).lines() {
        let Some(rest) = line.strip_prefix("| ") else {
            continue;
        };
        let num: String = rest.chars().take(4).collect();
        if num.len() != 4 || !num.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        // 링크 대상은 `](` 와 `)` 사이.
        let Some(at) = rest.find("](") else { continue };
        let after = &rest[at + 2..];
        let Some(end) = after.find(')') else { continue };
        out.push((num, after[..end].to_string()));
    }
    out
}

/// 한 번호는 한 ADR 만 가리킨다.
#[test]
fn an_adr_number_names_exactly_one_document() {
    let rows = index_rows();
    assert!(
        rows.len() >= MIN_ADRS,
        "인덱스에서 ADR 행을 {} 개밖에 못 뽑았다(하한 {MIN_ADRS}, 2026-09-05 실측 153). \
         행 형태가 바뀌었으면 이 추출기를 고쳐라 — 지금은 대조군이 죽은 상태다",
        rows.len()
    );

    let mut seen: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (num, file) in &rows {
        seen.entry(num).or_default().push(file);
    }
    let dupes: Vec<String> = seen
        .iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|(num, files)| format!("{num} → {}", files.join(" / ")))
        .collect();
    assert!(
        dupes.is_empty(),
        "같은 ADR 번호가 서로 다른 문서를 가리킨다. 번호는 소스 주석·다른 ADR·커밋 \
         메시지가 인용하는 식별자라, 겹치면 그 인용이 전부 모호해진다. 나중에 얹은 쪽이 \
         다음 빈 번호로 옮긴다(파일명·본문 제목·인덱스 행·참조 링크 넷 다).\n  {}",
        dupes.join("\n  ")
    );
}

/// 파일과 인덱스 행이 **양방향으로** 대응한다.
#[test]
fn every_adr_file_has_a_row_and_every_row_has_a_file() {
    let files = adr_files();
    assert!(
        files.len() >= MIN_ADRS,
        "ADR 파일이 {} 개뿐이다(하한 {MIN_ADRS})",
        files.len()
    );
    let rows = index_rows();
    let row_files: BTreeSet<&str> = rows.iter().map(|(_, f)| f.as_str()).collect();
    let disk: BTreeSet<&str> = files.values().map(|f| f.as_str()).collect();

    let missing_row: Vec<&&str> = disk.difference(&row_files).collect();
    assert!(
        missing_row.is_empty(),
        "ADR 파일은 있는데 인덱스에 행이 없다 — 인덱스가 카탈로그 구실을 못 한다: {missing_row:?}"
    );
    let missing_file: Vec<&&str> = row_files.difference(&disk).collect();
    assert!(
        missing_file.is_empty(),
        "인덱스 행이 없는 파일을 가리킨다 — 링크가 죽었다: {missing_file:?}"
    );
}

/// 문서 안의 `# ADR-NNNN` 제목이 자기 파일명 번호와 같다.
///
/// 번호를 옮길 때 파일명만 바꾸고 본문을 안 고치면, 문서를 **열어서** 번호를 읽은
/// 사람만 틀린 값을 갖게 된다. 파일 목록만 보는 판정으로는 안 잡힌다.
#[test]
fn the_heading_number_matches_the_file_name() {
    let mut wrong = Vec::new();
    for (num, name) in adr_files() {
        let body = read(&format!("{ADR_DIR}/{name}"));
        let Some(head) = body.lines().find(|l| l.starts_with("# ADR-")) else {
            wrong.push(format!("{name} — `# ADR-…` 제목 줄이 없다"));
            continue;
        };
        let in_head: String = head.trim_start_matches("# ADR-").chars().take(4).collect();
        if in_head != num {
            wrong.push(format!("{name} — 제목은 ADR-{in_head} 라고 말한다"));
        }
    }
    assert!(
        wrong.is_empty(),
        "파일명 번호와 본문 제목 번호가 다르다:\n  {}",
        wrong.join("\n  ")
    );
}
