//! CLI 인자 하나의 값을 꺼내고 타입을 강제하는 층 + stdin JSON 병합.
//!
//! 이 모듈은 `dynamic` 의 다른 어느 모듈도 부르지 않는다 — 값 하나만 본다.
//! 그래서 여기 있는 판정(숫자가 아니다 / 범위 밖이다 / 반복 지정됐다)은
//! 명령 구성이나 요청 조립을 몰라도 성립한다.

use anyhow::{Result, anyhow};
use clap::ArgMatches;
use serde_json::{Map, Value};

use tasty_plugin_manifest::{CliArg, CliArgGroup, CliArgType};

/// `read_stdin_json`이 값을 만들지 못한 이유. 어느 경로였는지 로그로 구분하기
/// 위한 것 — `claude hook session-start`가 이 경로 중 하나로 `None`을 받으면
/// `claude-session-id` meta 가 기록되지 않는다(사고 2026-08-05, surface 3095).
enum StdinSkipReason {
    /// 사람이 터미널에서 직접 커맨드를 입력한 경우 정상적으로 발생.
    Tty,
    ReadError(std::io::Error),
    /// non-TTY 인데도 payload 가 비어 있음 — 호출자(예: Claude Code hook
    /// 시스템)가 stdin 을 채우지 않은 상태.
    Empty,
    ParseError(serde_json::Error),
}

impl StdinSkipReason {
    /// TTY 는 사람이 직접 커맨드를 입력했을 때 정상적으로 발생 — warn 대상이
    /// 아니다. 나머지(non-TTY 인데 값이 없음)는 호출자가 payload 를 못 채운
    /// 것이므로 warn.
    fn is_expected(&self) -> bool {
        matches!(self, Self::Tty)
    }

    fn describe(&self) -> String {
        match self {
            Self::Tty => "stdin is a TTY".to_string(),
            Self::ReadError(e) => format!("failed to read stdin (non-TTY): {e}"),
            Self::Empty => "stdin was empty (non-TTY but no data piped)".to_string(),
            Self::ParseError(e) => format!("failed to parse stdin as JSON: {e}"),
        }
    }
}

/// stdin 이 TTY 가 아닐 때 (= pipe / redirect 로 입력이 들어올 때) stdin 전체를
/// JSON 한 덩이로 파싱한다. TTY 이거나 파싱 실패 시 `None`. blocking read 를
/// 피하기 위해 TTY 체크를 먼저 한다 — TTY 라면 사용자가 enter 칠 때까지 멈춰
/// 있을 위험이 있다.
pub(super) fn read_stdin_json() -> Option<Value> {
    let reason = match try_read_stdin_json() {
        Ok(v) => return Some(v),
        Err(reason) => reason,
    };
    if reason.is_expected() {
        tracing::debug!("read_stdin_json: {}", reason.describe());
    } else {
        tracing::warn!("read_stdin_json: {}", reason.describe());
    }
    None
}

fn try_read_stdin_json() -> Result<Value, StdinSkipReason> {
    use std::io::{IsTerminal, Read};
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(StdinSkipReason::Tty);
    }
    let mut buf = String::new();
    stdin
        .read_to_string(&mut buf)
        .map_err(StdinSkipReason::ReadError)?;
    if buf.trim().is_empty() {
        return Err(StdinSkipReason::Empty);
    }
    serde_json::from_str(&buf).map_err(StdinSkipReason::ParseError)
}

/// CLI 로 지정되지 않은 params 필드를, stdin JSON 의 해당 키에서 꺼내 채운다.
/// 매칭 키는 `arg.stdin_field` 우선, 없으면 `arg.name`. CLI 가 이미 채운 키는
/// 건드리지 않는다.
///
/// **선언 타입은 여기서도 강제한다.** 이 경로는 `extract_value`(=`--flag` 경로)를
/// 지나지 않으므로, 강제를 그쪽에만 두면 같은 `CliArg` 선언이 **들어온 문으로만**
/// 참인 보증이 된다(ADR-0132). 매니페스트가 `u32` 라고 적어둔 인자에 stdin JSON 이
/// 객체나 문자열을 실어 보내면 그대로 params 에 들어가 하류가 그것을 받는다.
pub(super) fn merge_stdin_params(
    params: &mut Map<String, Value>,
    group: &CliArgGroup,
    stdin: &Value,
) -> Result<()> {
    let Some(obj) = stdin.as_object() else {
        return Ok(());
    };
    for arg in group.positional.iter().chain(group.flags.iter()) {
        if params.contains_key(&arg.name) {
            continue;
        }
        let key = arg.stdin_field.as_deref().unwrap_or(&arg.name);
        if let Some(v) = obj.get(key)
            && !v.is_null()
        {
            params.insert(arg.name.clone(), coerce_stdin_value(v, arg)?);
        }
    }
    Ok(())
}

