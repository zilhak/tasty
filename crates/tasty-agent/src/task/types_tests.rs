use serde_json::{Value, json};

use super::*;

fn schema(v: Value) -> TypeSchema {
    TypeSchema::from_json(&v).expect("schema")
}

fn check(s: &TypeSchema, v: Value) -> Result<Value, TypeError> {
    TypeDefs::default().validate(s, &v)
}

fn kind_of(r: Result<Value, TypeError>) -> TypeErrorKind {
    r.expect_err("expected a type error").kind
}

fn parsed(text: &str) -> Value {
    serde_json::from_str(text).expect("json")
}

#[test]
fn int64_reads_integer_tokens_and_decimal_strings_and_rejects_fractions_and_overflow() {
    let s = TypeSchema::int64();
    // 검증된 int64 는 10진 문자열로 정규화된다.
    assert_eq!(check(&s, json!(42)).unwrap(), json!("42"));
    assert_eq!(check(&s, json!("42")).unwrap(), json!("42"));
    assert_eq!(check(&s, json!("-7")).unwrap(), json!("-7"));
    assert_eq!(check(&s, json!("0")).unwrap(), json!("0"));
    assert_eq!(kind_of(check(&s, json!(1.5))), TypeErrorKind::NotInteger);
    assert_eq!(kind_of(check(&s, json!("1.5"))), TypeErrorKind::NotInteger);
    // 정수처럼 보여도 f64 로 읽힌 값은 원래 정수를 알 수 없다.
    assert_eq!(
        kind_of(check(&s, parsed("42.0"))),
        TypeErrorKind::NotInteger
    );
    for odd in [
        "", "-", "-0", "007", "+5", " 5", "5 ", "1e3", "0x10", "forty",
    ] {
        assert_eq!(
            kind_of(check(&s, json!(odd))),
            TypeErrorKind::TypeMismatch,
            "{odd:?}"
        );
    }
    assert_eq!(
        kind_of(check(&s, parsed("9223372036854775808"))),
        TypeErrorKind::OutOfRange
    );
    assert_eq!(
        kind_of(check(&s, json!("9223372036854775808"))),
        TypeErrorKind::OutOfRange
    );
    assert_eq!(
        kind_of(check(&s, json!("-9223372036854775809"))),
        TypeErrorKind::OutOfRange
    );
    assert_eq!(
        kind_of(check(&s, parsed("100000000000000000000"))),
        TypeErrorKind::OutOfRange
    );
    assert_eq!(
        kind_of(check(&s, Value::Null)),
        TypeErrorKind::NullNotAllowed
    );
}

#[test]
fn int64_boundaries_and_beyond_2_pow_53_keep_their_value_as_decimal_strings() {
    let s = TypeSchema::int64();
    for (text, n) in [
        ("-9223372036854775808", i64::MIN),
        ("9223372036854775807", i64::MAX),
        ("9007199254740993", 9_007_199_254_740_993),
    ] {
        // 정수 토큰으로 받아도, 문자열로 받아도 같은 wire 값이 된다.
        let from_token = check(&s, parsed(text)).unwrap();
        let from_string = check(&s, json!(text)).unwrap();
        assert_eq!(from_token, json!(text));
        assert_eq!(from_string, from_token);
        // wire 값을 f64 숫자로 읽는 소비자를 거쳐도 문자열은 바뀌지 않는다.
        let wire = serde_json::to_string(&from_token).unwrap();
        assert_eq!(wire, format!("\"{text}\""));
        assert_eq!(int64_of(&parsed(&wire)), Ok(n));
    }
}

#[test]
fn float64_rejects_non_finite_inexact_integers_and_out_of_range_values() {
    let s = schema(json!({"type": "float64", "min": 0, "max": 1}));
    assert_eq!(check(&s, json!(0.5)).unwrap(), json!(0.5));
    assert_eq!(check(&s, json!(1)).unwrap(), json!(1));
    assert_eq!(kind_of(check(&s, json!(1.01))), TypeErrorKind::OutOfRange);
    // JSON 문서에는 NaN·Infinity 를 쓸 수 없고, Rust 쪽 f64 의 NaN·Infinity 는 null 이 된다.
    assert!(serde_json::from_str::<Value>("NaN").is_err());
    assert!(serde_json::from_str::<Value>("Infinity").is_err());
    assert_eq!(Value::from(f64::NAN), Value::Null);
    assert_eq!(
        kind_of(check(&s, Value::from(f64::INFINITY))),
        TypeErrorKind::NullNotAllowed
    );
    let unbounded = schema(json!({"type": "float64"}));
    // 2^53 + 1 은 f64 로 정확히 표현되지 않으므로 조용히 반올림하지 않는다.
    assert_eq!(
        kind_of(check(&unbounded, parsed("9007199254740993"))),
        TypeErrorKind::OutOfRange
    );
    assert!(check(&unbounded, parsed("9007199254740992")).is_ok());
}

