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
//!
//! **왜 도메인 계층인가**: 이 판정을 부르는 쪽이 IPC 핸들러만이 아니다. 원격 mirror 가
//! forward 한 구조 op 도 같은 모양의 파라미터 묶음(`StructuralOp` 의 `params`)을 싣고 오고,
//! 그 실행(`core::attach_runtime` 의 forward 실행)이 IPC 요청과 **같은 규칙**으로 그것을 읽어야 같은
//! 입력에 같은 실패 문구가 나온다. 판정이 어댑터(`adapters::ipc::handler::params`)에
//! 있으면 도메인 실행이 inbound 어댑터를 부르게 된다 — 그 역방향을 없애려고 판정만 여기로
//! 내렸다. JSON-RPC 응답으로 감싸는 얼굴(`require_u32` · `opt_int` · `p_try!` …)은
//! 어댑터에 남는다.

use serde_json::Value;

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

/// 숫자(id) 로도 문자열(이름) 로도 올 수 있는 **식별 토큰**을 읽어 문자열로 정규화한다.
///
/// 이 모양이 관문 밖에 남으면 안 되는 이유는 다른 스칼라와 같다: 관문 밖의 코드가
/// `v.as_u64()` 를 직접 부르는 순간, 그 자리가 자르기·버리기를 하는지 아무도 안 본다.
/// 여기서는 자르지 않는다 — 숫자는 폭 변환 없이 십진 표기로 넘긴다.
///
/// 빈 문자열(공백만 포함)은 **미지정**으로 읽는다. 값을 안 넣은 것과 빈 칸을 보낸 것을
/// 가를 근거가 없고, 이름 조회에 빈 토큰을 넘기면 "없는 이름" 이라는 엉뚱한 에러가 난다.
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

/// 부호 있는 정수(시각·오프셋 등). 음수가 정당한 자리에 쓴다.
pub(crate) fn read_i64(params: &Value, key: &str) -> Result<Option<i64>, String> {
    let Some(raw) = present(params, key) else {
        return Ok(None);
    };
    raw.as_i64()
        .map(Some)
        .ok_or_else(|| malformed(key, raw, "a whole number that fits in 64 signed bits"))
}

/// 부호 있는 정수를 **폭에 맞게** 읽는다. `exit_code`(i32) 처럼 음수가 정당하면서
/// 폭이 좁은 자리에 쓴다 — `as i32` 로 자르면 `4_294_967_296` 이 `0`(정상 종료!)이 된다.
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
