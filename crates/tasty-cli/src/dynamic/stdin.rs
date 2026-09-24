//! CLI 인자의 타입을 확인하고 stdin JSON을 병합한다.

use anyhow::{Result, anyhow};
use clap::ArgMatches;
use serde_json::{Map, Value};

use tasty_plugin_manifest::{CliArg, CliArgGroup, CliArgType};

/// stdin JSON을 읽지 못한 이유. TTY 입력과 오류를 구분한다.
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
/// stdin 경로도 숫자 타입을 검사해 잘못된 값을 인자 부재로 처리하지 않는다.
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

/// 숫자 타입은 플래그와 같은 규칙으로 검사한다. string/bool은 추가 검사 없이 전달한다.
pub(super) fn coerce_stdin_value(v: &Value, arg: &CliArg) -> Result<Value> {
    match arg.ty {
        CliArgType::U32 => coerce_stdin_number::<u32>(v, arg),
        CliArgType::I64 => coerce_stdin_number::<i64>(v, arg),
        CliArgType::String | CliArgType::Bool => Ok(v.clone()),
    }
}

/// 숫자 검사를 공용 parse_number에 맡겨 플래그와 같은 오류를 반환한다.
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

/// 값이 있는데 파싱하지 못하면 오류다. None으로 바꾸면 기본 대상으로 잘못 실행할 수 있다.
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
        // i128로 다시 읽어 정수의 범위 초과와 비수치 입력을 구분한다.
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