/// stdin JSON 값을 선언 타입에 맞춘다. 숫자 타입은 `--flag` 경로와 **같은 규칙**을
/// 쓴다 — 숫자로 읽히면 숫자로, 아니면 거부. `string`/`bool` 은 JSON 이 이미 타입을
/// 싣고 오므로 그대로 통과시킨다(그 둘의 강제는 이 ADR 의 범위 밖이다).
pub(super) fn coerce_stdin_value(v: &Value, arg: &CliArg) -> Result<Value> {
    match arg.ty {
        CliArgType::U32 => coerce_stdin_number::<u32>(v, arg),
        CliArgType::I64 => coerce_stdin_number::<i64>(v, arg),
        CliArgType::String | CliArgType::Bool => Ok(v.clone()),
    }
}

/// JSON 값을 문자열 한 형태로 눕힌 뒤 `parse_number` 에 넘긴다. 눕히는 이유는
/// 오류 메시지를 `--flag` 경로와 같게 하기 위해서다 — 호출자는 두 문 중 어느 쪽으로
/// 들어왔든 "어느 인자의 어떤 값이 문제인가" 를 같은 문장으로 받는다.
/// `1.5`(정수 아님) · `true` · 배열/객체는 전부 여기서 거부된다.
pub(super) fn coerce_stdin_number<T>(v: &Value, arg: &CliArg) -> Result<Value>
where
    T: std::str::FromStr,
    Value: From<T>,
{
    let raw = match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    Ok(Value::from(parse_number::<T>(&raw, arg)?))
}

/// 숫자 인자는 **값이 왔는데 못 읽으면 오류**다.
///
/// `parse().ok()` 로 `None` 을 만들면 하류에서 **"플래그가 아예 없음" 과 구별되지 않는다.**
/// 그러면 없을 때 도는 기본값 경로가 그대로 돌아, 사용자가 지정한 대상 대신 기본 대상으로
/// 조용히 실행된다 — `--surface` 의 기본값은 호출자 **자신**이라 명령이 자기에게 배달된다.
/// 종료코드는 0 이고 오류도 없어서, 응답의 주소를 따로 대조하지 않으면 드러나지 않는다.
pub(super) fn parse_number<T>(raw: &str, arg: &CliArg) -> Result<T>
where
    T: std::str::FromStr,
{
    raw.parse::<T>().map_err(|_| {
        let flag = arg
            .flag
            .as_deref()
            .unwrap_or(&arg.name)
            .trim_start_matches('-');
        // **정수인데 범위를 벗어난 것**과 **애초에 숫자가 아닌 것**을 가른다.
        // 둘을 한 문구로 답하면 `4294967297` 을 준 사용자가 "숫자가 아니다" 를 듣고
        // 자기가 오타를 냈다고 생각한다 — 실제로는 값이 크기만 한 것이고, 고칠 방법이
        // 전혀 다르다. `i128` 로 한 번 더 읽어 어느 쪽인지 판정한다(u32·i64 를 모두
        // 담는다).
        let key = if raw.trim().parse::<i128>().is_ok() {
            "cli.plugin_cli.flag_number_out_of_range"
        } else {
            "cli.plugin_cli.flag_not_a_number"
        };
        anyhow!("{}", tasty_i18n::t_fmt2(key, flag, raw))
    })
}

pub(super) fn extract_value(matches: &ArgMatches, arg: &CliArg) -> Result<Option<Value>> {
    // `reject_repeat` 인자는 build_arg 가 ArgAction::Append 로 등록하므로(모든
    // occurrence 보존), get_one 대신 get_many 로 개수부터 확인한다 — Set 전용
    // 접근(get_one)을 Append 인자에 섞으면 clap 내부 불변식과 어긋난다.
    if arg.reject_repeat {
        let mut values = matches
            .get_many::<String>(&arg.name)
            .map(|it| it.collect::<Vec<_>>())
            .unwrap_or_default();
        if values.len() > 1 {
            return Err(anyhow!(
                "{}",
                tasty_i18n::t_fmt2(
                    "cli.plugin_cli.flag_repeated",
                    arg.flag
                        .as_deref()
                        .unwrap_or(&arg.name)
                        .trim_start_matches('-'),
                    &values.len().to_string()
                )
            ));
        }
        return Ok(match arg.ty {
            CliArgType::U32 => match values.pop() {
                Some(s) => Some(Value::from(parse_number::<u32>(s, arg)?)),
                None => None,
            },
            CliArgType::I64 => match values.pop() {
                Some(s) => Some(Value::from(parse_number::<i64>(s, arg)?)),
                None => None,
            },
            CliArgType::String => values.pop().map(|s| Value::String(s.clone())),
            // build_arg는 Bool을 항상 SetTrue로 등록한다(reject_repeat 무관) —
            // 여기 도달하면 매니페스트 오설정이니 get_flag로 안전하게 처리.
            CliArgType::Bool => Some(Value::Bool(matches.get_flag(&arg.name))),
        });
    }
    Ok(match arg.ty {
        CliArgType::Bool => Some(Value::Bool(matches.get_flag(&arg.name))),
        CliArgType::U32 => match matches.get_one::<String>(&arg.name) {
            Some(s) => Some(Value::from(parse_number::<u32>(s, arg)?)),
            None => None,
        },
        CliArgType::I64 => match matches.get_one::<String>(&arg.name) {
            Some(s) => Some(Value::from(parse_number::<i64>(s, arg)?)),
            None => None,
        },
        CliArgType::String => matches
            .get_one::<String>(&arg.name)
            .map(|s| Value::String(s.clone())),
    })
}
