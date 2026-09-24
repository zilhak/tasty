//! 하한을 인자로 받는 함수가 같은 파일의 하한 상수를 다시 참조하는지 검사한다.
//! 합성 트리용 하한을 전달했는데 본문이 실제 저장소용 상수를 읽는 실수를 찾기 위한 검사다.
//!
//! Floor 타입 인자와 floor/min을 이름에 포함한 usize 인자를 구분해 비교한다.
//! usize 분류는 이름 관례에 의존한다. Floor도 정확한 타입 표기를 비교하므로 별칭은 해석하지 않는다.
//! 상수의 선언 범위나 shadowing은 구별하지 않고, 다른 함수를 거친 상수 전달도 추적하지 않는다.
//! 인자를 실제로 사용하는지, 순회로 전달하는지, 하한 값과 순회 루트가 맞는지는 검증하지 않는다.
//! 미사용 인자 경고만으로 빌드가 실패한다고 가정해서는 안 된다.

//! 수집에는 저장소의 소스 마스킹을 사용한다. 주석 안의 경로 글로브를 블록 주석으로 오인하면
//! 뒤의 선언까지 지워져 개수가 줄 수 있다. 같은 측정값도 트리·검색 조건·마스킹 범위가 다르면
//! 같은 근거가 아니며, 공통 하한을 쓴다고 순회 루트와 하강 정책까지 같아지는 것은 아니다.

use std::collections::BTreeSet;

use super::{mask_non_code, matching_delim, rust_sources_with_integration_tests, word_positions};

/// 이 검사 자체가 대상 타입 선언으로 세어지지 않도록 이름을 문자열로 둔다.
const FLOOR_TYPE: &str = "Floor";

const MIN_CONST_PREFIX: &str = "MIN_";
const FLOOR_PARAM_HINTS: &[&str] = &["floor", "min"];

/// 2026-09-08 당시 작업 기준 트리에서 이 검사 자체로 잰 함수 수를 하한으로 삼았다.
/// 주석·리터럴을 가린 뒤 값 또는 참조로 받는 인자를 포함했다. 정확한 트리 해시는 남아 있지 않다.
/// 함수가 줄면 리팩터링과 파서의 수집 누락을 구별해 하한의 근거를 다시 확인한다.
const MIN_TYPED_FLOOR_PARAMS: usize = 12;
const MIN_NAMED_FLOOR_PARAMS: usize = 6;

/// 파일에서 수집한 상수 이름과 해당 종류의 인자를 받는 함수들.
struct Axis {
    consts: BTreeSet<String>,
    fns: Vec<(String, String)>,
}

fn consts_typed(masked: &str, ty: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for kw in word_positions(masked, "const")
        .into_iter()
        .chain(word_positions(masked, "static"))
    {
        let rest = &masked[kw..];
        let Some(colon) = rest.find(':') else {
            continue;
        };
        let Some(eq) = rest.find('=') else { continue };
        if colon > eq {
            continue;
        }
        let name: String = rest[..colon]
            .rsplit(|c: char| c.is_whitespace())
            .next()
            .unwrap_or("")
            .to_string();
        let declared = rest[colon + 1..eq].trim().trim_start_matches('&').trim();
        if declared == ty && !name.is_empty() {
            out.insert(name);
        }
    }
    out
}

fn min_consts(masked: &str) -> BTreeSet<String> {
    consts_typed(masked, "usize")
        .into_iter()
        .filter(|n| n.starts_with(MIN_CONST_PREFIX))
        .collect()
}

fn fn_parts(masked: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for kw in word_positions(masked, "fn") {
        let after = kw + "fn".len();
        let name: String = masked[after..]
            .chars()
            .skip_while(|c| c.is_whitespace())
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        let Some(open) = masked[after..].find('(').map(|i| i + after) else {
            continue;
        };
        let Some(close) = matching_delim(masked, open) else {
            continue;
        };
        let Some((bo, bc)) = super::block_after(masked, close + 1) else {
            continue;
        };
        out.push((
            name,
            masked[open + 1..close].to_string(),
            masked[bo..bc].to_string(),
        ));
    }
    out
}

fn axes_of(masked: &str) -> (Axis, Axis) {
    let typed = consts_typed(masked, FLOOR_TYPE);
    let named = min_consts(masked);
    let (mut ta, mut na) = (Vec::new(), Vec::new());
    for (name, params, body) in fn_parts(masked) {
        if takes_typed_floor(&params) {
            ta.push((name.clone(), body.clone()));
        }
        if takes_named_floor(&params) {
            na.push((name, body));
        }
    }
    (
        Axis {
            consts: typed,
            fns: ta,
        },
        Axis {
            consts: named,
            fns: na,
        },
    )
}

/// Floor 또는 &Floor로 적힌 인자를 찾는다.
fn takes_typed_floor(params: &str) -> bool {
    params.split(',').any(|p| {
        let Some((_, ty)) = p.split_once(':') else {
            return false;
        };
        ty.trim().trim_start_matches('&').trim() == FLOOR_TYPE
    })
}

