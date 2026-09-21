//! IPC params 에서 스칼라를 꺼내는 공용 판정의 **JSON-RPC 얼굴**.
//!
//! 판정 자체(`read_int` · `read_u32` · `read_id_or_name` …)는 도메인 계층의
//! [`crate::core::param_bag`] 가 소유한다 — 원격 mirror 가 forward 한 구조 op 의 실행도
//! 같은 규칙으로 파라미터를 읽어야 해서다. 여기는 그 결과를 `invalid_params` 응답으로
//! 감싸는 자리이고, 핸들러가 부르던 경로(`params::read_u32` 등)는 아래 재수출로 그대로
//! 둔다. 규칙의 근거(자르지 않는다 · 없는 것과 잘못된 것을 가른다)는 그 모듈 문서에 있다.

use serde_json::Value;
use tasty_ipc::protocol::JsonRpcResponse;

pub(crate) use crate::core::param_bag::{
    read_f64, read_i64, read_id_or_name, read_int, read_signed, read_u32,
};

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

    /// 숫자/이름 겸용 토큰의 갈래. 숫자를 **폭에 맞춰 자르지 않는다**는 것이 요지다 —
    /// 여기서 자르면 `u32` 범위 밖 id 가 실재하는 다른 카테고리 이름이 될 수 있다.
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
        // 폭보다 큰 수도 십진 표기 그대로 — 조회에서 "없는 id" 로 떨어질 뿐,
        // 잘려서 **다른** 카테고리를 가리키지 않는다.
        let big = u64::from(u32::MAX) + 2;
        assert_eq!(
            read_id_or_name(&json!({ "category": big }), "category").unwrap(),
            Some(big.to_string())
        );
        assert_eq!(
            read_id_or_name(&json!({ "category": " work " }), "category").unwrap(),
            Some("work".to_string())
        );
        // 빈 토큰은 미지정.
        assert_eq!(
            read_id_or_name(&json!({ "category": "   " }), "category").unwrap(),
            None
        );
        // 숫자도 문자열도 아니면 거절한다 — 조용히 미지정으로 바꾸지 않는다.
        assert!(read_id_or_name(&json!({ "category": [1] }), "category").is_err());
        assert!(read_id_or_name(&json!({ "category": 1.5 }), "category").is_err());
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
