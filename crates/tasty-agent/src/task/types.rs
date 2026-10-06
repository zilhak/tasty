//! v2 task 계약의 값 타입 — 선언 형식, 값 검증, 타입 간 대입 호환.
//!
//! 값은 `serde_json::Value` 로 전달한다. int64 는 JSON 정수 토큰만 받는다.
//! serde_json 은 정수 토큰을 i64/u64 로 그대로 보관하므로 Tasty 의 Rust 경로에서는
//! int64 최소·최대와 2^53 을 넘는 정수도 정확히 왕복한다. 소수점·지수 표기는 이미
//! f64 로 읽힌 값이라 원래 정수를 알 수 없으므로, 값이 정수처럼 보여도 거절한다.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// 스키마 중첩 깊이 상한. 이름 있는 타입 참조를 따라간 깊이도 포함한다.
pub const MAX_SCHEMA_DEPTH: usize = 32;
/// 검증하는 값의 중첩 깊이 상한(`json` 타입 값 포함).
pub const MAX_VALUE_DEPTH: usize = 64;
/// 검증하는 값의 직렬화 크기 상한(바이트). task 레코드 전체가 memory 항목 상한
/// (`tasty_memory::MAX_VALUE_BYTES`) 안에 들어가도록 그보다 작게 둔다.
pub const MAX_VALUE_BYTES: usize = 256 * 1024;

/// 값 타입 하나. `nullable` 이면 명시 `null` 도 허용한다.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeSchema {
    pub kind: TypeKind,
    pub nullable: bool,
}

/// [`TypeSchema`] 의 종류.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeKind {
    Boolean,
    Int64,
    /// 유한한 배정밀도 값. `min`/`max` 는 포함 경계다.
    Float64 {
        min: Option<f64>,
        max: Option<f64>,
    },
    /// UTF-8 문자열. `max_len` 은 문자(char) 수 상한이다.
    String {
        max_len: Option<u32>,
    },
    /// 선언된 고유 문자열 값 중 하나.
    Enum {
        values: Vec<String>,
    },
    /// 이름 있는 필드. 선언하지 않은 필드는 거절한다.
    Object {
        fields: BTreeMap<String, FieldSchema>,
    },
    List {
        items: Box<TypeSchema>,
        max_len: Option<u32>,
    },
    /// 의미 있는 값이 없다. 값은 `null` 하나다.
    Unit,
    /// 임의 JSON. 구체 타입으로 쓰려면 명시 projection 과 검증이 필요하다.
    Json,
    /// 같은 계약의 `types` 에 선언한 이름 있는 타입.
    Ref(String),
}

/// object 필드 하나. `optional` 은 필드 부재 허용, `default` 는 부재 시 채울 값이다.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSchema {
    pub schema: TypeSchema,
    pub optional: bool,
    pub default: Option<Value>,
}

impl TypeSchema {
    pub fn new(kind: TypeKind) -> Self {
        Self {
            kind,
            nullable: false,
        }
    }

    pub fn unit() -> Self {
        Self::new(TypeKind::Unit)
    }

    pub fn json() -> Self {
        Self::new(TypeKind::Json)
    }

    pub fn int64() -> Self {
        Self::new(TypeKind::Int64)
    }

    pub fn string() -> Self {
        Self::new(TypeKind::String { max_len: None })
    }

    /// 오류 메시지에 쓰는 짧은 타입 표기.
    pub fn describe(&self) -> String {
        let base = match &self.kind {
            TypeKind::Boolean => "boolean".to_string(),
            TypeKind::Int64 => "int64".to_string(),
            TypeKind::Float64 { min, max } => match (min, max) {
                (None, None) => "float64".to_string(),
                _ => format!(
                    "float64[{}..{}]",
                    min.map(|v| v.to_string()).unwrap_or_default(),
                    max.map(|v| v.to_string()).unwrap_or_default()
                ),
            },
            TypeKind::String { max_len: None } => "string".to_string(),
            TypeKind::String { max_len: Some(n) } => format!("string(max {n})"),
            TypeKind::Enum { values } => format!("enum({})", values.join("|")),
            TypeKind::Object { .. } => "object".to_string(),
            TypeKind::List { items, .. } => format!("list<{}>", items.describe()),
            TypeKind::Unit => "unit".to_string(),
            TypeKind::Json => "json".to_string(),
            TypeKind::Ref(name) => name.clone(),
        };
        if self.nullable {
            format!("{base}?")
        } else {
            base
        }
    }
}

