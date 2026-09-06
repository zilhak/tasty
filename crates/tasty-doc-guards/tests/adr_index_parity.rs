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
use std::path::{Path, PathBuf};

const ADR_DIR: &str = "docs/adr";
const INDEX: &str = "docs/adr/index.md";

/// ADR 수의 하한 — **연기 검사**다. 목록이 비면 아래 집합 대조는 빈 집합끼리라
/// 그냥 통과한다. 값의 근거: 2026-09-05 실측 153 건.
///
/// **판별식** — 이 상수 하나가 **세 시험의 네 수**를 지킨다. 그 넷은 서로 독립으로 재는데
/// **정상이면 전부 같은 값**이다. 그래서 넷을 나란히 읽는 것이 곧 이 하한의 검사다:
///
/// ```text
/// cargo test -p tasty-doc-guards --test adr_index_parity -- --nocapture
///   → [ADR 인덱스] ADR 파일 <N> · 하한 120        (every_adr_file_has_a_row…)
///   → [ADR 인덱스] 인덱스 행 <N> · 하한 120        (an_adr_number_names_exactly_one_document)
///   → [adr-index-parity] 행 <N> · Status 대조 <N> · Date 대조 <N> · …
///                                                 (an_index_row_carries_the_same_status…)
/// ```
///
/// ★ **넷이 갈리면 하한이 아니라 독법이 고장 난 것이다.** 파일 수와 인덱스 행 수가 다르면
/// 인덱스가 밀린 것이고, 그 둘이 같은데 Status/Date 대조 수만 낮으면 헤더 독법이 죽은 것이다.
/// 그 구분은 이 하한이 못 한다 — 하한은 "넷 다 0 은 아니다" 까지만 말한다.
///
/// 실측 2026-09-07(`de0572359`): **네 수가 전부 190** 이다(09-05 의 153 에서 늘었다).
/// 하한이 120 이라 **여유가 70** 이다 — 술어가 3 분의 1 만 남아도 통과한다는 뜻이다.
/// 값을 올릴지는 하한 조이기라는 별개 축이라 여기서는 실측만 남긴다.
///
/// **이 수를 내려서 초록을 만들지 마라.** 이 자리의 하한은 대조군이 살아 있는지만 보는
/// 연기 검사라, 내리면 아래 집합 대조들이 더 작은 집합에서만 참이 되면서 색은 안 변한다.
///
/// 정당한 수선: ADR 을 실제로 지웠으면 이 수를 함께 내려라. 그때 **위 네 수를 함께 봐라** —
/// 넷이 같이 줄었으면 지운 것이고, 하나만 줄었으면 지운 것이 아니라 독법이 깨진 것이다.
const MIN_ADRS: usize = 120;

/// 본문이 `Superseded by NNNN` 인 ADR 수의 하한 — **연기 검사**다.
///
/// [`tolerated_too_much`] 의 "행이 대체 ADR 번호를 잃었다" 갈래는 본문에 그 형태가
/// 있어야만 밟힌다. 하나도 없으면 그 갈래는 한 번도 안 돌고, 그때의 초록은 "위반이
/// 없다" 가 아니라 **"볼 것이 없었다"** 다 — 두 초록은 값이 같고 뜻이 다르다.
///
/// 값의 근거: 2026-09-06 실측 5 건(0015 · 0018 · 0040 · 0042 · 0066).
/// 나머지 갈래인 "행의 상태 칸이 비었다" 는 실물이 0 이라 하한을 둘 수 없다 —
/// 그쪽은 변이 팔로만 확인된다.
const MIN_SUPERSEDED: usize = 3;

fn repo_root() -> PathBuf {
    // 이 크레이트는 `crates/tasty-doc-guards` 다 — 레포 루트는 두 단계 위.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/<crate> 아래여야 한다")
        .to_path_buf()
}

fn read(root: &Path, rel: &str) -> String {
    let p = root.join(rel);
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
        .replace("\r\n", "\n")
}

