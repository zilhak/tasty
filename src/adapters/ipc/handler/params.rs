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

/// 값이 왔는데 `u32` 로 안 읽힐 때의 문구. 값을 되비춰 호출자가 자기 입력을 의심하게 한다.
fn malformed(key: &str, raw: &Value) -> String {
    format!(
        "'{key}' was given as {raw} — it must be a whole number that fits in 32 bits. \
         Refusing rather than truncating it: a truncated id names a different, possibly real, target"
    )
}

/// 왔고 `u32` 범위 안이면 그 값. `null` 이거나 키가 없으면 `None`.
/// **값이 왔는데 안 읽히면 `Err`** — 조용히 버리지 않는다.
fn read_u32(params: &Value, key: &str) -> Result<Option<u32>, String> {
    let Some(raw) = params.get(key).filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    raw.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .map(Some)
        .ok_or_else(|| malformed(key, raw))
}

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