/// 타입 오류의 분류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeErrorKind {
    /// 선언 형식 자체가 잘못됐다.
    InvalidSchema,
    UnknownTypeRef,
    RecursiveType,
    TypeMismatch,
    NotInteger,
    OutOfRange,
    NotFinite,
    UnknownEnumValue,
    NullNotAllowed,
    MissingField,
    UnexpectedField,
    TooLong,
    TooDeep,
    TooLarge,
    /// 대입 호환 검사에서 source 타입을 target 타입으로 받을 수 없다.
    Incompatible,
}

/// 타입 오류. `path` 는 값(또는 스키마) 안의 RFC 6901 JSON Pointer 다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeError {
    pub kind: TypeErrorKind,
    pub path: String,
    pub expected: String,
    pub actual: String,
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let at = if self.path.is_empty() {
            "/"
        } else {
            &self.path
        };
        write!(
            f,
            "{} at {at}: expected {}, got {}",
            kind_label(self.kind),
            self.expected,
            self.actual
        )
    }
}

impl std::error::Error for TypeError {}

fn kind_label(kind: TypeErrorKind) -> &'static str {
    match kind {
        TypeErrorKind::InvalidSchema => "invalid schema",
        TypeErrorKind::UnknownTypeRef => "unknown type",
        TypeErrorKind::RecursiveType => "recursive type",
        TypeErrorKind::TypeMismatch => "type mismatch",
        TypeErrorKind::NotInteger => "not an integer",
        TypeErrorKind::OutOfRange => "out of range",
        TypeErrorKind::NotFinite => "not finite",
        TypeErrorKind::UnknownEnumValue => "unknown enum value",
        TypeErrorKind::NullNotAllowed => "null not allowed",
        TypeErrorKind::MissingField => "missing field",
        TypeErrorKind::UnexpectedField => "unexpected field",
        TypeErrorKind::TooLong => "too long",
        TypeErrorKind::TooDeep => "too deep",
        TypeErrorKind::TooLarge => "too large",
        TypeErrorKind::Incompatible => "incompatible types",
    }
}

fn err(
    kind: TypeErrorKind,
    path: &str,
    expected: impl Into<String>,
    actual: impl Into<String>,
) -> TypeError {
    TypeError {
        kind,
        path: path.to_string(),
        expected: expected.into(),
        actual: actual.into(),
    }
}

fn child_path(path: &str, token: &str) -> String {
    format!("{path}/{}", token.replace('~', "~0").replace('/', "~1"))
}

/// 값의 JSON 종류와 짧은 표기. 오류의 `actual` 에 쓴다.
pub fn describe_value(v: &Value) -> String {
    let kind = match v {
        Value::Null => return "null".to_string(),
        Value::Bool(b) => return format!("boolean {b}"),
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(a) => return format!("array(len {})", a.len()),
        Value::Object(_) => return "object".to_string(),
    };
    let mut text = v.to_string();
    if text.chars().count() > 64 {
        text = text.chars().take(64).collect::<String>() + "…";
    }
    format!("{kind} {text}")
}

// ── 선언 형식 (JSON) ─────────────────────────────────────────────────────────

const SCHEMA_KEYS_COMMON: &[&str] = &["type", "nullable"];
const FIELD_KEYS: &[&str] = &["optional", "default"];

impl TypeSchema {
    /// JSON 선언을 읽는다. 알 수 없는 키·타입은 오류다.
    pub fn from_json(v: &Value) -> Result<Self, TypeError> {
        parse_schema(v, "", 0, false).map(|(s, _)| s)
    }

