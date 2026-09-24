//! IPC와 원격 구조 변경이 같은 규칙으로 scalar params를 읽는다.
//! 키 부재·null은 미지정, 값의 타입·범위 오류는 거절한다.
//! ID를 잘라 변환하면 다른 대상을 가리킬 수 있어 TryFrom으로 범위를 확인한다.

use serde_json::Value;

/// 잘못된 값과 기대 타입·범위를 함께 알려 준다.
fn malformed(key: &str, raw: &Value, what: &str) -> String {
    format!("'{key}' was given as {raw} — it must be {what}")
}

/// null은 직렬화된 미지정 값으로 취급한다.
fn present<'a>(params: &'a Value, key: &str) -> Option<&'a Value> {
    params.get(key).filter(|v| !v.is_null())
}

/// 미지정이면 None, T에 들어가지 않는 정수면 Err다. 폭은 호출부의 타입이 정한다.
pub(crate) fn read_int<T>(params: &Value, key: &str) -> Result<Option<T>, String>
where
    T: TryFrom<u64>,
{
    let Some(raw) = present(params, key) else {
        return Ok(None);
    };
    raw.as_u64()
        .and_then(|n| T::try_from(n).ok())
        .map(Some)
        .ok_or_else(|| {
            malformed(
                key,
                raw,
                &format!(
                    "a whole number that fits in {} bits and is not negative",
                    std::mem::size_of::<T>() * 8
                ),
            )
        })
}

/// 부호 없는 숫자 ID는 폭 변환 없이 십진 문자열로, 이름은 앞뒤 공백을 제거해 반환한다.
/// 빈 이름은 미지정으로 취급한다.
pub(crate) fn read_id_or_name(params: &Value, key: &str) -> Result<Option<String>, String> {
    let Some(raw) = present(params, key) else {
        return Ok(None);
    };
    if let Some(n) = raw.as_u64() {
        return Ok(Some(n.to_string()));
    }
    let Some(s) = raw.as_str() else {
        return Err(malformed(
            key,
            raw,
            "either a whole number (an id) or a string (a name)",
        ));
    };
    let s = s.trim();
    Ok((!s.is_empty()).then(|| s.to_string()))
}

/// 음수를 허용하는 시각·오프셋 등의 정수를 읽는다.
pub(crate) fn read_i64(params: &Value, key: &str) -> Result<Option<i64>, String> {
    let Some(raw) = present(params, key) else {
        return Ok(None);
    };
    raw.as_i64()
        .map(Some)
        .ok_or_else(|| malformed(key, raw, "a whole number that fits in 64 signed bits"))
}

/// 음수를 허용하되 T의 범위를 벗어나면 자르지 않고 거절한다.
pub(crate) fn read_signed<T>(params: &Value, key: &str) -> Result<Option<T>, String>
where
    T: TryFrom<i64>,
{
    let Some(raw) = present(params, key) else {
        return Ok(None);
    };
    raw.as_i64()
        .and_then(|n| T::try_from(n).ok())
        .map(Some)
        .ok_or_else(|| {
            malformed(
                key,
                raw,
                &format!(
                    "a whole number that fits in {} signed bits",
                    std::mem::size_of::<T>() * 8
                ),
            )
        })
}

pub(crate) fn read_f64(params: &Value, key: &str) -> Result<Option<f64>, String> {
    let Some(raw) = present(params, key) else {
        return Ok(None);
    };
    raw.as_f64()
        .map(Some)
        .ok_or_else(|| malformed(key, raw, "a number"))
}

pub(crate) fn read_u32(params: &Value, key: &str) -> Result<Option<u32>, String> {
    read_int::<u32>(params, key)
}