/// 앞 네 자리가 숫자인 `.md` 만 ADR 로 센다 — `index.md` · `template.md` 는 빠진다.
fn adr_files(root: &Path) -> BTreeMap<String, String> {
    let dir = root.join(ADR_DIR);
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
fn index_rows(root: &Path) -> Vec<IndexRow> {
    let mut out = Vec::new();
    for line in read(root, INDEX).lines() {
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

/// 접두 일치가 **관용해서는 안 되는** 두 형태.
///
/// 행은 본문 `Status` 의 앞부분만 실을 수 있다 — 본문은 ` — ` 뒤에 사유를 붙이고 행은
/// 그것을 버린다. 그런데 그 절단 지점이 관례로 정해져 있지 않다(실측 2026-09-06):
/// 0015 는 `(부분)` 을 빼고 0040 은 넣으며, 0027 은 괄호절 전체를 뺀다. 그래서 동일성
/// 비교로 바꾸면 그 셋이 오탐이 된다 — 정규형이 실제로 없는 것을 있다고 가정하는 셈이다.
///
/// 관용은 두되, **잃으면 안 되는 것**만 뺀다.
///
/// - 행이 비면 `starts_with("")` 이 항상 참이라 그 행은 무슨 값이든 통과한다. 빈 칸은
///   읽는 사람에게 "상태 없음" 이라 통과시킬 값이 아니다.
/// - 본문이 다른 ADR 을 가리키는데(`Superseded by NNNN`) 행이 그 번호를 잃으면, 인덱스만
///   읽는 사람은 **어디로 갔는지 모른 채** 죽은 결정을 본다. 상태 이름만 남기는 것이
///   정확히 그 형태이고 접두 일치는 그것을 통과시킨다. 이 가드가 막으려는 사고가 바로
///   그것이므로(0042 는 그 반대 방향이었다 — 행이 `Accepted` 로 남았다) 여기서 뺀다.
///
/// 이 두 형태의 실물은 오늘 0 이다(빈 칸 0 · 맨 `Superseded` 0). 모수는 실행마다
/// 바뀌므로 여기 안 적는다 — 스캔 테스트가 `-- --nocapture` 로 그 줄을 싣는다. 0 인 것과
/// 막는 것이 있는 것은 다르다 — 이 함수가 그 차이다.
fn tolerated_too_much(want: &str, got: &str) -> Option<&'static str> {
    if got.trim().is_empty() {
        return Some("행의 상태 칸이 비었다");
    }
    // 상태 **이름 전체**를 실어야 한다. 접두만 요구하면 본문 `Accepted` 에 행 `A` 가
    // 통과한다 — "칸을 채워라" 를 최소로 이행한 결과가 정확히 그것이고, 그러면 이
    // 검사가 지키려던 것이 남지 않는다. 뒤쪽(사유·괄호절)은 여전히 잘라도 된다.
    if first_word(want) != first_word(got) {
        return Some("행이 상태 이름의 일부만 싣고 있다");
    }
    let num = superseder(want)?;
    (!got.contains(&num)).then_some("행이 대체한 ADR 번호를 잃었다")
}

/// 공백 앞까지. 값이 비면 빈 문자열.
fn first_word(v: &str) -> &str {
    v.split_whitespace().next().unwrap_or("")
}

/// `Superseded by 0162 …` 에서 `0162`. [`normalize_header_value`] 를 거친 값이라
/// 링크와 `ADR-` 는 이미 벗겨져 있다.
fn superseder(want: &str) -> Option<String> {
    let rest = want.strip_prefix("Superseded by ")?;
    let num: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (num.len() == 4).then_some(num)
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
    let rows = index_rows(&repo_root());
    // R445 — 측정값은 단정보다 앞에. 이 파일의 세 시험이 같은 하한을 쓰고 서로 다른 수를
    // 재므로, 그 수들을 나란히 읽는 것이 곧 하한의 판별식이다(상수 doc 참조).
    println!("[ADR 인덱스] 인덱스 행 {} · 하한 {MIN_ADRS}", rows.len());
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
    let root = repo_root();
    let files = adr_files(&root);
    println!("[ADR 인덱스] ADR 파일 {} · 하한 {MIN_ADRS}", files.len());
    assert!(
        files.len() >= MIN_ADRS,
        "ADR 파일이 {} 개뿐이다(하한 {MIN_ADRS})",
        files.len()
    );
    let rows = index_rows(&root);
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
    let root = repo_root();
    let mut wrong = Vec::new();
    for (num, name) in adr_files(&root) {
        let body = read(&root, &format!("{ADR_DIR}/{name}"));
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
/// 판정 술어를 **직접** 부르는 대조. 위의 스캔 테스트만으로는 이 갈래들이 지켜지지
/// 않는다 — 실측 2026-09-06: `tolerated_too_much` 를 항상 `None` 으로 바꿔도 스캔은
/// 초록이었다(rc=0 · F=0). 코퍼스(`docs/adr/`)를 흔들면 갈래가 밟히지만 그 변이는
/// 원복하면 사라져 회귀에 안 남는다. 그래서 갈래마다 여기서 한 번 더 묻는다.
///
/// 같은 크레이트의 `temp_path` 는 이 형태를 12 개 갖고 있고, 그래서 그쪽은 판정을
/// 느슨하게 바꾸면 유닛이 빨개진다. 이 파일에는 0 이었다 — 그 차이가 구멍이었다.
#[test]
fn the_status_verdicts_each_have_their_own_reason() {
    // 빈 칸 — 최소 이행이 "아무거나 채운다" 가 되면 안 된다.
    assert_eq!(
        tolerated_too_much("Accepted", ""),
        Some("행의 상태 칸이 비었다")
    );
    assert_eq!(
        tolerated_too_much("Accepted", "   "),
        Some("행의 상태 칸이 비었다")
    );

    // 한 글자 — 접두이긴 하지만 상태 **이름**이 아니다. 빈 칸 처방의 최소 이행이
    // 정확히 이 형태였다.
    assert_eq!(
        tolerated_too_much("Accepted", "A"),
        Some("행이 상태 이름의 일부만 싣고 있다")
    );
    assert_eq!(
        tolerated_too_much("Superseded by 0162", "Sup"),
        Some("행이 상태 이름의 일부만 싣고 있다")
    );

    // 이름은 맞는데 대체한 번호를 잃었다 — 인덱스만 읽는 사람이 어디로 갔는지 모른다.
    assert_eq!(
        tolerated_too_much("Superseded by 0162", "Superseded"),
        Some("행이 대체한 ADR 번호를 잃었다")
    );

    // 뒤쪽을 자르는 것은 허용이다. 이름이 같고 번호가 남아 있으면 통과한다.
    assert_eq!(tolerated_too_much("Accepted", "Accepted"), None);
    assert_eq!(tolerated_too_much("Accepted (부분 적용)", "Accepted"), None);
    assert_eq!(
        tolerated_too_much("Superseded by 0162 — 사유", "Superseded by 0162"),
        None
    );
}

/// `superseder` 는 네 자리 번호만 인정한다. 이 갈래가 죽으면 위의 "번호를 잃었다" 가
/// 영영 안 밟히고, `MIN_SUPERSEDED` 하한이 그것을 대신 잡아 주지 않는다 — 하한은
/// **본문에 그 형태가 몇 개 있나**를 볼 뿐 판정이 사는지는 안 본다.
#[test]
fn a_superseder_is_a_four_digit_number_or_nothing() {
    assert_eq!(superseder("Superseded by 0162"), Some("0162".to_string()));
    assert_eq!(
        superseder("Superseded by 0162 — 사유가 뒤에 붙는다"),
        Some("0162".to_string())
    );
    assert_eq!(superseder("Accepted"), None);
    assert_eq!(superseder("Superseded by 162"), None); // 세 자리는 아니다
    assert_eq!(superseder("superseded by 0162"), None); // 대소문자가 다르면 아니다
}

/// 첫 낱말은 공백 앞까지다. 값이 비면 빈 문자열이고, 그때는 빈 칸 갈래가 먼저 잡는다.
#[test]
fn the_first_word_stops_at_whitespace() {
    assert_eq!(first_word("Accepted"), "Accepted");
    assert_eq!(first_word("Superseded by 0162"), "Superseded");
    assert_eq!(first_word("  Accepted  (부분)"), "Accepted");
    assert_eq!(first_word(""), "");
    assert_eq!(first_word("   "), "");
}

#[test]
fn an_index_row_carries_the_same_status_and_date_as_its_adr() {
    let root = repo_root();
    let rows = index_rows(&root);
    let files = adr_files(&root);
    let mut checked_status = 0usize;
    let mut checked_date = 0usize;
    let mut superseded_seen = 0usize;
    let mut prefix_ok = 0usize;
    let mut drift: Vec<String> = Vec::new();

    for row in &rows {
        let Some(name) = files.get(&row.num) else {
            // 파일 없는 행은 `every_adr_file_has_a_row_and_every_row_has_a_file` 이 잡는다.
            continue;
        };
        let body = read(&root, &format!("{ADR_DIR}/{name}"));

        if let Some(v) = header_field(&body, "Status") {
            checked_status += 1;
            let want = normalize_header_value(&v);
            let got = normalize_header_value(&row.status);
            if superseder(&want).is_some() {
                superseded_seen += 1;
            }
            let verdict = if want.starts_with(&got) {
                prefix_ok += 1;
                tolerated_too_much(&want, &got)
            } else {
                Some("행이 본문의 접두가 아니다")
            };
            if let Some(why) = verdict {
                drift.push(format!(
                    "{} Status — {why}: 본문 {:?} · 행 {:?}",
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

    // ★ 아래 하한들은 **하한이지 모수가 아니다.** "표류 0" 이 안 봐서 0 인지 정말
    // 없어서 0 인지는 그 초록만으로 안 갈린다(R473 형태). 그래서 모수를 여기서 싣는다.
    // libtest 는 통과한 테스트의 출력을 삼키므로 `-- --nocapture` 로 읽는다.
    // `tracing` 은 여기서 못 쓴다 — 이 크레이트는 의존이 0 인 것이 존재 이유라
    // (ADR-0138) subscriber 자체가 없다.
    // 단정보다 **앞**에 둔다: 빨간 경로에서도 모수가 남아야 한다.
    eprintln!(
        "[adr-index-parity] 행 {} · Status 대조 {checked_status} · Date 대조 {checked_date} \
         · 접두 통과 {prefix_ok} · 대체 형태 {superseded_seen} · 표류 {}",
        rows.len(),
        drift.len()
    );

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
        superseded_seen >= MIN_SUPERSEDED,
        "본문이 다른 ADR 을 가리키는 행을 {superseded_seen} 건밖에 못 봤다 \
         (하한 {MIN_SUPERSEDED}) — 그 형태가 없으면 '행이 대체 번호를 잃었다' \
         갈래가 한 번도 안 밟히고, 그때의 초록은 위반이 없다는 뜻이 아니다. \
         ★ 이 하한을 내려서 통과시키지 마라 — 내리면 그 갈래가 밟히는지를 아무도 \
         안 지킨다. 본문에서 그 형태가 정말 사라졌으면 갈래도 함께 지워라"
    );

    assert!(
        drift.is_empty(),
        "인덱스 행이 본문 헤더와 다른 값을 싣고 있다 {} 건. \
         ★ 본문이 정본이다 — 행을 본문에 맞춰라. 본문을 행에 맞추지 마라: \
         그렇게 해도 초록은 되지만, 인덱스만 읽는 사람은 죽은 결정을 살아 있는 것으로 \
         읽고 그 결정이 어디로 갔는지까지 잃는다:\n  {}",
        drift.len(),
        drift.join("\n  ")
    );
}

/// `MIN_ADRS` 의 **양성 대조** — 모수가 하한 아래로 떨어지는 코퍼스를 만들고, 이 파일의
/// 두 수집기가 실제로 그것을 말하는지 묻는다.
///
/// 하한이 있다는 것과 그 하한이 옳다는 것은 다르다. 위 세 시험은 전부 **레포 자신**을
/// 읽으므로 수집이 죽는 상황이 여기서는 한 번도 안 만들어진다 — 그 초록은 "수집이
/// 산다" 가 아니라 "레포에 ADR 이 많다" 만 말한다.
///
/// 상수 doc 이 말하는 **네 수가 갈리는 형태**를 칸으로 만든다. 두 수집기가 독립이라
/// 한쪽만 죽는 것이 실제 사고 모양이고(인덱스가 밀림 / 디렉터리를 못 읽음), 한쪽만
/// 보는 대조는 그것을 못 가른다.
///
/// 비영 대조를 함께 세운다 — 3/3 을 세는 칸이 있어야 앞 칸의 0 이 **구조의 0**(코퍼스가
/// 비었다)이지 **술어의 0**(수집기가 입력을 안 본다)이 아님이 갈린다.
#[test]
fn the_adr_floor_sees_a_collapsed_collection() {
    let root =
        std::env::temp_dir().join(format!("tasty-adrfloor-{}-{}", std::process::id(), line!()));
    // 앞선 실행의 잔여를 치운다 — 없는 것이 정상이라 실패가 정보가 아니다.
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join(ADR_DIR);
    std::fs::create_dir_all(&dir).expect("임시 디렉토리를 못 만들었다");
    std::fs::write(root.join(INDEX), "").expect("쓰기 실패");

    // ① 둘 다 비었다 — 하한이 잡아야 하는 상태.
    assert_eq!(
        adr_files(&root).len(),
        0,
        "뿌리가 비었는데 0 이 아니면 이 수집기는 인자를 안 보고 레포를 읽는 것이다"
    );
    assert_eq!(
        index_rows(&root).len(),
        0,
        "인덱스가 비었는데 0 이 아니면 이 독법은 인자를 안 보고 레포를 읽는 것이다"
    );

    // ② 표 머리글은 행이 아니다. 이것을 세면 인덱스가 통째로 밀려도 수가 안 떨어진다.
    std::fs::write(
        root.join(INDEX),
        "| 번호 | 제목 | Status | Date | Tags |\n|---|---|---|---|---|\n",
    )
    .expect("쓰기 실패");
    assert_eq!(
        index_rows(&root).len(),
        0,
        "머리글·구분줄을 ADR 행으로 세면 빈 인덱스가 하한을 통과한다"
    );

    // ③ 파일만 있고 행이 없다 — 두 수가 **갈리는** 칸. 상수 doc 의 "넷이 갈리면 독법이
    //    고장 난 것" 이 실제로 갈리는지를 여기서 본다.
    for (num, slug) in [("0001", "a"), ("0002", "b"), ("0003", "c")] {
        std::fs::write(dir.join(format!("{num}-{slug}.md")), "").expect("쓰기 실패");
    }
    assert_eq!(
        adr_files(&root).len(),
        3,
        "디렉터리를 안 읽으면 파일 수가 0 에 머문다"
    );
    assert_eq!(
        index_rows(&root).len(),
        0,
        "행이 없는데 행 수가 늘면 두 수가 서로를 가려 준다 — 그러면 한쪽이 죽어도 하한이 안 걸린다"
    );

    // ④ 비영 대조 — 행을 넣으면 두 수가 같이 3 이 된다. 앞 칸들의 0 이 구조의 0 이었음이
    //    여기서 갈린다.
    let mut index = String::from("| 번호 | 제목 | Status | Date | Tags |\n|---|---|---|---|---|\n");
    for (num, slug) in [("0001", "a"), ("0002", "b"), ("0003", "c")] {
        index.push_str(&format!(
            "| {num} | [제목]({num}-{slug}.md) | Accepted | 2026-09-07 | tag |\n"
        ));
    }
    std::fs::write(root.join(INDEX), &index).expect("쓰기 실패");
    assert_eq!(adr_files(&root).len(), 3, "파일 수가 흔들리면 안 된다");
    assert_eq!(
        index_rows(&root).len(),
        3,
        "행이 셋인데 3 이 아니면 독법이 죽은 것이다 — 그때 앞 칸의 0 은 코퍼스가 아니라 독법 탓이다"
    );

    // ⑤ 반대 방향 — 번호가 아닌 `.md` 를 세면 모수가 부풀고, 부푼 만큼 하한이 무뎌진다.
    //    `index.md` 는 이미 이 디렉터리에 있고 `template.md` 를 하나 더 놓는다.
    std::fs::write(dir.join("template.md"), "").expect("쓰기 실패");
    assert_eq!(
        adr_files(&root).len(),
        3,
        "앞 네 자리가 숫자가 아닌 `.md` 를 ADR 로 세면 하한이 그만큼 헐거워진다"
    );

    // ⑥ 그리고 이 코퍼스는 하한 아래다 — 위 세 시험이 이 뿌리를 읽었다면 빨개진다.
    assert!(
        adr_files(&root).len() < MIN_ADRS && index_rows(&root).len() < MIN_ADRS,
        "이 대조가 하한 위에 있으면 '하한이 무너진 상태' 를 한 번도 안 만든 것이다"
    );

    // 뒷정리 실패는 무시한다 — 임시 디렉토리라 남아도 다음 실행이 먼저 지우고, 여기서
    // 죽으면 위 단정의 결과가 정리 오류에 가린다.
    let _ = std::fs::remove_dir_all(&root);
}