    /// JSON 선언으로 쓴다. [`Self::from_json`] 과 왕복한다.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        self.write_json(&mut m);
        Value::Object(m)
    }

    fn write_json(&self, m: &mut Map<String, Value>) {
        match &self.kind {
            TypeKind::Ref(name) => {
                m.insert("ref".into(), Value::String(name.clone()));
            }
            kind => {
                let ty = match kind {
                    TypeKind::Boolean => "boolean",
                    TypeKind::Int64 => "int64",
                    TypeKind::Float64 { .. } => "float64",
                    TypeKind::String { .. } => "string",
                    TypeKind::Enum { .. } => "enum",
                    TypeKind::Object { .. } => "object",
                    TypeKind::List { .. } => "list",
                    TypeKind::Unit => "unit",
                    TypeKind::Json => "json",
                    TypeKind::Ref(_) => unreachable!("handled above"),
                };
                m.insert("type".into(), Value::String(ty.into()));
            }
        }
        match &self.kind {
            TypeKind::Float64 { min, max } => {
                if let Some(v) = min {
                    m.insert("min".into(), Value::from(*v));
                }
                if let Some(v) = max {
                    m.insert("max".into(), Value::from(*v));
                }
            }
            TypeKind::String { max_len: Some(n) } => {
                m.insert("max_len".into(), Value::from(*n));
            }
            TypeKind::Enum { values } => {
                m.insert(
                    "values".into(),
                    Value::Array(values.iter().cloned().map(Value::String).collect()),
                );
            }
            TypeKind::Object { fields } => {
                let mut fm = Map::new();
                for (name, field) in fields {
                    let mut f = Map::new();
                    field.schema.write_json(&mut f);
                    if field.optional {
                        f.insert("optional".into(), Value::Bool(true));
                    }
                    if let Some(d) = &field.default {
                        f.insert("default".into(), d.clone());
                    }
                    fm.insert(name.clone(), Value::Object(f));
                }
                m.insert("fields".into(), Value::Object(fm));
            }
            TypeKind::List { items, max_len } => {
                m.insert("items".into(), items.to_json());
                if let Some(n) = max_len {
                    m.insert("max_len".into(), Value::from(*n));
                }
            }
            _ => {}
        }
        if self.nullable {
            m.insert("nullable".into(), Value::Bool(true));
        }
    }
}

impl Serialize for TypeSchema {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(s)
    }
}

impl<'de> Deserialize<'de> for TypeSchema {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        TypeSchema::from_json(&v).map_err(serde::de::Error::custom)
    }
}

fn invalid(path: &str, expected: impl Into<String>, actual: impl Into<String>) -> TypeError {
    err(TypeErrorKind::InvalidSchema, path, expected, actual)
}

fn opt_u32(m: &Map<String, Value>, key: &str, path: &str) -> Result<Option<u32>, TypeError> {
    match m.get(key) {
        None => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| {
                invalid(
                    &child_path(path, key),
                    "unsigned 32-bit integer",
                    describe_value(v),
                )
            }),
    }
}

fn opt_finite(m: &Map<String, Value>, key: &str, path: &str) -> Result<Option<f64>, TypeError> {
    match m.get(key) {
        None => Ok(None),
        Some(v) => match v.as_f64() {
            Some(f) if f.is_finite() => Ok(Some(f)),
            _ => Err(invalid(
                &child_path(path, key),
                "finite number",
                describe_value(v),
            )),
        },
    }
}

