//! IPC params 에서 스칼라를 꺼내는 공용 판정.
//!
//! **왜 한 자리인가**: 같은 몸통의 `require_u32` 가 `terminal.rs` · `pty.rs` ·
//! `preset.rs` 에 세 벌 있었고, 셋 다 같은 결함을 갖고 있었다. 하나를 고치면 나머지
//! 둘은 안 고쳐진다 — 규칙이 셋으로 흩어져 있으면 판정도 셋으로 흩어진다.
//!
//! **무엇을 고쳤나**: `as_u64()` 로 읽은 값을 `as u32` 로 자르던 것. 자르기는 값을
//! **거절하지 않고 다른 값으로 바꾼다** — `4_294_967_297` 은 `1` 이 되고
//! `5_000_000_000` 은 `705_032_704` 가 된다. 그 결과가 surface id 자리에 들어가면
//! 실재하는 **다른 surface** 를 가리키게 되어, 명령이 조용히 남의 터미널로 간다
//! (실측: `surface.locate` 에 `<실재 id> + 2^32` 를 주면 그 실재 surface 를 그대로
//! 되돌려줬다).
//!
//! **없는 것과 잘못된 것을 가른다**: 값이 왔는데 안 읽히는 것을 "missing" 이라고
//!답하면 호출자가 자기가 준 값을 안 의심한다. `null` 은 **안 왔다**로 읽는다 —
//! 직렬화가 빈 슬롯을 `null` 로 채우는 경우가 있어, 오타로 취급하면 정상 경로가 막힌다.

use serde_json::Value;
use tasty_ipc::protocol::JsonRpcResponse;

/// 값이 왔는데 안 읽힐 때의 문구. 값과 **기대한 폭**을 되비춰 호출자가 자기 입력을
/// 의심하게 한다 — "숫자가 아니다" 와 "숫자인데 범위 밖이다" 는 고칠 방법이 다르다.
fn malformed(key: &str, raw: &Value, what: &str) -> String {
    format!(
        "'{key}' was given as {raw} — it must be {what}. Refusing rather than coercing it: \
         a truncated id names a different, possibly real, target, and a dropped value is \
         indistinguishable from the parameter being absent"
    )
}

/// 키가 왔는가. `null` 은 **안 왔다**로 읽는다 — 직렬화가 빈 슬롯을 `null` 로 채우는
/// 경우가 있어, 오타로 취급하면 정상 경로가 막힌다.
fn present<'a>(params: &'a Value, key: &str) -> Option<&'a Value> {
    params.get(key).filter(|v| !v.is_null())
}

/// 부호 없는 정수 파라미터를 **폭에 맞게** 읽는다. `null`/키 없음은 `None`,
/// **값이 왔는데 안 읽히면 `Err`** — 조용히 버리거나 자르지 않는다.
///
/// 폭은 호출부의 타입이 정한다(`read_int::<u32>` · `::<u16>` · `::<usize>` …).
/// `try_into` 가 실패하는 것이 곧 "이 자리에 안 들어가는 값" 이다.
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

/// 부호 있는 정수(시각·오프셋 등). 음수가 정당한 자리에 쓴다.
pub(crate) fn read_i64(params: &Value, key: &str) -> Result<Option<i64>, String> {
    let Some(raw) = present(params, key) else {
        return Ok(None);
    };
    raw.as_i64()
        .map(Some)
        .ok_or_else(|| malformed(key, raw, "a whole number that fits in 64 signed bits"))
}

/// 실수 파라미터(정규화 좌표 등).
pub(crate) fn read_f64(params: &Value, key: &str) -> Result<Option<f64>, String> {
    let Some(raw) = present(params, key) else {
        return Ok(None);
    };
    raw.as_f64()
        .map(Some)
        .ok_or_else(|| malformed(key, raw, "a number"))
}

/// `read_int::<u32>` 의 이름 있는 별칭 — surface/pane/workspace id 자리가 가장 많아
/// 호출부가 매번 turbofish 를 쓰지 않게 한다.
pub(crate) fn read_u32(params: &Value, key: &str) -> Result<Option<u32>, String> {
    read_int::<u32>(params, key)
}

/// 선택 정수 파라미터 — **안 온 것만 `None`** 이다. 잘못 온 것은 `Err`.
///
/// 종전에는 `params.get(k).and_then(|v| v.as_u64())` 가 둘을 합쳐 `None` 을 냈고,
/// 그러면 **필터가 필터링을 멈추거나**(`since`/`limit`) 호출자가 지정한 대상 대신
/// 기본값이 쓰였다. 둘 다 조용히 틀린 결과를 낸다.
pub(crate) fn opt_int<T>(
    params: &Value,
    key: &str,
    id: &Value,
) -> Result<Option<T>, JsonRpcResponse>
where
    T: TryFrom<u64>,
{
    let r = read_int::<T>(params, key);
    match r {
        Ok(v) => Ok(v),
        Err(msg) => Err(JsonRpcResponse::invalid_params(id.clone(), msg)),
    }
}

