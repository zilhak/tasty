//! 읽기 전용으로 연 `File` 핸들에 **mtime 을 쓰는 것**을 금지한다.
//!
//! `File::open` 은 읽기 접근만 얻는다. Windows 의 `SetFileTime` 은 핸들에
//! `FILE_WRITE_ATTRIBUTES` 를 요구하므로 그 핸들로 `set_modified`/`set_times` 를
//! 부르면 `PermissionDenied(os error 5)` 가 난다. POSIX `futimens` 는 소유자면
//! 읽기 전용 fd 로도 통과하므로 **Linux·macOS 에서는 드러나지 않는다** — 쓰기
//! 권한으로 열면(`OpenOptions::new().write(true).open(..)`) 양쪽 다 동작한다.
//!
//! 이 함정은 서로 독립인 두 crate 의 테스트에서 같은 형태로 반복됐다. 한쪽을
//! 고쳐도 다른 쪽이 남는 부류라 텍스트로 못박는다.
//!
//! ## 판정 방식
//!
//! 이름이 아니라 **같은 표현식 체인인지**로 가른다 — 마스킹된 소스를 `;` 단위
//! 구문으로 잘라, 한 구문 안에 `File::open(` 과 `.set_modified(`(또는
//! `.set_times(`)가 함께 있을 때만 잡는다. 그래서 `OpenOptions` 로 연 핸들은
//! 통과한다(B-2, `does_not_flag_a_handle_opened_for_write`).
//!
//! ## 면제를 더 좁히지 않는 이유 — "면제는 좁게" 의 예외
//!
//! 창 단위를 `;` 구문에서 **줄**로 좁히면 rustfmt 가 줄바꿈한 멀티라인 체인
//! (이 레포의 실제 위반이 정확히 그 형태였다)을 통째로 놓친다. 즉 여기서는
//! **좁히는 쪽이 검출을 깎는다.** "면제는 좁게" 는 창이 문법 단위보다 넓을 때의
//! 처방이고, `;` 는 이 판정에서 곧 문법 단위(구문 = 표현식 체인의 경계)다.
//! 다음 사람이 이 예외를 규칙의 누락으로 오해하지 않도록 여기 적어 둔다.
//!
//! ## 의도된 false negative — 구문 경계 밖 바인딩(cross-statement binding)
//!
//! 핸들을 변수에 담아 두 구문으로 나눈 형태
//! (`let f = File::open(p)?;` / `f.set_modified(t)?;`)는 **일부러 안 잡는다**.
//! 텍스트만으로는 그 변수가 어떻게 열렸는지 따라갈 수 없기 때문이다. 못 가르는
//! 것을 가르는 척하지 않으려는 결정이며,
//! `intentionally_misses_a_handle_bound_across_statements` 가 그 결정을 고정한다.
//! 나중에 판정기가 이 형태를 잡게 된다면 그건 버그를 고친 것이 아니라 **이 결정을
//! 바꾼 것**이므로, 그 테스트를 함께 고쳐야 한다.

use super::*;

/// mtime 을 쓰는 호출이 레포에서 통째로 사라지면 이 가드는 아무것도 안 보고
/// 통과한다. 실제로 사라졌다면 이 하한을 의도적으로 고쳐야 한다.
const MIN_MTIME_WRITE_SITES: usize = 1;