/// `(스키마, (optional, default))` 를 읽는다. `as_field` 일 때만 필드 전용 키를 받는다.
fn parse_schema(
    v: &Value,
    path: &str,
    depth: usize,
    as_field: bool,
) -> Result<(TypeSchema, (bool, Option<Value>)), TypeError> {
    if depth > MAX_SCHEMA_DEPTH {
        return Err(err(
            TypeErrorKind::TooDeep,
            path,
            format!("schema depth <= {MAX_SCHEMA_DEPTH}"),
            "deeper schema",
        ));
    }
    let Value::Object(m) = v else {
        return Err(invalid(path, "schema object", describe_value(v)));
    };
    let nullable = match m.get("nullable") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(other) => {
            return Err(invalid(
                &child_path(path, "nullable"),
                "boolean",
                describe_value(other),
            ));
        }
    };
    let (optional, default) = if as_field {
        let optional = match m.get("optional") {
            None => false,
            Some(Value::Bool(b)) => *b,
            Some(other) => {
                return Err(invalid(
                    &child_path(path, "optional"),
                    "boolean",
                    describe_value(other),
                ));
            }
        };
        (optional, m.get("default").cloned())
    } else {
        (false, None)
    };

    let (kind, allowed): (TypeKind, &[&str]) = if let Some(r) = m.get("ref") {
        if m.contains_key("type") {
            return Err(invalid(path, "either 'type' or 'ref'", "both"));
        }
        let Value::String(name) = r else {
            return Err(invalid(
                &child_path(path, "ref"),
                "type name",
                describe_value(r),
            ));
        };
        (TypeKind::Ref(name.clone()), &["ref"])
    } else {
        let ty = match m.get("type") {
            Some(Value::String(s)) => s.as_str(),
            Some(other) => {
                return Err(invalid(
                    &child_path(path, "type"),
                    "type name",
                    describe_value(other),
                ));
            }
            None => return Err(invalid(path, "'type' or 'ref'", "neither")),
        };
        match ty {
            "boolean" => (TypeKind::Boolean, &[]),
            "int64" => (TypeKind::Int64, &[]),
            "float64" => {
                let min = opt_finite(m, "min", path)?;
                let max = opt_finite(m, "max", path)?;
                if let (Some(lo), Some(hi)) = (min, max)
                    && lo > hi
                {
                    return Err(invalid(path, "min <= max", format!("min {lo} > max {hi}")));
                }
                (TypeKind::Float64 { min, max }, &["min", "max"])
            }
            "string" => (
                TypeKind::String {
                    max_len: opt_u32(m, "max_len", path)?,
                },
                &["max_len"],
            ),
            "enum" => {
                let values = parse_enum_values(m, path)?;
                (TypeKind::Enum { values }, &["values"])
            }
            "object" => {
                let fields = parse_fields(m, path, depth)?;
                (TypeKind::Object { fields }, &["fields"])
            }
            "list" => {
                let items_v = m
                    .get("items")
                    .ok_or_else(|| invalid(path, "'items' schema", "missing"))?;
                let (items, _) =
                    parse_schema(items_v, &child_path(path, "items"), depth + 1, false)?;
                (
                    TypeKind::List {
                        items: Box::new(items),
                        max_len: opt_u32(m, "max_len", path)?,
                    },
                    &["items", "max_len"],
                )
            }
            "unit" => (TypeKind::Unit, &[]),
            "json" => (TypeKind::Json, &[]),
            other => {
                return Err(invalid(
                    &child_path(path, "type"),
                    "boolean|int64|float64|string|enum|object|list|unit|json",
                    format!("\"{other}\""),
                ));
            }
        }
    };

    for key in m.keys() {
        let known = SCHEMA_KEYS_COMMON.contains(&key.as_str())
            || allowed.contains(&key.as_str())
            || (as_field && FIELD_KEYS.contains(&key.as_str()));
        if !known {
            return Err(err(
                TypeErrorKind::UnexpectedField,
                &child_path(path, key),
                "a declared schema key",
                format!("\"{key}\""),
            ));
        }
    }
    if nullable && matches!(kind, TypeKind::Unit | TypeKind::Json) {
        // unit·json 은 이미 null 을 받는다. 두 표기가 같은 뜻이 되지 않게 거절한다.
        return Err(invalid(
            &child_path(path, "nullable"),
            "no 'nullable' on unit or json",
            "nullable: true",
        ));
    }
    Ok((TypeSchema { kind, nullable }, (optional, default)))
}

fn parse_enum_values(m: &Map<String, Value>, path: &str) -> Result<Vec<String>, TypeError> {
    let vpath = child_path(path, "values");
    let Some(Value::Array(arr)) = m.get("values") else {
        return Err(invalid(
            &vpath,
            "array of strings",
            "missing or not an array",
        ));
    };
    if arr.is_empty() {
        return Err(invalid(&vpath, "at least one value", "empty array"));
    }
    let mut seen = BTreeSet::new();
    let mut values = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let ipath = child_path(&vpath, &i.to_string());
        let Value::String(s) = item else {
            return Err(invalid(&ipath, "string", describe_value(item)));
        };
        if !seen.insert(s.clone()) {
            return Err(invalid(
                &ipath,
                "unique value",
                format!("duplicate \"{s}\""),
            ));
        }
        values.push(s.clone());
    }
    Ok(values)
}

fn parse_fields(
    m: &Map<String, Value>,
    path: &str,
    depth: usize,
) -> Result<BTreeMap<String, FieldSchema>, TypeError> {
    let fpath = child_path(path, "fields");
    let Some(Value::Object(fm)) = m.get("fields") else {
        return Err(invalid(
            &fpath,
            "object of field schemas",
            "missing or not an object",
        ));
    };
    let mut fields = BTreeMap::new();
    for (name, fv) in fm {
        let (schema, (optional, default)) =
            parse_schema(fv, &child_path(&fpath, name), depth + 1, true)?;
        fields.insert(
            name.clone(),
            FieldSchema {
                schema,
                optional,
                default,
            },
        );
    }
    Ok(fields)
}