/// 선택 부호 있는 정수(시각·오프셋).
pub(crate) fn opt_i64(
    params: &Value,
    key: &str,
    id: &Value,
) -> Result<Option<i64>, JsonRpcResponse> {
    read_i64(params, key).map_err(|msg| JsonRpcResponse::invalid_params(id.clone(), msg))
}

/// 선택 실수(정규화 좌표·임계값).
pub(crate) fn opt_f64(
    params: &Value,
    key: &str,
    id: &Value,
) -> Result<Option<f64>, JsonRpcResponse> {
    read_f64(params, key).map_err(|msg| JsonRpcResponse::invalid_params(id.clone(), msg))
}

/// `Result<_, JsonRpcResponse>` 를 그 자리에서 풀거나 응답을 반환한다.
///
/// **구조체 리터럴 안에서 쓰려고** 있다. `ListOpts { since: p_try!(...), .. }` 처럼
/// 식(expression) 자리에 들어가야 해서 `?` 를 못 쓴다(`?` 는 함수 반환 타입이
/// `Result` 여야 하는데 핸들러는 `JsonRpcResponse` 를 그대로 돌려준다).
macro_rules! p_try {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(resp) => return resp,
        }
    };
}
pub(crate) use p_try;

/// 필수 `u32` 파라미터. 없으면 `missing '<key>'`, 잘못됐으면 그 값을 되비추는 문구.
pub(crate) fn require_u32(params: &Value, key: &str, id: &Value) -> Result<u32, JsonRpcResponse> {
    match read_u32(params, key) {
        Ok(Some(v)) => Ok(v),
        Ok(None) => Err(JsonRpcResponse::invalid_params(
            id.clone(),
            format!("missing '{key}'"),
        )),
        Err(msg) => Err(JsonRpcResponse::invalid_params(id.clone(), msg)),
    }
}

/// 선택 `u32` 파라미터. **안 온 것만 `None`** 이다 — 잘못 온 것은 `Err` 로 올라간다.
/// 종전에는 잘못 온 값이 `None` 이 되어 "안 줬다" 와 구별되지 않았고, 호출자가 지정한
/// 대상이 조용히 기본값으로 바뀌었다.
pub(crate) fn optional_u32(
    params: &Value,
    key: &str,
    id: &Value,
) -> Result<Option<u32>, JsonRpcResponse> {
    read_u32(params, key).map_err(|msg| JsonRpcResponse::invalid_params(id.clone(), msg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn id() -> Value {
        json!(1)
    }

    /// 네 갈래를 픽스처로 못박는다 — 실재하는 surface id 를 안 쓴다(그 id 가 사라지면
    /// 회귀가 조용히 뜻을 잃는다).
    #[test]
    fn require_u32_separates_absent_from_malformed_and_refuses_to_truncate() {
        // ① 키 없음.
        let e = require_u32(&json!({}), "surface", &id()).unwrap_err();
        assert!(format!("{e:?}").contains("missing"), "{e:?}");

        // ② 정상 — 경계값이 그대로 통과한다.
        assert_eq!(require_u32(&json!({ "s": 0 }), "s", &id()).unwrap(), 0);
        assert_eq!(
            require_u32(&json!({ "s": u32::MAX }), "s", &id()).unwrap(),
            u32::MAX
        );

        // ③ 숫자가 아니다 — 거부하고, "missing" 이라고 답하지 않는다.
        let e = require_u32(&json!({ "s": "conductor" }), "s", &id()).unwrap_err();
        let m = format!("{e:?}");
        assert!(m.contains("32 bits"), "{m}");
        assert!(!m.contains("missing"), "값이 왔는데 없다고 답한다: {m}");

        // ④ ★ 범위 초과 — 자르면 다른 대상이 된다. `u32::MAX + 2` 는 1 로 잘린다.
        for over in [
            u64::from(u32::MAX) + 1,
            u64::from(u32::MAX) + 2,
            5_000_000_000,
        ] {
            let e = require_u32(&json!({ "s": over }), "s", &id()).unwrap_err();
            assert!(format!("{e:?}").contains("32 bits"), "{over} 가 안 걸린다");
        }

        assert!(require_u32(&json!({ "s": -1 }), "s", &id()).is_err());
    }

    /// 선택 인자에서 **안 온 것**과 **잘못 온 것**이 갈린다. 종전에는 둘 다 `None` 이라
    /// 호출자가 지정한 대상이 조용히 기본값으로 바뀌었다.
    #[test]
    fn optional_u32_reports_a_malformed_value_instead_of_dropping_it() {
        assert_eq!(optional_u32(&json!({}), "pane", &id()).unwrap(), None);
        assert_eq!(
            optional_u32(&json!({ "pane": Value::Null }), "pane", &id()).unwrap(),
            None
        );
        assert_eq!(
            optional_u32(&json!({ "pane": 7 }), "pane", &id()).unwrap(),
            Some(7)
        );
        assert!(optional_u32(&json!({ "pane": "left" }), "pane", &id()).is_err());
        assert!(optional_u32(&json!({ "pane": u64::from(u32::MAX) + 2 }), "pane", &id()).is_err());
    }
}