/// mtime 쓰기 호출이 **어느 파일에 몇 곳** 있는지 고정한다 — 건수 고정이다.
///
/// ## 모수: 4 → 1 → 2 → **3** — 그리고 그때마다 이 표의 역할이 바뀐다
///
/// 원래 이 표가 겨눈 것은 "하한 1 대 실측 4" 의 틈이었다. 네 사이트는 claude/codex
/// plugin 의 **같은 테스트가 두 벌 복제된 것**이었고, 세 곳이 조용히 사라져도 하한은
/// 안 움직였다. 그 복제가 `tasty-plugin-agent-common` 으로 합쳐지면서
/// (`prompt_file` 의 sweep 테스트가 헬퍼 하나를 공유한다) 레포 전체의 mtime 쓰기가
/// **한 파일의 한 곳**이 됐고, 그동안은 하한과 이 표가 "전부 사라짐" 에 대해 **같이 울었다**.
///
/// **2026-09-07 실측 2** — 번들 설치가 내용 판정으로 바뀌며 `builtin.rs` 에 사이트가 하나
/// 늘었다(아래 절). 모수가 1 을 넘었으므로 두 판정이 **다시 갈린다**: 한 파일이 통째로
/// 사라져도 다른 파일이 남아 하한은 조용하고, **이 표만 운다.** 그 폭이 이 표가 되찾은
/// 값이고, `a_lost_file_is_named_not_just_counted` 가 그것을 그대로 단정한다.
///
/// 표가 하는 일 셋은 그대로다: ① 사라진 파일을 **이름으로** 말한다(하한은 수만 말한다),
/// ② 다른 파일에 사이트가 **새로 생기면** 잡는다 — 복제가 되살아나는 형태가 정확히
/// 그것이다, ③ 한 파일 안에서 건수가 늘거나 줄면 잡는다.
///
/// ## 왜 집합 동등이 아니라 건수 고정에서 멈추는가
///
/// `define_class_return` 은 블록마다 `struct <이름>;` 이라는 식별자가 있어 **이름 집합**을
/// 고정할 수 있었다. 여기는 그런 것이 없다 — 사이트의 문장이 `.set_modified(old)` 한 줄이라
/// 서로 구별할 식별자가 없다. 감싸는 함수 이름을 읽으면 갈리지만, 그러려면 이 가드 안에 함수
/// 경계 파서를 새로 만들어야 하고 그 파서가 또 틀릴 수 있다. 겨누는 실패 모드는 "판정기가
/// 사이트를 놓친다" 이고 파일별 건수가 그것을 잡으므로, 거기서 멈춘다.
///
/// ## 사이트가 둘이 된 이유 (2026-09-07)
///
/// 번들 설치가 mtime 대신 **내용**으로 복사 여부를 정하게 바뀌면서, 그 판정이 거짓 mtime 에
/// 안 속는지를 시험이 물어야 했다. 그러려면 시험이 mtime 을 **거짓으로 세워야** 하고
/// (`cp -p`·아카이브 해제가 하는 일), 그것이 `builtin.rs` 의 사이트다. 이 가드가 겨누는
/// 함정(읽기 전용 핸들)은 그 자리에서도 그대로 적용된다 — 그 헬퍼는
/// `OpenOptions::new().write(true)` 로 연다.
///
/// **그래서 안 잡히는 것을 적어 둔다**: 한 파일 안에서 한 사이트를 지우고 다른 사이트를
/// 더하면 이 표는 안 움직인다. `a_same_file_swap_is_not_distinguished` 가 그 한계를
/// 고정한다 — 나중에 판정기가 그것을 가르게 된다면 결함을 고친 것이 아니라 **이 결정을
/// 바꾼 것**이므로 그 테스트를 함께 고쳐야 한다.
/// ## 사이트가 셋이 된 이유 (2026-09-07 실측 3)
///
/// markdown 감시자가 mtime 대신 **내용 지문**으로 판정하게 바뀌면서, 그 판정이 *같은 mtime
/// 으로 다시 쓴 파일*을 보는지를 시험이 물어야 했다. 그러려면 다시 쓴 뒤 mtime 을 **원래
/// 값으로 되돌려야** 하고, 그것이 `watch.rs` 의 사이트다. 여기도
/// `OpenOptions::new().write(true)` 로 연다 — 이 가드가 겨누는 함정은 그 자리에도 그대로
/// 적용된다.
///
/// ☆ 두 사이트(`builtin.rs` · `watch.rs`)가 **같은 이유로** 생겼다: 판정을 시계에서 내용으로
/// 옮기면, 그 판정이 시계에 안 속는다는 것을 보이려고 시험이 시계를 거짓으로 세운다. 그
/// 형태가 또 오면 이 표에 줄이 하나 더 는다 — 그때 이 문단을 늘려라.
const EXPECTED_MTIME_SITES: &[(&str, usize)] = &[
    ("crates/tasty-host-plugin/src/builtin.rs", 1),
    ("crates/tasty-plugin-agent-common/src/prompt_file.rs", 1),
    ("crates/tasty-plugin-markdown/src/watch.rs", 1),
];

/// 레포를 훑어 `파일 → 사이트 수` 를 만든다. 본 판정과 변이가 같은 것을 쓴다.
fn scan_site_population() -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for (path, text) in rust_sources() {
        let found = scan(&mask_non_code(&text));
        if found.sites > 0 {
            out.insert(path.to_string_lossy().replace('\\', "/"), found.sites);
        }
    }
    out
}