// ── 이름 있는 타입 ────────────────────────────────────────────────────────────

/// 한 계약 안의 이름 있는 타입 모음.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TypeDefs {
    pub types: BTreeMap<String, TypeSchema>,
}

impl TypeDefs {
    pub fn new(types: BTreeMap<String, TypeSchema>) -> Self {
        Self { types }
    }

    /// 참조 대상 존재, 비재귀, 깊이 상한, default 값의 유효성을 검사한다.
    pub fn check(&self) -> Result<(), TypeError> {
        for (name, schema) in &self.types {
            let path = child_path("/types", name);
            self.check_schema(schema, &path, &mut vec![name.clone()], 0)?;
        }
        Ok(())
    }

    /// 계약의 입력·출력처럼 이름 밖에서 쓰는 스키마를 검사한다.
    pub fn check_schema_at(&self, schema: &TypeSchema, path: &str) -> Result<(), TypeError> {
        self.check_schema(schema, path, &mut Vec::new(), 0)
    }

    fn check_schema(
        &self,
        schema: &TypeSchema,
        path: &str,
        stack: &mut Vec<String>,
        depth: usize,
    ) -> Result<(), TypeError> {
        if depth > MAX_SCHEMA_DEPTH {
            return Err(err(
                TypeErrorKind::TooDeep,
                path,
                format!("schema depth <= {MAX_SCHEMA_DEPTH}"),
                "deeper schema",
            ));
        }
        match &schema.kind {
            TypeKind::Ref(name) => {
                let Some(target) = self.types.get(name) else {
                    return Err(err(
                        TypeErrorKind::UnknownTypeRef,
                        path,
                        "a type declared in 'types'",
                        format!("\"{name}\""),
                    ));
                };
                if stack.contains(name) {
                    return Err(err(
                        TypeErrorKind::RecursiveType,
                        path,
                        "a non-recursive type",
                        format!("{} -> {name}", stack.join(" -> ")),
                    ));
                }
                stack.push(name.clone());
                let r = self.check_schema(target, path, stack, depth + 1);
                stack.pop();
                r
            }
            TypeKind::Object { fields } => {
                for (fname, field) in fields {
                    let fpath = child_path(&child_path(path, "fields"), fname);
                    self.check_schema(&field.schema, &fpath, stack, depth + 1)?;
                    if let Some(d) = &field.default {
                        self.validate_at(&field.schema, d, &child_path(&fpath, "default"), 0)?;
                    }
                }
                Ok(())
            }
            TypeKind::List { items, .. } => {
                self.check_schema(items, &child_path(path, "items"), stack, depth + 1)
            }
            _ => Ok(()),
        }
    }

    /// 참조를 따라 실제 스키마를 얻는다. `nullable` 은 참조 쪽과 대상 쪽을 합친다.
    pub fn resolve<'a>(&'a self, schema: &'a TypeSchema) -> Result<ResolvedSchema<'a>, TypeError> {
        let mut nullable = schema.nullable;
        let mut cur = schema;
        let mut hops = 0;
        while let TypeKind::Ref(name) = &cur.kind {
            hops += 1;
            if hops > MAX_SCHEMA_DEPTH {
                return Err(err(
                    TypeErrorKind::RecursiveType,
                    "",
                    "a non-recursive type",
                    format!("\"{name}\""),
                ));
            }
            cur = self.types.get(name).ok_or_else(|| {
                err(
                    TypeErrorKind::UnknownTypeRef,
                    "",
                    "a type declared in 'types'",
                    format!("\"{name}\""),
                )
            })?;
            nullable |= cur.nullable;
        }
        Ok(ResolvedSchema {
            kind: &cur.kind,
            nullable,
        })
    }

    /// 값을 검사하고 default 를 채운 값을 반환한다.
    pub fn validate(&self, schema: &TypeSchema, value: &Value) -> Result<Value, TypeError> {
        let size = serde_json::to_vec(value)
            .map(|b| b.len())
            .unwrap_or(usize::MAX);
        if size > MAX_VALUE_BYTES {
            return Err(err(
                TypeErrorKind::TooLarge,
                "",
                format!("serialized value <= {MAX_VALUE_BYTES} bytes"),
                format!("{size} bytes"),
            ));
        }
        self.validate_at(schema, value, "", 0)
    }

