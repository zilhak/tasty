//! core::param_bag의 입력 검사 결과를 JSON-RPC 오류로 바꾼다.
//! 원격 구조 변경과 IPC가 같은 범위·타입 검사를 사용한다.

use serde_json::Value;
use tasty_ipc::protocol::JsonRpcResponse;

pub(crate) use crate::core::param_bag::{
    read_f64, read_i64, read_id_or_name, read_int, read_signed, read_u32,
};

/// 생략한 선택 정수는 None, 잘못된 타입·범위는 Err로 구분한다.
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

/// 핸들러는 Result 대신 JsonRpcResponse를 반환하므로 ? 대신 오류 응답을 바로 반환한다.
/// 구조체 필드 등 식이 필요한 위치에서도 사용할 수 있다.
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

/// 생략한 선택 u32는 None이고 잘못된 값은 오류다.
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

    #[test]
    fn require_u32_separates_absent_from_malformed_and_refuses_to_truncate() {
        let e = require_u32(&json!({}), "surface", &id()).unwrap_err();
        assert!(format!("{e:?}").contains("missing"), "{e:?}");

        assert_eq!(require_u32(&json!({ "s": 0 }), "s", &id()).unwrap(), 0);
        assert_eq!(
            require_u32(&json!({ "s": u32::MAX }), "s", &id()).unwrap(),
            u32::MAX
        );

        let e = require_u32(&json!({ "s": "conductor" }), "s", &id()).unwrap_err();
        let m = format!("{e:?}");
        assert!(m.contains("32 bits"), "{m}");
        assert!(!m.contains("missing"), "값이 왔는데 없다고 답한다: {m}");

        // 범위 초과 값을 잘라 다른 ID로 해석하면 안 된다.
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

    #[test]
    fn read_id_or_name_normalises_both_shapes_without_narrowing_the_number() {
        assert_eq!(read_id_or_name(&json!({}), "category").unwrap(), None);
        assert_eq!(
            read_id_or_name(&json!({ "category": Value::Null }), "category").unwrap(),
            None
        );
        assert_eq!(
            read_id_or_name(&json!({ "category": 3 }), "category").unwrap(),
            Some("3".to_string())
        );
        // 큰 숫자도 문자열로 그대로 넘겨 다른 대상의 ID로 바뀌지 않게 한다.
        let big = u64::from(u32::MAX) + 2;
        assert_eq!(
            read_id_or_name(&json!({ "category": big }), "category").unwrap(),
            Some(big.to_string())
        );
        assert_eq!(
            read_id_or_name(&json!({ "category": " work " }), "category").unwrap(),
            Some("work".to_string())
        );
        assert_eq!(
            read_id_or_name(&json!({ "category": "   " }), "category").unwrap(),
            None
        );
        assert!(read_id_or_name(&json!({ "category": [1] }), "category").is_err());
        assert!(read_id_or_name(&json!({ "category": 1.5 }), "category").is_err());
    }

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