/// 스냅샷과의 차이를 사람이 읽을 줄로 낸다. 순수 함수라 변이가 트리를 안 고치고 찌른다.
fn site_drift(actual: &BTreeMap<String, usize>) -> Vec<String> {
    let expected: BTreeMap<String, usize> = EXPECTED_MTIME_SITES
        .iter()
        .map(|(path, n)| ((*path).to_string(), *n))
        .collect();
    let mut drift = Vec::new();
    for (path, n) in &expected {
        match actual.get(path) {
            None => drift.push(format!("  사라짐: {path} (스냅샷 {n} 곳)")),
            Some(m) if m != n => {
                drift.push(format!("  개수 다름: {path} — 스냅샷 {n} · 실측 {m}"));
            }
            Some(_) => {}
        }
    }
    for (path, m) in actual {
        if !expected.contains_key(path) {
            drift.push(format!("  새로 생김: {path} ({m} 곳)"));
        }
    }
    drift
}

const READ_ONLY_OPEN: &str = "File::open(";
const MTIME_WRITES: &[&str] = &[".set_modified(", ".set_times("];

/// 마스킹된 소스 하나에 대한 판정 결과. 줄 번호는 1-based.
struct Scan {
    /// mtime 을 쓰는 호출 수(스캔 하한용).
    sites: usize,
    /// 읽기 전용 핸들로 쓰는 줄.
    violations: Vec<usize>,
}

/// 레포 전수 테스트와 합성 입력 테스트가 함께 부르는 판정기.
fn scan(masked: &str) -> Scan {
    let mut out = Scan {
        sites: 0,
        violations: Vec::new(),
    };
    let mut stmt_start = 0usize;
    for (offset, _) in masked.match_indices(';').chain([(masked.len(), "")]) {
        let stmt = &masked[stmt_start..offset];
        for needle in MTIME_WRITES {
            let Some(rel) = stmt.find(needle) else {
                continue;
            };
            out.sites += 1;
            if stmt.contains(READ_ONLY_OPEN) {
                out.violations.push(line_of(masked, stmt_start + rel));
            }
        }
        stmt_start = offset + 1;
    }
    out
}

#[test]
fn mtime_is_never_written_through_a_read_only_handle() {
    let mut sites = 0usize;
    let mut violations = Vec::new();
    for (path, text) in rust_sources() {
        let found = scan(&mask_non_code(&text));
        sites += found.sites;
        for line in found.violations {
            violations.push(format!("{}:{line}", path.display()));
        }
    }
    assert!(
        sites >= MIN_MTIME_WRITE_SITES,
        "스캔 하한 미달: mtime 을 쓰는 호출을 {sites} 곳 찾았다(하한 \
         {MIN_MTIME_WRITE_SITES}). 정말 사라졌다면 이 하한을 함께 고쳐라"
    );
    let drift = site_drift(&scan_site_population());
    assert!(
        drift.is_empty(),
        "mtime 쓰기 사이트 분포가 스냅샷과 다르다 — 하한 {MIN_MTIME_WRITE_SITES} 은 사라진 \
         파일의 이름도, 다른 파일에 새로 생긴 사이트도 말하지 못하므로 파일별 건수를 \
         고정한다.\n{}",
        drift.join("\n")
    );
    assert!(
        violations.is_empty(),
        "`File::open` 은 읽기 접근만 얻는다 — 그 핸들로 mtime 을 쓰면 Windows 에서 \
         `PermissionDenied(os error 5)` 가 난다(Linux·macOS 는 통과해서 안 드러난다). \
         `std::fs::OpenOptions::new().write(true).open(..)` 로 열어라.\n  {}",
        violations.join("\n  ")
    );
}

mod exemption_mutations {
    //! 이 가드의 **면제마다** 그것을 겨냥한 변이. 특히 B-1(`;` 구문 창)은 두 needle
    //! **사이**에 세미콜론을 숨겨야 판별력이 생긴다 — 세미콜론이 needle 뒤에 있으면
    //! 구문이 갈려도 앞 조각에 둘 다 남아 여전히 잡히므로 면제를 찌르지 못한다.

    use super::*;

    /// B-1 을 겨냥한다. 문자열 속 `;` 가 구문을 잘라 버리면 `File::open(` 과
    /// `.set_modified(` 가 서로 다른 조각으로 갈려 진짜 위반이 빠져나간다.
    #[test]
    fn catches_a_chain_whose_semicolon_hides_in_a_string() {
        let src = "std::fs::File::open(&p.join(\"a;b\")).unwrap().set_modified(t).unwrap();\n";
        let found = scan(&mask_non_code(src));
        assert_eq!(found.sites, 1);
        assert_eq!(found.violations, vec![1]);
    }

