//! 타입 사이의 구조적 대입 가능성 검사.

use super::*;

/// `src` 타입의 모든 유효 값이 `dst` 타입에도 유효한지 검사한다. 값 변환·필드 제거는
/// 하지 않는다. source 쪽 필드가 더 많거나 `json` 을 구체 타입으로 받으려면
/// 명시 projection 이 필요하므로 호환되지 않는다.
pub fn check_assignable(
    src_defs: &TypeDefs,
    src: &TypeSchema,
    dst_defs: &TypeDefs,
    dst: &TypeSchema,
) -> Result<(), TypeError> {
    assignable_at(src_defs, src, dst_defs, dst, "", 0)
}

fn assignable_at(
    sd: &TypeDefs,
    src: &TypeSchema,
    dd: &TypeDefs,
    dst: &TypeSchema,
    path: &str,
    depth: usize,
) -> Result<(), TypeError> {
    let fail = || {
        err(
            TypeErrorKind::Incompatible,
            path,
            dst.describe(),
            src.describe(),
        )
    };
    if depth > MAX_SCHEMA_DEPTH {
        return Err(err(
            TypeErrorKind::TooDeep,
            path,
            format!("schema depth <= {MAX_SCHEMA_DEPTH}"),
            "deeper schema",
        ));
    }
    let s = sd.resolve(src)?;
    let d = dd.resolve(dst)?;
    if matches!(d.kind, TypeKind::Json) {
        return Ok(());
    }
    if s.nullable && !d.nullable {
        return Err(fail());
    }
    match (s.kind, d.kind) {
        (TypeKind::Boolean, TypeKind::Boolean)
        | (TypeKind::Int64, TypeKind::Int64)
        | (TypeKind::Unit, TypeKind::Unit) => Ok(()),
        (
            TypeKind::Float64 {
                min: smin,
                max: smax,
            },
            TypeKind::Float64 {
                min: dmin,
                max: dmax,
            },
        ) => {
            let lo_ok = match (smin, dmin) {
                (_, None) => true,
                (Some(s), Some(d)) => s >= d,
                (None, Some(_)) => false,
            };
            let hi_ok = match (smax, dmax) {
                (_, None) => true,
                (Some(s), Some(d)) => s <= d,
                (None, Some(_)) => false,
            };
            if lo_ok && hi_ok { Ok(()) } else { Err(fail()) }
        }
        (TypeKind::String { max_len: sl }, TypeKind::String { max_len: dl }) => match (sl, dl) {
            (_, None) => Ok(()),
            (Some(s), Some(d)) if s <= d => Ok(()),
            _ => Err(fail()),
        },
        (TypeKind::Enum { values: sv }, TypeKind::Enum { values: dv }) => {
            if sv.iter().all(|v| dv.contains(v)) {
                Ok(())
            } else {
                Err(fail())
            }
        }
        (
            TypeKind::List {
                items: si,
                max_len: sl,
            },
            TypeKind::List {
                items: di,
                max_len: dl,
            },
        ) => {
            match (sl, dl) {
                (_, None) => {}
                (Some(s), Some(d)) if s <= d => {}
                _ => return Err(fail()),
            }
            assignable_at(sd, si, dd, di, &child_path(path, "items"), depth + 1)
        }
        (TypeKind::Object { fields: sf }, TypeKind::Object { fields: df }) => {
            if let Some(extra) = sf.keys().find(|k| !df.contains_key(*k)) {
                return Err(err(
                    TypeErrorKind::UnexpectedField,
                    &child_path(path, extra),
                    "no field outside the target type (project it explicitly)",
                    format!("field \"{extra}\""),
                ));
            }
            for (name, dfield) in df {
                let fpath = child_path(path, name);
                match sf.get(name) {
                    Some(sfield) => {
                        // default 가 있는 source 필드는 검증 뒤 항상 채워진다.
                        let may_be_absent = sfield.optional && sfield.default.is_none();
                        if may_be_absent && !dfield.optional && dfield.default.is_none() {
                            return Err(err(
                                TypeErrorKind::MissingField,
                                &fpath,
                                dfield.schema.describe(),
                                "optional field",
                            ));
                        }
                        assignable_at(sd, &sfield.schema, dd, &dfield.schema, &fpath, depth + 1)?;
                    }
                    None if dfield.optional || dfield.default.is_some() => {}
                    None => {
                        return Err(err(
                            TypeErrorKind::MissingField,
                            &fpath,
                            dfield.schema.describe(),
                            "absent",
                        ));
                    }
                }
            }
            Ok(())
        }
        _ => Err(fail()),
    }
}
