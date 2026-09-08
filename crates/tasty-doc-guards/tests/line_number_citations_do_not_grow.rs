//! 코드를 `경로:숫자` 로 가리키는 인용이 **늘지 않는지** 본다 (ADR-0194 3 단계).
//!
//! ## 왜 수를 세나 — 정합을 못 보기 때문이다
//!
//! 줄 번호가 맞는지는 이 시험이 묻지 않는다. 물을 수가 없다: 인용의 절반 이상이 무엇을
//! 가리키는지 적지 않아 대조할 대상이 없다. 실측(2026-09-07)으로 지금 정확히 맞는 인용은
//! 100 여 건 중 두 건뿐이었고, 그 둘 중 하나는 같은 날 다른 커밋이 파일에서 220 줄을
//! 들어내며 **우연히 맞게 만든 것**이었다. 좌표는 절대 위치라 파일 크기 변화에 양방향으로
//! 노출되고, 그래서 "지금 맞는다" 는 관측이 그 좌표가 지켜진다는 증거가 되지 못한다.
//!
//! 그래서 정합 대신 **수**를 본다. 새 인용이 심볼 이름으로 적히면 이 수는 안 늘고,
//! 줄 번호로 적히면 는다. 그것이 이 시험이 답할 수 있는 물음이다.
//!
//! ## 세지 않는 것
//!
//! - **ADR 의 `Context` · `Decision` 절** — 그 절은 결정 *전*의 세계를 서술하므로, 가리키던
//!   것이 사라지는 것은 결함이 아니라 그 결정이 실행됐다는 증거다. 고치라고 요구하면
//!   결정 시점의 기록을 지우게 된다. ADR 의 그 밖 절은 기본값이 현재 포인터라 센다.
//! - **코드펜스 안과 진단 출력** — `--> src/main.rs:10:5` 같은 것은 인용이 아니라 그 텍스트의
//!   내용 자체다. 손대면 문서가 거짓이 된다.
//! - **파일 머리 구역 범위**(`:1-N`) — 심볼이 아니라 *구역*을 가리키는 정당한 형태이고,
//!   그 구역은 파일이 자라도 머리에 남아 이동에 강하다. 실측에서 이 형태 둘은 판정해 보니
//!   **둘 다 맞는 인용**이었고, 중간 범위(`:256-258` 등)는 전부 틀렸다. 그래서 시작이 1 인
//!   범위만 뺀다.
//! - **이 파일 자신** — 위 형태들을 설명하려면 형태를 적어야 한다. 자기를 세면 설명이
//!   늘 때마다 값이 움직인다.
//!
//! ## 왜 여유에 띠가 있나 — 다른 래칫과 성질이 다르다
//!
//! `check-allow-reason` 과 `check-shared-walk-ratchet` 은 여유 0 양방향이고, 그 근거는
//! "남는 여유가 곧 안 보는 구간이다" 다. 그 모수들은 정당한 감소가 드물어 감소를 사건으로
//! 다뤄도 마찰이 없다.
//!
//! 이 모수는 반대다 — **감소가 목표다.** 인용을 심볼로 바꾸는 정리가 곧 이 수를 줄인다.
//! 여유를 0 으로 두면 정리 커밋마다 이 상수를 함께 고쳐야 하고, 그 마찰이 정리를 미루게
//! 만든다. 그렇다고 한 방향으로만 두면 상한이 실제값보다 커진 채 남아 **그만큼 조용히 새
//! 위반을 받아준다.**
//!
//! 띠가 그 둘을 가른다. 띠는 임의값이 아니라 **한 파일이 담는 인용의 최대치**(실측
//! 2026-09-07 기준 15)다 — 문서 하나를 통째로 정리하는 것이 이 축의 자연스러운 작업 단위이고,
//! 띠가 그보다 좁으면 정상적인 정리 하나가 상수 편집을 강제한다.

use std::path::{Path, PathBuf};
use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, normalized_rel, walk_with_floor};

/// 실측 2026-09-07 의 값. 늘면 실패하고, 이 값보다 [`BAND`] 넘게 줄어도 실패한다
/// (그때는 상한을 내리라는 뜻이다 — 모듈 doc "왜 여유에 띠가 있나" 참조).
const CAP: usize = 60;

/// 상한과 실제값 사이에 허용하는 폭. 한 파일이 담는 인용의 최대치에서 왔다.
const BAND: usize = 15;

/// 순회 하나가 하나의 하한을 갖는다. 넷을 한 값으로 묶으면 가장 작은 뿌리(`scripts/`)에
/// 맞춰야 하고, 그러면 가장 큰 뿌리(`crates/`)가 통째로 죽어도 통과한다.
const WHY_GAP: &str = "이 모수는 그 뿌리 아래의 파일 수다. 크레이트·문서의 분리와 통폐합은                        정상 변경이라 하한을 실측에 붙이면 그 정리가 순회 사망으로 잘못                        진단된다. 여기서 하한은 순회 생존만 본다.";

/// `(뿌리, 하한, 실측)` — 실측은 2026-09-07 값이고 하한은 그 1/3 언저리다.
const ROOTS: &[(&str, usize, usize)] = &[
    ("docs", 120, 386),
    ("src", 200, 598),
    ("crates", 200, 650),
    ("tests", 20, 59),
    ("scripts", 8, 23),
];