    /// 같은 면제, 주석판.
    #[test]
    fn catches_a_chain_whose_semicolon_hides_in_a_comment() {
        let src = "std::fs::File::open(&p /* ; */).unwrap().set_modified(t).unwrap();\n";
        let found = scan(&mask_non_code(src));
        assert_eq!(found.sites, 1);
        assert_eq!(found.violations, vec![1]);
    }

    /// 같은 면제, 멀티라인 체인 — 줄 단위로 좁히면 놓치는 형태(레포의 실제 위반이
    /// 이 모양이었다). 위반 줄은 mtime 을 쓰는 줄로 보고한다.
    #[test]
    fn catches_a_chain_broken_across_lines_by_rustfmt() {
        let src =
            "std::fs::File::open(&stale)\n    .unwrap()\n    .set_modified(old)\n    .unwrap();\n";
        assert_eq!(scan(&mask_non_code(src)).violations, vec![3]);
    }

    /// B-2 의 정당한 쪽 — 쓰기 권한으로 연 핸들은 잡지 않되, 호출 수에는 센다.
    #[test]
    fn does_not_flag_a_handle_opened_for_write() {
        let src =
            "std::fs::OpenOptions::new().write(true).open(&p).unwrap().set_modified(t).unwrap();\n";
        let found = scan(&mask_non_code(src));
        assert_eq!(found.sites, 1);
        assert!(found.violations.is_empty());
    }

    /// **의도된 false negative — 구문 경계 밖 바인딩.** 이 테스트가 깨졌다면
    /// 판정기가 넓어진 것이고, 그건 버그 수정이 아니라 결정 변경이다.
    #[test]
    fn intentionally_misses_a_handle_bound_across_statements() {
        let src = "let f = std::fs::File::open(&p).unwrap();\nf.set_modified(t).unwrap();\n";
        let found = scan(&mask_non_code(src));
        assert_eq!(found.sites, 1);
        assert!(
            found.violations.is_empty(),
            "구문 경계 밖 바인딩은 일부러 안 잡는다 — 판정기를 넓혔다면 이 결정을 \
             바꾼 것이므로 가드 doc 도 함께 고쳐라"
        );
    }

    /// `set_times` 도 같은 부류다 — needle 목록이 줄어들지 않았는지 본다.
    #[test]
    fn catches_set_times_as_well_as_set_modified() {
        let src = "std::fs::File::open(&p).unwrap().set_times(times).unwrap();\n";
        assert_eq!(scan(&mask_non_code(src)).violations, vec![1]);
    }
}

/// 이 승급을 겨냥한 변이.
mod population_mutations {
    use super::*;

    /// 무변이 대조 + 하한이 실제로 채워져 있는지.
    #[test]
    fn the_unmutated_site_map_has_no_drift() {
        let actual = scan_site_population();
        assert!(site_drift(&actual).is_empty(), "무변이인데 차분이 있다");
        let total: usize = actual.values().sum();
        assert!(
            total >= MIN_MTIME_WRITE_SITES,
            "사이트를 {total} 곳 찾았다(하한 {MIN_MTIME_WRITE_SITES}) — 스캐너가 죽었다"
        );
    }

    /// 파일이 통째로 빠지면 **이름으로** 말한다.
    ///
    /// 모수가 2 라 이 변이는 **하한이 못 잡는다** — 한 파일이 통째로 사라져도 다른 파일의
    /// 사이트가 남아 하한은 조용하다. 그 폭이 이 표의 값이고, 아래에서 그 조용함까지
    /// 함께 단정한다(표만 우는 것을 확인해야 "표가 일한다" 가 측정된 말이 된다).
    #[test]
    fn a_lost_file_is_named_not_just_counted() {
        let actual = scan_site_population();
        let victim = actual.keys().next().expect("대조군이 비었다").clone();
        let mut lost = actual.clone();
        lost.remove(&victim).expect("방금 고른 키다");

        let drift = site_drift(&lost);
        assert_eq!(drift.len(), 1, "잃은 파일 하나만 말해야 한다: {drift:?}");
        assert!(
            drift[0].contains(&victim),
            "이름으로 말하지 않는다: {drift:?}"
        );

        // 지금 모수(2)에서는 **하한이 조용하다** — 그것이 이 표가 되찾은 폭이다.
        // 이 줄이 깨졌다면 모수가 다시 1 로 줄어 두 판정이 겹친 것이고, 그때는 doc 의
        // "모수: 4 → 1 → 2" 절과 이 주석을 그 실측으로 함께 고쳐라.
        let left: usize = lost.values().sum();
        assert!(
            left >= MIN_MTIME_WRITE_SITES,
            "모수가 줄었다({left} 곳 남음) — 하한과 이 표가 다시 겹친다. doc 과 이 주석을 함께 고쳐라"
        );
    }

