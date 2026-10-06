//! 타입이 있는 값의 내부 표현과 wire 표현 사이의 변환.
//!
//! 메모리 안의 값은 내부 표현이다(int64 는 JSON 정수). 저장·IPC·CLI 처럼 직렬화하는
//! 경계에서만 int64 를 10진 문자열로 쓴다. JavaScript 처럼 숫자를 f64 로 읽는 소비자를
//! 지나도 i64 전 범위가 같은 정수로 돌아오게 하기 위해서다. 스키마가 위치를 알려 주므로
//! string 타입 값과 섞이지 않는다. `json` 타입 안의 숫자는 바꾸지 않는다.

use super::*;

impl TypeDefs {
    /// 내부 표현을 wire 표현으로 바꾼다. 검증을 통과한 값을 받으므로 실패하지 않는다.
    /// 스키마와 맞지 않는 자리는 그대로 둔다.
    pub fn encode_wire(&self, schema: &TypeSchema, value: &Value) -> Value {
        self.convert(schema, value, Direction::Encode, "")
            .unwrap_or_else(|_| value.clone())
    }

    /// wire 표현을 내부 표현으로 바꾼다. int64 자리에 정수가 아닌 값이 있으면 오류다.
    /// 크기·필드 검사는 하지 않는다. 저장된 값은 확정할 때 이미 검증했다.
    pub fn decode_wire(&self, schema: &TypeSchema, value: &Value) -> Result<Value, TypeError> {
        self.convert(schema, value, Direction::Decode, "")
    }

    fn convert(
        &self,
        schema: &TypeSchema,
        value: &Value,
        dir: Direction,
        path: &str,
    ) -> Result<Value, TypeError> {
        if value.is_null() {
            return Ok(Value::Null);
        }
        let resolved = self.resolve(schema)?;
        match (resolved.kind, value) {
            (TypeKind::Int64, _) => match (dir, int64_of(value)) {
                (Direction::Encode, Ok(i)) => Ok(Value::String(i.to_string())),
                (Direction::Encode, Err(_)) => Ok(value.clone()),
                (Direction::Decode, Ok(i)) => Ok(Value::from(i)),
                (Direction::Decode, Err(kind)) => {
                    Err(err(kind, path, "int64", describe_value(value)))
                }
            },
            (TypeKind::Object { fields }, Value::Object(map)) => map
                .iter()
                .map(|(name, v)| {
                    let v = match fields.get(name) {
                        Some(field) => {
                            self.convert(&field.schema, v, dir, &child_path(path, name))?
                        }
                        None => v.clone(),
                    };
                    Ok((name.clone(), v))
                })
                .collect::<Result<Map<String, Value>, TypeError>>()
                .map(Value::Object),
            (TypeKind::List { items, .. }, Value::Array(arr)) => arr
                .iter()
                .enumerate()
                .map(|(i, v)| self.convert(items, v, dir, &child_path(path, &i.to_string())))
                .collect::<Result<Vec<Value>, TypeError>>()
                .map(Value::Array),
            _ => Ok(value.clone()),
        }
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Encode,
    Decode,
}