#[test]
fn boolean_and_string_do_not_convert_between_each_other() {
    let b = schema(json!({"type": "boolean"}));
    assert_eq!(check(&b, json!(false)).unwrap(), json!(false));
    assert_eq!(
        kind_of(check(&b, json!("false"))),
        TypeErrorKind::TypeMismatch
    );
    let s = schema(json!({"type": "string", "max_len": 3}));
    assert!(check(&s, json!("한글세")).is_ok());
    assert_eq!(kind_of(check(&s, json!("abcd"))), TypeErrorKind::TooLong);
    assert_eq!(kind_of(check(&s, json!(1))), TypeErrorKind::TypeMismatch);
}

#[test]
fn enum_accepts_declared_values_only() {
    let s = schema(json!({"type": "enum", "values": ["pass", "revise", "review"]}));
    assert_eq!(check(&s, json!("revise")).unwrap(), json!("revise"));
    let e = check(&s, json!("unknown")).unwrap_err();
    assert_eq!(e.kind, TypeErrorKind::UnknownEnumValue);
    assert_eq!(e.expected, "enum(pass|revise|review)");
    assert_eq!(e.actual, "string \"unknown\"");
}

fn review_object() -> TypeSchema {
    schema(json!({
        "type": "object",
        "fields": {
            "verdict": {"type": "enum", "values": ["pass", "revise", "review"]},
            "note": {"type": "string", "optional": true},
            "reviewer": {"type": "string", "nullable": true},
            "round": {"type": "int64", "default": 1},
        }
    }))
}

#[test]
fn object_distinguishes_required_optional_nullable_and_default() {
    let s = review_object();
    let v = check(&s, json!({"verdict": "pass", "reviewer": null})).unwrap();
    // optional 은 빠진 채로 두고, default 는 채운다. nullable 의 null 은 그대로다.
    assert_eq!(
        v,
        json!({"verdict": "pass", "reviewer": null, "round": "1"})
    );

    let missing = check(&s, json!({"reviewer": null})).unwrap_err();
    assert_eq!(missing.kind, TypeErrorKind::MissingField);
    assert_eq!(missing.path, "/verdict");

    let null_required = check(&s, json!({"verdict": null, "reviewer": null})).unwrap_err();
    assert_eq!(null_required.kind, TypeErrorKind::NullNotAllowed);
    assert_eq!(null_required.path, "/verdict");

    // nullable 은 부재를 허용하지 않는다.
    let absent_nullable = check(&s, json!({"verdict": "pass"})).unwrap_err();
    assert_eq!(absent_nullable.kind, TypeErrorKind::MissingField);
    assert_eq!(absent_nullable.path, "/reviewer");

    let extra = check(&s, json!({"verdict": "pass", "reviewer": null, "x": 1})).unwrap_err();
    assert_eq!(extra.kind, TypeErrorKind::UnexpectedField);
    assert_eq!(extra.path, "/x");

    let bad_nested = check(&s, json!({"verdict": "maybe", "reviewer": null})).unwrap_err();
    assert_eq!(bad_nested.kind, TypeErrorKind::UnknownEnumValue);
    assert_eq!(bad_nested.path, "/verdict");
}

#[test]
fn list_checks_items_and_length_with_item_paths() {
    let s = schema(json!({"type": "list", "items": {"type": "int64"}, "max_len": 2}));
    assert!(check(&s, json!([1, 2])).is_ok());
    assert_eq!(kind_of(check(&s, json!([1, 2, 3]))), TypeErrorKind::TooLong);
    let e = check(&s, json!([1, true])).unwrap_err();
    assert_eq!(e.path, "/1");
    assert_eq!(e.kind, TypeErrorKind::TypeMismatch);
}

#[test]
fn unit_is_null_only_and_json_takes_anything() {
    assert_eq!(
        check(&TypeSchema::unit(), Value::Null).unwrap(),
        Value::Null
    );
    assert_eq!(
        kind_of(check(&TypeSchema::unit(), json!(false))),
        TypeErrorKind::TypeMismatch
    );
    for v in [json!(null), json!(1), json!("x"), json!({"a": [1]})] {
        assert_eq!(check(&TypeSchema::json(), v.clone()).unwrap(), v);
    }
}

#[test]
fn value_size_and_depth_limits_are_reported() {
    let big = Value::String("x".repeat(MAX_VALUE_BYTES + 1));
    assert_eq!(
        kind_of(check(&TypeSchema::json(), big)),
        TypeErrorKind::TooLarge
    );
    let mut deep = json!(1);
    for _ in 0..(MAX_VALUE_DEPTH + 2) {
        deep = json!([deep]);
    }
    assert_eq!(
        kind_of(check(&TypeSchema::json(), deep)),
        TypeErrorKind::TooDeep
    );
}