    /// **다른 파일에 사이트가 새로 생기면 잡는다.** 지금 이 표의 주된 일이다 —
    /// 합쳤던 헬퍼가 어느 plugin 으로 되살아나는 형태가 정확히 이 모양이다.
    #[test]
    fn a_new_site_in_another_file_is_caught() {
        let mut grown = scan_site_population();
        grown.insert("crates/tasty-plugin-codex/src/handlers.rs".into(), 2);

        let drift = site_drift(&grown);
        assert_eq!(drift.len(), 1, "{drift:?}");
        assert!(drift[0].contains("새로 생김"), "{drift:?}");
        assert!(drift[0].contains("tasty-plugin-codex"), "{drift:?}");
    }

    /// 한 파일 안에서 개수가 하나 줄면 잡는다 — 하한이 못 보는 자리다.
    #[test]
    fn a_single_lost_site_inside_a_file_is_caught() {
        let actual = scan_site_population();
        let victim = actual.keys().next().expect("대조군이 비었다").clone();
        let mut thinner = actual.clone();
        *thinner.get_mut(&victim).expect("방금 고른 키다") -= 1;

        let drift = site_drift(&thinner);
        assert_eq!(drift.len(), 1, "{drift:?}");
        assert!(drift[0].contains("개수 다름"), "{drift:?}");
    }

    /// **못 가르는 것을 가르는 척하지 않는다.** 한 파일 안에서 한 곳을 지우고 다른 곳을
    /// 더하면 건수가 그대로라 이 표는 안 움직인다. `define_class_return` 은 블록마다
    /// 이름이 있어 이것을 갈랐지만 여기는 사이트에 식별자가 없다. 판정기가 나중에 이
    /// 형태를 가르게 된다면 그건 결함을 고친 것이 아니라 **이 결정을 바꾼 것**이므로,
    /// 이 테스트를 함께 고쳐야 한다.
    #[test]
    fn a_same_file_swap_is_not_distinguished() {
        // 같은 파일 안에서 사이트가 **다른 자리로 옮겨간** 두 소스. 내용은 다르고 건수는 같다.
        let together = "\
fn only_here() {
    let f = std::fs::OpenOptions::new().write(true).open(p).unwrap();
    f.set_modified(t).unwrap();
    let g = std::fs::OpenOptions::new().write(true).open(p).unwrap();
    g.set_modified(t).unwrap();
}
";
        let apart = "\
fn first() {
    let f = std::fs::OpenOptions::new().write(true).open(p).unwrap();
    f.set_modified(t).unwrap();
}

fn second() {
    let g = std::fs::OpenOptions::new().write(true).open(p).unwrap();
    g.set_modified(t).unwrap();
}
";
        let a = scan(&mask_non_code(together));
        let b = scan(&mask_non_code(apart));
        assert_ne!(
            together, apart,
            "두 소스가 같으면 이 테스트는 아무것도 안 잰다"
        );
        assert!(
            a.violations.is_empty() && b.violations.is_empty(),
            "합성 소스에 위반이 있으면 아래 비교가 다른 이유로 갈린다"
        );
        assert_eq!(a.sites, 2, "합성 소스에서 사이트를 못 셌다");

        // 파일별 건수만 들고 있으므로 두 소스의 판정이 **같다.** 이것이 이 승급이
        // 가르지 못하는 형태다.
        assert_eq!(
            a.sites, b.sites,
            "이 한계가 사라졌다면 판정기가 사이트 정체를 갖게 된 것이다 — doc 과 이 \
             테스트를 함께 고쳐라"
        );
        let same_map: BTreeMap<String, usize> =
            [("x.rs".to_string(), a.sites)].into_iter().collect();
        let also_same: BTreeMap<String, usize> =
            [("x.rs".to_string(), b.sites)].into_iter().collect();
        assert_eq!(same_map, also_same);
    }

    /// 이 가드 파일 자신이 모수에 들어오지 않는가 — 여기 needle 은 전부 문자열이라
    /// 마스킹으로 지워진다. 안 지워지면 스냅샷이 자기 참조가 된다.
    #[test]
    fn the_guard_file_does_not_count_itself() {
        let actual = scan_site_population();
        let me = "src/source_guards/read_only_handle_mtime.rs";
        assert!(
            !actual.contains_key(me),
            "가드 자신이 모수에 들어왔다: {actual:?}"
        );
        assert!(
            !actual.is_empty(),
            "모수가 비었다 — 위 단정은 언제나 통과한다"
        );
    }
}
