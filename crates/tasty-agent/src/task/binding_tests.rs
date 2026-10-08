use serde_json::json;

use super::*;

fn binding(v: Value) -> Result<InputBinding, serde_json::Error> {
    serde_json::from_value(v)
}

#[test]
fn bindings_read_three_forms_and_reject_unknown_or_mixed_fields() {
    assert_eq!(
        binding(json!({"literal": null})).unwrap(),
        InputBinding::Literal(Value::Null)
    );
    let from =
        binding(json!({"from_task": "stage.one", "pointer": "/a~1b", "convert": "to_string"}))
            .unwrap();
    assert_eq!(
        from,
        InputBinding::FromTask {
            source: BindingSource {
                from_task: "stage.one".into(),
                pointer: "/a~1b".into()
            },
            convert: Some(Conversion::ToString),
        }
    );
    // 직렬화와 왕복한다.
    assert_eq!(binding(serde_json::to_value(&from).unwrap()).unwrap(), from);
    let one_of = binding(json!({"one_of": [{"from_task": "main"}, {"from_task": "fb"}]})).unwrap();
    assert_eq!(one_of.sources().len(), 2);
    assert_eq!(
        binding(serde_json::to_value(&one_of).unwrap()).unwrap(),
        one_of
    );

    assert!(binding(json!({"from_task": "a", "extra": 1})).is_err());
    assert!(binding(json!({"literal": 1, "convert": "assert"})).is_err());
    assert!(binding(json!({"pointer": "/a"})).is_err());
    assert!(binding(json!({"from_task": 3})).is_err());
}

#[test]
fn pointer_tokens_unescape_rfc6901_and_reject_bad_escapes() {
    assert_eq!(pointer_tokens("").unwrap(), Vec::<String>::new());
    assert_eq!(pointer_tokens("/a~1b").unwrap(), vec!["a/b"]);
    assert_eq!(pointer_tokens("/a~0b").unwrap(), vec!["a~b"]);
    assert_eq!(pointer_tokens("/x/0").unwrap(), vec!["x", "0"]);
    assert!(pointer_tokens("a").is_err());
    assert!(pointer_tokens("/a~2").is_err());
    assert!(pointer_tokens("/a~").is_err());
}

#[test]
fn schema_at_walks_fields_lists_and_json_and_reports_absence() {
    let defs = TypeDefs::default();
    let s = TypeSchema::from_json(&json!({"type": "object", "fields": {
        "count": {"type": "int64"},
        "note": {"type": "string", "optional": true},
        "kept": {"type": "string", "optional": true, "default": "x"},
        "rows": {"type": "list", "items": {"type": "boolean"}},
        "raw": {"type": "json"}
    }}))
    .unwrap();
    assert_eq!(
        schema_at(&defs, &s, "/count").unwrap(),
        (TypeSchema::int64(), false)
    );
    assert!(schema_at(&defs, &s, "/note").unwrap().1);
    assert!(!schema_at(&defs, &s, "/kept").unwrap().1);
    assert_eq!(
        schema_at(&defs, &s, "/rows/3").unwrap().0,
        TypeSchema::new(TypeKind::Boolean)
    );
    assert_eq!(
        schema_at(&defs, &s, "/raw/any/thing").unwrap().0,
        TypeSchema::json()
    );
    let e = schema_at(&defs, &s, "/missing").unwrap_err();
    assert_eq!(e.kind, TypeErrorKind::MissingField);
    assert_eq!(e.path, "/missing");
    assert_eq!(
        schema_at(&defs, &s, "/count/x").unwrap_err().kind,
        TypeErrorKind::TypeMismatch
    );
    assert!(schema_at(&defs, &s, "/rows/01").is_err());
}

#[test]
fn conversions_are_exact_and_refuse_lossy_values() {
    assert_eq!(
        convert_value(Some(Conversion::ToString), json!(9007199254740993_i64)).unwrap(),
        json!("9007199254740993")
    );
    assert_eq!(
        convert_value(Some(Conversion::ToString), json!(false)).unwrap(),
        json!("false")
    );
    assert!(convert_value(Some(Conversion::ToString), json!(1.5)).is_err());
    assert_eq!(
        convert_value(Some(Conversion::Int64ToFloat64), json!(3)).unwrap(),
        json!(3.0)
    );
    let e = convert_value(
        Some(Conversion::Int64ToFloat64),
        json!(9007199254740993_i64),
    )
    .unwrap_err();
    assert_eq!(e.kind, TypeErrorKind::OutOfRange);
    // assert 는 값을 바꾸지 않는다. 검증은 입력 스키마가 한다.
    assert_eq!(
        convert_value(Some(Conversion::Assert), json!({"k": 1})).unwrap(),
        json!({"k": 1})
    );
}

/// 권한이 없어 열 수 없는 파일은 run 입력 파일로 받지 않는다.
#[cfg(unix)]
#[test]
fn an_unreadable_file_is_not_a_file_argument() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("secret");
    std::fs::write(&f, "x").unwrap();
    assert!(check_readable_file(f.to_str().unwrap(), None).is_ok());
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o000)).unwrap();
    // root 는 권한과 관계없이 열 수 있어 이 경우를 만들 수 없다.
    if std::fs::File::open(&f).is_ok() {
        return;
    }
    let why = check_readable_file(f.to_str().unwrap(), None).unwrap_err();
    assert!(why.starts_with("cannot be read"), "{why}");
    let rel = check_readable_file("secret", Some(dir.path())).unwrap_err();
    assert!(rel.starts_with("cannot be read"), "{rel}");
}