#[test]
fn schema_parsing_rejects_unknown_keys_types_and_ambiguous_forms() {
    let bad = [
        json!({"type": "int64", "max": 3}),
        json!({"type": "integer"}),
        json!({"type": "enum", "values": ["a", "a"]}),
        json!({"type": "enum", "values": []}),
        json!({"type": "unit", "nullable": true}),
        json!({"type": "float64", "min": 2, "max": 1}),
        json!({"type": "string", "ref": "X"}),
        json!({"type": "string", "optional": true}),
        json!("int64"),
    ];
    for v in bad {
        assert!(TypeSchema::from_json(&v).is_err(), "accepted {v}");
    }
}

#[test]
fn schema_json_round_trips() {
    for v in [
        json!({"type": "float64", "min": 0.0, "max": 1.0, "nullable": true}),
        json!({"ref": "ReviewResult"}),
        json!({"type": "list", "items": {"type": "string", "max_len": 4}, "max_len": 3}),
        review_object().to_json(),
    ] {
        let s = schema(v.clone());
        assert_eq!(TypeSchema::from_json(&s.to_json()).unwrap(), s);
    }
}

#[test]
fn named_types_resolve_and_reject_unknown_and_recursive_refs() {
    let mut types = std::collections::BTreeMap::new();
    types.insert("Review".to_string(), review_object());
    let defs = TypeDefs::new(types);
    defs.check().unwrap();
    let r = schema(json!({"ref": "Review"}));
    assert!(
        defs.validate(&r, &json!({"verdict": "review", "reviewer": "a"}))
            .is_ok()
    );

    let unknown = defs.check_schema_at(&schema(json!({"ref": "Nope"})), "/output_schema");
    assert_eq!(unknown.unwrap_err().kind, TypeErrorKind::UnknownTypeRef);

    let mut cyclic = std::collections::BTreeMap::new();
    cyclic.insert(
        "A".to_string(),
        schema(json!({"type": "object", "fields": {"b": {"ref": "B"}}})),
    );
    cyclic.insert(
        "B".to_string(),
        schema(json!({"type": "list", "items": {"ref": "A"}})),
    );
    assert_eq!(
        TypeDefs::new(cyclic).check().unwrap_err().kind,
        TypeErrorKind::RecursiveType
    );
}

#[test]
fn defaults_are_checked_at_declaration() {
    let s = schema(json!({
        "type": "object",
        "fields": {"n": {"type": "int64", "default": "one"}}
    }));
    let e = TypeDefs::default()
        .check_schema_at(&s, "/input_schema")
        .unwrap_err();
    assert_eq!(e.kind, TypeErrorKind::TypeMismatch);
    assert_eq!(e.path, "/input_schema/fields/n/default");
}

fn assignable(src: Value, dst: Value) -> Result<(), TypeError> {
    check_assignable(
        &TypeDefs::default(),
        &schema(src),
        &TypeDefs::default(),
        &schema(dst),
    )
}

#[test]
fn assignability_requires_exact_structure_without_hidden_conversion() {
    assert!(assignable(json!({"type": "int64"}), json!({"type": "int64"})).is_ok());
    assert!(assignable(json!({"type": "int64"}), json!({"type": "json"})).is_ok());
    // json 을 구체 타입으로 받으려면 명시 projection 과 검증이 필요하다.
    assert!(assignable(json!({"type": "json"}), json!({"type": "int64"})).is_err());
    // int64 -> float64 처럼 변환이 필요한 대입은 자동으로 하지 않는다.
    assert!(assignable(json!({"type": "int64"}), json!({"type": "float64"})).is_err());
    assert!(
        assignable(
            json!({"type": "enum", "values": ["pass"]}),
            json!({"type": "enum", "values": ["pass", "revise"]})
        )
        .is_ok()
    );
    assert!(
        assignable(
            json!({"type": "enum", "values": ["pass", "other"]}),
            json!({"type": "enum", "values": ["pass", "revise"]})
        )
        .is_err()
    );
    assert!(
        assignable(
            json!({"type": "string", "nullable": true}),
            json!({"type": "string"})
        )
        .is_err()
    );
    assert!(
        assignable(
            json!({"type": "float64", "min": 0, "max": 1}),
            json!({"type": "float64", "min": -1})
        )
        .is_ok()
    );
    assert!(
        assignable(
            json!({"type": "float64"}),
            json!({"type": "float64", "max": 1})
        )
        .is_err()
    );
}

#[test]
fn object_assignability_does_not_drop_or_invent_fields() {
    let target = json!({"type": "object", "fields": {"a": {"type": "int64"}}});
    let extra =
        json!({"type": "object", "fields": {"a": {"type": "int64"}, "b": {"type": "int64"}}});
    let e = assignable(extra, target.clone()).unwrap_err();
    assert_eq!(e.kind, TypeErrorKind::UnexpectedField);
    assert_eq!(e.path, "/b");
    let optional = json!({"type": "object", "fields": {"a": {"type": "int64", "optional": true}}});
    assert_eq!(
        assignable(optional, target.clone()).unwrap_err().kind,
        TypeErrorKind::MissingField
    );
    let defaulted = json!({"type": "object", "fields": {"a": {"type": "int64", "optional": true, "default": 0}}});
    assert!(assignable(defaulted, target).is_ok());
}