fn takes_named_floor(params: &str) -> bool {
    params.split(',').any(|p| {
        let Some((name, ty)) = p.split_once(':') else {
            return false;
        };
        if ty.trim() != "usize" {
            return false;
        }
        let name = name.trim().to_ascii_lowercase();
        FLOOR_PARAM_HINTS.iter().any(|h| name.contains(h))
    })
}

fn discarded(body: &str, consts: &BTreeSet<String>) -> Vec<String> {
    consts
        .iter()
        .filter(|c| !word_positions(body, c).is_empty())
        .cloned()
        .collect()
}

type Hit = (String, String, Vec<String>);

fn scan() -> (Vec<Hit>, [usize; 2]) {
    let mut hits: Vec<Hit> = Vec::new();
    let mut counts = [0usize; 2];
    for (path, src) in rust_sources_with_integration_tests() {
        let rel = path.to_string_lossy().into_owned();
        let masked = mask_non_code(&src);
        let (typed, named) = axes_of(&masked);
        for (axis, a) in [typed, named].into_iter().enumerate() {
            counts[axis] += a.fns.len();
            for (name, body) in &a.fns {
                let taken = discarded(body, &a.consts);
                if !taken.is_empty() {
                    hits.push((rel.clone(), name.clone(), taken));
                }
            }
        }
    }
    (hits, counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_floor_taken_as_an_argument_is_not_replaced_by_the_module_constant() {
        let (hits, counts) = scan();
        assert!(
            counts[0] >= MIN_TYPED_FLOOR_PARAMS && counts[1] >= MIN_NAMED_FLOOR_PARAMS,
            "하한 인자를 받는 함수 수가 줄었다. 타입 기준 {}개(하한 {MIN_TYPED_FLOOR_PARAMS}), 이름 기준 {}개(하한 {MIN_NAMED_FLOOR_PARAMS}). 실제 함수 변경과 파서의 수집 누락을 확인한다.",
            counts[0],
            counts[1]
        );
        assert!(
            hits.is_empty(),
            "하한을 인자로 받는 함수가 같은 파일의 하한 상수를 다시 참조한다. 합성 트리용 값을 덮어쓰지 않도록 전달받은 인자를 사용한다: {hits:?}"
        );
    }

    #[test]
    fn the_shape_this_judges_is_actually_caught() {
        let bad = format!(
            "const HOUSE_LIMIT: {FLOOR_TYPE} = make();\n\
             fn read_under(root: &Path, limit: &{FLOOR_TYPE}) {{\n\
             \x20   walk(root, &HOUSE_LIMIT)\n\
             }}\n"
        );
        let (typed, _) = axes_of(&mask_non_code(&bad));
        assert_eq!(
            typed.fns.len(),
            1,
            "Floor 인자를 받는 함수를 수집하지 못했다"
        );
        assert_eq!(
            discarded(&typed.fns[0].1, &typed.consts),
            vec!["HOUSE_LIMIT".to_string()],
            "인자를 버리는 형태를 안 잡았다"
        );

        let bad_named = format!(
            "const {MIN_CONST_PREFIX}HOUSES: usize = 9;\n\
             fn read_under(root: &Path, floor: usize) {{\n\
             \x20   walk(root, {MIN_CONST_PREFIX}HOUSES)\n\
             }}\n"
        );
        let (_, named) = axes_of(&mask_non_code(&bad_named));
        assert_eq!(
            named.fns.len(),
            1,
            "하한 이름의 usize 인자를 받는 함수를 수집하지 못했다"
        );
        assert_eq!(
            discarded(&named.fns[0].1, &named.consts),
            vec![format!("{MIN_CONST_PREFIX}HOUSES")],
            "이름 축에서 인자를 버리는 형태를 안 잡았다"
        );
    }

    #[test]
    fn the_intended_outer_and_inner_pair_is_not_caught() {
        let good = format!(
            "const HOUSE_LIMIT: {FLOOR_TYPE} = make();\n\
             fn read(root: &Path) {{ read_under(root, &HOUSE_LIMIT) }}\n\
             fn read_under(root: &Path, limit: &{FLOOR_TYPE}) {{ walk(root, limit) }}\n"
        );
        let (typed, _) = axes_of(&mask_non_code(&good));
        assert_eq!(
            typed.fns.len(),
            1,
            "하한 인자가 없는 바깥 함수를 검사 대상으로 수집했다"
        );
        assert_eq!(
            typed.fns[0].0, "read_under",
            "수집한 함수가 read_under가 아니다"
        );
        assert!(
            discarded(&typed.fns[0].1, &typed.consts).is_empty(),
            "인자를 그대로 쓰는 속함수를 잡았다"
        );
    }

    /// 검사 대상은 인자로 정하므로 같은 파일에 상수가 없어도 함수는 수집돼야 한다.
    #[test]
    fn the_population_is_counted_by_arguments_not_by_constants() {
        let no_const =
            format!("fn read_under(root: &Path, limit: &{FLOOR_TYPE}) {{ walk(root, limit) }}\n");
        let (typed, _) = axes_of(&mask_non_code(&no_const));
        assert_eq!(typed.consts.len(), 0, "상수가 없어야 하는 픽스처다");
        assert_eq!(
            typed.fns.len(),
            1,
            "파일에 상수가 없다는 이유로 하한 인자를 받는 함수를 제외했다"
        );
    }
}
