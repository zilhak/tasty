//! 타입이 있는 값.
//!
//! 메모리 안에서는 선언 타입 그대로 든다(int64 는 i64). 직렬화하면 항상 wire 형식이
//! 되어 int64 는 10진 문자열로 쓴다. JavaScript 처럼 숫자를 f64 로 읽는 소비자를 지나도
//! i64 전 범위가 같은 정수로 돌아오게 하기 위해서다(proto3 JSON 관례). 어떤 경로로
//! 직렬화해도 같은 형식이 나오도록 변환을 이 타입의 `Serialize` 에 둔다.
//!
//! wire 형식만으로는 int64 와 string 을 구별할 수 없으므로 역직렬화에는 스키마가
//! 필요하다. [`TypedValueSeed`] 가 스키마를 받는 역직렬화다. `json` 타입 값은 무타입
//! JSON 이라 안의 숫자를 바꾸지 않는다.

use serde::de::{DeserializeSeed, Error as _};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserializer, Serializer};

use super::*;

/// 선언 타입을 아는 값.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum TypedValue {
    /// null(unit, nullable 의 null).
    #[default]
    Null,
    Bool(bool),
    Int64(i64),
    Float64(serde_json::Number),
    /// string 과 enum 값.
    String(String),
    List(Vec<TypedValue>),
    Object(BTreeMap<String, TypedValue>),
    /// `json` 타입 값. 무타입이라 그대로 둔다.
    Json(Value),
}

impl Serialize for TypedValue {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            TypedValue::Null => s.serialize_unit(),
            TypedValue::Bool(b) => s.serialize_bool(*b),
            TypedValue::Int64(i) => s.serialize_str(&i.to_string()),
            TypedValue::Float64(n) => n.serialize(s),
            TypedValue::String(v) => s.serialize_str(v),
            TypedValue::List(items) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            TypedValue::Object(fields) => {
                let mut map = s.serialize_map(Some(fields.len()))?;
                for (k, v) in fields {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
            TypedValue::Json(v) => v.serialize(s),
        }
    }
}

impl TypedValue {
    /// wire 형식의 JSON. `serde_json::to_value(self)` 와 같다.
    pub fn to_wire(&self) -> Value {
        match self {
            TypedValue::Int64(i) => Value::String(i.to_string()),
            TypedValue::List(items) => Value::Array(items.iter().map(Self::to_wire).collect()),
            TypedValue::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_wire()))
                    .collect(),
            ),
            other => other.to_internal(),
        }
    }

    /// int64 를 JSON 정수로 둔 JSON. reducer 처럼 값을 JSON 으로 다루는 내부 계산용이다.
    pub fn to_internal(&self) -> Value {
        match self {
            TypedValue::Null => Value::Null,
            TypedValue::Bool(b) => Value::Bool(*b),
            TypedValue::Int64(i) => Value::from(*i),
            TypedValue::Float64(n) => Value::Number(n.clone()),
            TypedValue::String(v) => Value::String(v.clone()),
            TypedValue::List(items) => Value::Array(items.iter().map(Self::to_internal).collect()),
            TypedValue::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_internal()))
                    .collect(),
            ),
            TypedValue::Json(v) => v.clone(),
        }
    }
}

impl TypeDefs {
    /// 값을 검증하고(기본값 적용 포함) typed 값으로 만든다.
    pub fn validate_typed(
        &self,
        schema: &TypeSchema,
        value: &Value,
    ) -> Result<TypedValue, TypeError> {
        let checked = self.validate(schema, value)?;
        self.typed_value(schema, &checked)
    }

    /// 스키마를 따라 typed 값을 만든다. int64 자리는 정수 토큰과 10진 문자열을 모두
    /// 받는다. 범위·길이·필수 필드는 검사하지 않는다. 검증을 통과한 값이나 저장된
    /// wire 값을 받는 용도다.
    pub fn typed_value(&self, schema: &TypeSchema, value: &Value) -> Result<TypedValue, TypeError> {
        self.typed_at(schema, value, "")
    }

    fn typed_at(
        &self,
        schema: &TypeSchema,
        value: &Value,
        path: &str,
    ) -> Result<TypedValue, TypeError> {
        if value.is_null() {
            return Ok(TypedValue::Null);
        }
        let resolved = self.resolve(schema)?;
        let mismatch = || {
            err(
                TypeErrorKind::TypeMismatch,
                path,
                schema.describe(),
                describe_value(value),
            )
        };
        Ok(match (resolved.kind, value) {
            (TypeKind::Boolean, Value::Bool(b)) => TypedValue::Bool(*b),
            (TypeKind::Int64, _) => TypedValue::Int64(
                int64_of(value).map_err(|kind| err(kind, path, "int64", describe_value(value)))?,
            ),
            (TypeKind::Float64 { .. }, Value::Number(n)) => TypedValue::Float64(n.clone()),
            (TypeKind::String { .. } | TypeKind::Enum { .. }, Value::String(s)) => {
                TypedValue::String(s.clone())
            }
            (TypeKind::Json, _) => TypedValue::Json(value.clone()),
            (TypeKind::List { items, .. }, Value::Array(arr)) => TypedValue::List(
                arr.iter()
                    .enumerate()
                    .map(|(i, v)| self.typed_at(items, v, &child_path(path, &i.to_string())))
                    .collect::<Result<_, _>>()?,
            ),
            (TypeKind::Object { fields }, Value::Object(map)) => TypedValue::Object(
                map.iter()
                    .map(|(name, v)| {
                        let fpath = child_path(path, name);
                        let field = fields.get(name).ok_or_else(|| {
                            err(
                                TypeErrorKind::UnexpectedField,
                                &fpath,
                                "a declared field",
                                describe_value(v),
                            )
                        })?;
                        Ok((name.clone(), self.typed_at(&field.schema, v, &fpath)?))
                    })
                    .collect::<Result<_, TypeError>>()?,
            ),
            _ => return Err(mismatch()),
        })
    }
}

/// 스키마를 받는 [`TypedValue`] 역직렬화.
pub struct TypedValueSeed<'a> {
    pub defs: &'a TypeDefs,
    pub schema: &'a TypeSchema,
}

impl<'de> DeserializeSeed<'de> for TypedValueSeed<'_> {
    type Value = TypedValue;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<TypedValue, D::Error> {
        let raw = Value::deserialize(d)?;
        self.defs
            .typed_value(self.schema, &raw)
            .map_err(D::Error::custom)
    }
}
