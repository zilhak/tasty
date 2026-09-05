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
//!
//! ## 인덱스 행이 거울처럼 싣는 값
//!
//! 인덱스 행은 ADR 헤더의 `Status`·`Date` 를 **복사**한다. 값이 두 곳에 있고 함께
//! 움직여야 하는데, 움직이는 것은 대개 한 곳뿐이다 — 재sync 나 부분 개정에서 본문만
//! 올라가고 행은 첫 커밋 값으로 남는다. 실측(2026-09-06, 모수 179): `Date` 는 179/179
//! 가 같았고 `Status` 는 **한 건이 어긋나 있었다**(0042 — 본문 `Superseded by ADR-0162`,
//! 행 `Accepted`). 그 한 건은 이 가드를 켜는 커밋에서 함께 고쳤다.
//!
//! **`Title` 과 `Tags` 는 일부러 안 본다.** 같은 짝인데 열마다 관계가 다르다 — 본문
//! 제목은 강조 마커(`**…**`)를 쓰고 행은 안 쓰며, `Tags` 는 **행이 본문의 상위집합인
//! 경우가 12 건**이라 등호도 접두도 아니다. 정규화 없이 넣으면 오탐이 21 건이고,
//! 오탐이 그만큼이면 가드를 아무도 안 믿는다. 두 열의 정규화 규칙이 서면 그때 넣는다.

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

/// 인덱스 표의 한 행에서 뽑아낸 값들.
struct IndexRow {
    num: String,
    file: String,
    status: String,
    date: String,
}

/// 인덱스 표의 `| NNNN | [제목](파일명) | Status | Date | Tags |` 행을 읽는다.
///
/// 독법은 여기 하나다. 열이 더 필요해지면 이 함수를 넓히고, **두 번째 독법을 만들지
/// 않는다** — 같은 표를 두 방법으로 읽으면 답이 갈리고, 갈린 답 중 어느 것이 옳은지는
/// 표를 다시 읽어야 알게 된다.
fn index_rows() -> Vec<IndexRow> {
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
        let file = after[..end].to_string();
        // 링크를 닫는 `)` 뒤부터가 Status | Date | Tags 다. 제목 안에 `|` 가 들어갈 수
        // 있으므로 앞에서부터 세지 않고 **링크 뒤**에서 센다.
        let tail: Vec<&str> = after[end + 1..].split('|').collect();
        let cell = |i: usize| {
            tail.get(i)
                .map(|c| c.trim().to_string())
                .unwrap_or_default()
        };
        out.push(IndexRow {
            num,
            file,
            status: cell(1),
            date: cell(2),
        });
    }
    out
}

/// 인덱스 행이 본문 헤더의 값을 그대로 싣는지 볼 때 쓰는 정규화.
///
/// 본문은 같은 값을 인라인 링크로 적을 수 있다(`Superseded by [0032](0032-….md)`).
/// 행은 링크 없이 적는다. 그 차이는 표기이지 값이 아니므로 링크를 벗기고, `ADR-`
/// 접두와 잉여 공백도 지운다.
fn normalize_header_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else {
            out.push_str(&rest[open..]);
            return squeeze(&out);
        };
        out.push_str(&after[..close]);
        let tail = &after[close + 1..];
        rest = match tail.strip_prefix('(') {
            // 링크 대상은 통째로 버린다.
            Some(target) => match target.find(')') {
                Some(e) => &target[e + 1..],
                None => "",
            },
            None => tail,
        };
    }
    out.push_str(rest);
    squeeze(&out)
}

fn squeeze(s: &str) -> String {
    s.replace("ADR-", "")
        .replace('`', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// 본문 헤더의 `- **<이름>**: <값>` 한 줄을 읽는다.
fn header_field(body: &str, name: &str) -> Option<String> {
    let want = format!("- **{name}**:");
    body.lines()
        .find_map(|l| l.trim_start().strip_prefix(&want))
        .map(|v| v.trim().to_string())
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
    for IndexRow { num, file, .. } in &rows {
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
    let row_files: BTreeSet<&str> = rows.iter().map(|r| r.file.as_str()).collect();
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

/// 인덱스 행의 `Status`·`Date` 가 그 ADR 본문 헤더의 값과 같은지 본다.
///
/// 두 곳에 있고 함께 움직여야 하는 값인데, 재sync·부분 개정에서는 본문만 올라가고
/// 행이 첫 커밋 값으로 남는다. 그때 인덱스만 읽는 사람은 죽은 결정을 살아 있는 것으로
/// 읽는다 — 실제로 0042 가 그 상태였다(본문 Superseded, 행 Accepted).
///
/// 표기 차이는 값 차이가 아니다: 본문은 `Superseded by [0032](0032-….md)` 처럼 인라인
/// 링크를 쓸 수 있고 행은 안 쓴다. 그래서 링크를 벗기고 `ADR-` 접두를 지운 뒤 비교한다.
/// 그러고도 본문이 사유를 덧붙이는 경우가 있어(`Superseded by 0052 (부분) — …`)
/// **행이 본문의 접두인지**를 묻는다 — 행은 본문의 짧은 형태다.
#[test]
fn an_index_row_carries_the_same_status_and_date_as_its_adr() {
    let rows = index_rows();
    let files = adr_files();
    let mut checked_status = 0usize;
    let mut checked_date = 0usize;
    let mut drift: Vec<String> = Vec::new();

    for row in &rows {
        let Some(name) = files.get(&row.num) else {
            // 파일 없는 행은 `every_adr_file_has_a_row_and_every_row_has_a_file` 이 잡는다.
            continue;
        };
        let body = read(&format!("{ADR_DIR}/{name}"));

        if let Some(v) = header_field(&body, "Status") {
            checked_status += 1;
            let want = normalize_header_value(&v);
            let got = normalize_header_value(&row.status);
            if !want.starts_with(&got) {
                drift.push(format!(
                    "{} Status — 본문 {:?} · 행 {:?}",
                    row.num,
                    want.chars().take(60).collect::<String>(),
                    got
                ));
            }
        }
        if let Some(v) = header_field(&body, "Date") {
            checked_date += 1;
            let want = normalize_header_value(&v);
            let got = normalize_header_value(&row.date);
            if want != got {
                drift.push(format!("{} Date — 본문 {want:?} · 행 {got:?}", row.num));
            }
        }
    }

    // 0 을 통과로 만들지 않는다 — 열 하나가 안 읽히면 그 갈래는 조용히 빈다.
    // 두 열을 따로 센다: 한 열의 독법이 죽어도 다른 열의 수가 그것을 안 가린다.
    assert!(
        checked_status >= MIN_ADRS,
        "Status 를 {checked_status} 건밖에 못 읽었다(하한 {MIN_ADRS}) — \
         행이나 헤더 독법이 죽었으면 아래 초록은 거짓이다"
    );
    assert!(
        checked_date >= MIN_ADRS,
        "Date 를 {checked_date} 건밖에 못 읽었다(하한 {MIN_ADRS}) — \
         행이나 헤더 독법이 죽었으면 아래 초록은 거짓이다"
    );

    assert!(
        drift.is_empty(),
        "인덱스 행이 본문 헤더와 다른 값을 싣고 있다 {} 건. 본문을 고쳤으면 행도 \
         같이 내려라 — 인덱스만 읽는 사람은 죽은 결정을 살아 있는 것으로 읽는다:\n  {}",
        drift.len(),
        drift.join("\n  ")
    );
}