    fn validate_at(
        &self,
        schema: &TypeSchema,
        value: &Value,
        path: &str,
        depth: usize,
    ) -> Result<Value, TypeError> {
        if depth > MAX_VALUE_DEPTH {
            return Err(err(
                TypeErrorKind::TooDeep,
                path,
                format!("value depth <= {MAX_VALUE_DEPTH}"),
                "deeper value",
            ));
        }
        let resolved = self.resolve(schema).map_err(|mut e| {
            e.path = path.to_string();
            e
        })?;
        let expected = || schema.describe();
        if value.is_null() && !matches!(resolved.kind, TypeKind::Unit | TypeKind::Json) {
            return if resolved.nullable {
                Ok(Value::Null)
            } else {
                Err(err(TypeErrorKind::NullNotAllowed, path, expected(), "null"))
            };
        }
        let mismatch = || {
            err(
                TypeErrorKind::TypeMismatch,
                path,
                expected(),
                describe_value(value),
            )
        };
        match resolved.kind {
            TypeKind::Boolean => value.as_bool().map(|_| value.clone()).ok_or_else(mismatch),
            TypeKind::Int64 => int64_of(value)
                .map(|i| Value::String(i.to_string()))
                .map_err(|kind| {
                    let want = match kind {
                        TypeErrorKind::OutOfRange => {
                            format!("int64 in [{}, {}]", i64::MIN, i64::MAX)
                        }
                        _ => expected(),
                    };
                    err(kind, path, want, describe_value(value))
                }),
            TypeKind::Float64 { min, max } => {
                let Value::Number(n) = value else {
                    return Err(mismatch());
                };
                let f = float_of(n).ok_or_else(|| {
                    err(
                        TypeErrorKind::OutOfRange,
                        path,
                        "integer exactly representable as float64",
                        describe_value(value),
                    )
                })?;
                if !f.is_finite() {
                    return Err(err(
                        TypeErrorKind::NotFinite,
                        path,
                        expected(),
                        describe_value(value),
                    ));
                }
                if min.is_some_and(|lo| f < lo) || max.is_some_and(|hi| f > hi) {
                    return Err(err(
                        TypeErrorKind::OutOfRange,
                        path,
                        expected(),
                        describe_value(value),
                    ));
                }
                Ok(value.clone())
            }
            TypeKind::String { max_len } => {
                let Value::String(s) = value else {
                    return Err(mismatch());
                };
                if let Some(n) = max_len
                    && s.chars().count() > *n as usize
                {
                    return Err(err(
                        TypeErrorKind::TooLong,
                        path,
                        expected(),
                        format!("string of {} chars", s.chars().count()),
                    ));
                }
                Ok(value.clone())
            }
            TypeKind::Enum { values } => {
                let Value::String(s) = value else {
                    return Err(mismatch());
                };
                if values.iter().any(|v| v == s) {
                    Ok(value.clone())
                } else {
                    Err(err(
                        TypeErrorKind::UnknownEnumValue,
                        path,
                        expected(),
                        describe_value(value),
                    ))
                }
            }
            TypeKind::Object { fields } => {
                let Value::Object(obj) = value else {
                    return Err(mismatch());
                };
                if let Some(extra) = obj.keys().find(|k| !fields.contains_key(*k)) {
                    return Err(err(
                        TypeErrorKind::UnexpectedField,
                        &child_path(path, extra),
                        "no undeclared field",
                        format!("field \"{extra}\""),
                    ));
                }
                let mut out = Map::new();
                for (name, field) in fields {
                    let fpath = child_path(path, name);
                    match obj.get(name) {
                        Some(v) => {
                            out.insert(
                                name.clone(),
                                self.validate_at(&field.schema, v, &fpath, depth + 1)?,
                            );
                        }
                        None => match &field.default {
                            // 기본값도 같은 경로로 정규화한다(int64 는 10진 문자열).
                            Some(d) => {
                                out.insert(
                                    name.clone(),
                                    self.validate_at(&field.schema, d, &fpath, depth + 1)?,
                                );
                            }
                            None if field.optional => {}
                            None => {
                                return Err(err(
                                    TypeErrorKind::MissingField,
                                    &fpath,
                                    field.schema.describe(),
                                    "absent",
                                ));
                            }
                        },
                    }
                }
                Ok(Value::Object(out))
            }
            TypeKind::List { items, max_len } => {
                let Value::Array(arr) = value else {
                    return Err(mismatch());
                };
                if let Some(n) = max_len
                    && arr.len() > *n as usize
                {
                    return Err(err(
                        TypeErrorKind::TooLong,
                        path,
                        expected(),
                        format!("list of {} items", arr.len()),
                    ));
                }
                arr.iter()
                    .enumerate()
                    .map(|(i, v)| {
                        self.validate_at(items, v, &child_path(path, &i.to_string()), depth + 1)
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map(Value::Array)
            }
            TypeKind::Unit => {
                if value.is_null() {
                    Ok(Value::Null)
                } else {
                    Err(mismatch())
                }
            }
            TypeKind::Json => {
                if json_depth(value) > MAX_VALUE_DEPTH.saturating_sub(depth) {
                    return Err(err(
                        TypeErrorKind::TooDeep,
                        path,
                        format!("value depth <= {MAX_VALUE_DEPTH}"),
                        "deeper value",
                    ));
                }
                Ok(value.clone())
            }
            TypeKind::Ref(_) => unreachable!("resolve follows every ref"),
        }
    }
}

/// 참조를 따라간 결과.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedSchema<'a> {
    pub kind: &'a TypeKind,
    pub nullable: bool,
}

/// int64 값을 wire 형식에서 읽는다. 검증을 통과한 값은 10진 문자열이고(JavaScript 의
/// f64 숫자를 지나도 같은 정수로 돌아오게 하기 위해), 입력으로는 JSON 정수 토큰도 받는다.
/// 문자열은 `-?(0|[1-9][0-9]*)` 꼴만 받는다. `-0`·앞자리 0·`+`·공백은 거절한다.
pub fn int64_of(value: &Value) -> Result<i64, TypeErrorKind> {
    match value {
        Value::String(s) => {
            let digits = s.strip_prefix('-').unwrap_or(s);
            let canonical = !digits.is_empty()
                && digits.bytes().all(|b| b.is_ascii_digit())
                && (digits == "0" || !digits.starts_with('0'))
                && s != "-0";
            if canonical {
                s.parse::<i64>().map_err(|_| TypeErrorKind::OutOfRange)
            } else if s
                .parse::<f64>()
                .is_ok_and(|f| f.is_finite() && f.fract() != 0.0)
            {
                Err(TypeErrorKind::NotInteger)
            } else {
                Err(TypeErrorKind::TypeMismatch)
            }
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i)
            } else if n.is_u64() {
                Err(TypeErrorKind::OutOfRange)
            } else {
                // 정수 모양의 큰 f64 는 범위 밖, 나머지 실수 토큰은 정수가 아니다.
                let f = n.as_f64().unwrap_or(f64::NAN);
                if f.fract() == 0.0 && -(i64::MIN as f64) <= f.abs() {
                    Err(TypeErrorKind::OutOfRange)
                } else {
                    Err(TypeErrorKind::NotInteger)
                }
            }
        }
        _ => Err(TypeErrorKind::TypeMismatch),
    }
}

/// 정수 토큰은 f64 로 정확히 표현될 때만 float64 로 받는다.
fn float_of(n: &serde_json::Number) -> Option<f64> {
    if let Some(i) = n.as_i64() {
        let f = i as f64;
        // f64 로 바꿨다가 넓은 정수로 되돌려 반올림 여부를 본다.
        return (f as i128 == i as i128).then_some(f);
    }
    if let Some(u) = n.as_u64() {
        let f = u as f64;
        return (f as u128 == u as u128).then_some(f);
    }
    n.as_f64()
}

fn json_depth(v: &Value) -> usize {
    // 재귀 대신 명시 스택으로 깊이를 잰다. 깊은 값이 스택을 넘치지 않게 한다.
    let mut max = 0;
    let mut stack = vec![(v, 1usize)];
    while let Some((cur, d)) = stack.pop() {
        max = max.max(d);
        match cur {
            Value::Array(a) => stack.extend(a.iter().map(|x| (x, d + 1))),
            Value::Object(o) => stack.extend(o.values().map(|x| (x, d + 1))),
            _ => {}
        }
    }
    max
}

// ── 대입 호환 ────────────────────────────────────────────────────────────────

mod assign;
pub use assign::check_assignable;

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
