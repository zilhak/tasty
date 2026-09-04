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