fn floor_for(min: usize, measured: usize) -> Floor {
    Floor {
        min,
        measured,
        measured_on: "2026-09-07",
        counted_on: tasty_doc_guards::floored_walk::CountedOn::SyntheticTree,
        why_this_gap: WHY_GAP,
    }
}

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/<name> 아래이므로 조상 둘이 레포 루트다")
}

/// 이 파일 자신 — 형태를 설명하느라 형태를 담는다.
fn is_self(rel: &str) -> bool {
    rel.ends_with("tests/line_number_citations_do_not_grow.rs")
}

/// `<경로 또는 파일명>.rs:<숫자>[-<숫자>]` 를 한 줄에서 모은다.
///
/// 정규식 크레이트를 끌어오지 않는다 — 이 판정은 `.rs:` 를 찾고 그 앞뒤를 훑는 것으로
/// 끝나고, 의존 하나가 이 시험의 실행 조건이 되는 편이 더 비싸다.
fn citations(line: &str) -> Vec<(usize, bool)> {
    let bytes: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        if bytes[i] == '.' && bytes[i + 1..].starts_with(&['r', 's', ':']) {
            // 앞쪽이 파일명 글자인가 — `.rs` 앞에 이름이 있어야 인용이다.
            let named = i > 0 && (bytes[i - 1].is_alphanumeric() || bytes[i - 1] == '_');
            let mut j = i + 4;
            let mut start = 0usize;
            let mut digits = 0;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                start = start * 10 + bytes[j].to_digit(10).expect("ascii digit") as usize;
                digits += 1;
                j += 1;
            }
            if named && digits > 0 {
                let ranged = j < bytes.len()
                    && bytes[j] == '-'
                    && bytes.get(j + 1).is_some_and(char::is_ascii_digit);
                out.push((start, ranged));
            }
            i = j.max(i + 4);
            continue;
        }
        i += 1;
    }
    out
}

/// 한 파일에서 세야 할 인용 수. 마크다운이면 절과 코드펜스를 보고, 소스면 주석 줄만 본다.
fn count_in(rel: &str, text: &str) -> usize {
    let markdown = rel.ends_with(".md");
    let adr = rel.starts_with("docs/adr/")
        && rel
            .rsplit('/')
            .next()
            .is_some_and(|f| f.len() > 4 && f[..4].chars().all(|c| c.is_ascii_digit()));
    let mut section = String::new();
    let mut fenced = false;
    let mut n = 0;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if markdown {
            if trimmed.starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if let Some(h) = line.strip_prefix("## ") {
                section = h.trim().to_string();
            }
            if fenced || line.contains("-->") {
                continue;
            }
        } else if !(trimmed.starts_with("//") || trimmed.starts_with('#')) {
            continue;
        }
        if markdown && adr && (section == "Context" || section == "Decision") {
            continue;
        }
        n += citations(line)
            .into_iter()
            .filter(|&(start, ranged)| !(ranged && start == 1))
            .count();
    }
    n
}

fn walk(dir: &str, floor: &Floor, keep: &dyn Fn(&Walked) -> bool) -> Vec<Walked> {
    let root = root();
    walk_with_floor(&root.join(dir), root, floor, Descend::SkipBuildCaches, keep)
        .unwrap_or_else(|why| panic!("{why}"))
}

fn population() -> Vec<(String, PathBuf)> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for &(dir, min, measured) in ROOTS {
        let floor = floor_for(min, measured);
        let want_md = dir == "docs";
        files.extend(
            walk(dir, &floor, &|w: &Walked| {
                let ext_ok = if want_md {
                    w.rel.ends_with(".md")
                } else {
                    w.rel.ends_with(".rs") || w.rel.ends_with(".sh")
                };
                ext_ok && !is_self(&w.rel)
            })
            .into_iter()
            .map(|w| (w.rel, w.path)),
        );
    }

    // 루트 바로 아래 마크다운(README 등)도 추적 문서다.
    for name in ["README.md", "README.ko.md", "CLAUDE.md", "CHANGELOG.md"] {
        let p = root().join(name);
        if p.is_file() {
            files.push((normalized_rel(&p, root()), p));
        }
    }
    files
}

#[test]
fn line_number_citations_do_not_grow() {
    let mut total = 0usize;
    let mut per_file: Vec<(String, usize)> = Vec::new();
    for (rel, path) in population() {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let n = count_in(&rel, &text);
        if n > 0 {
            total += n;
            per_file.push((rel, n));
        }
    }
    per_file.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let listing: String = per_file
        .iter()
        .map(|(r, n)| format!("\n    {n:3}  {r}"))
        .collect();

    assert!(
        total <= CAP,
        "`경로:숫자` 인용이 늘었다 — {total} 건 (상한 {CAP}).\n\
         새 인용은 줄 번호가 아니라 **심볼 이름**으로 적는다 (ADR-0194): 경로와 함께 \
         함수·타입 이름을 쓰면 그 인용은 이동을 견디고, 가리키던 것이 사라지면 \
         검색 결과 0 으로 드러난다.\n\
         지금 세어진 자리:{listing}"
    );
    assert!(
        total + BAND >= CAP,
        "`경로:숫자` 인용이 상한보다 {} 건 적다 ({total} / 상한 {CAP} · 띠 {BAND}).\n\
         정리가 진행됐다는 뜻이니 이 시험의 `CAP` 을 {total} 으로 내려라 — 남는 여유는 \
         그만큼 조용히 새 위반을 받아주는 구간이다.\n\
         지금 세어진 자리:{listing}",
        CAP - total
    );
}
