use super::*;

fn check(src: &str) -> FileClass {
    classify(
        &mask_non_code(src).lines().collect::<Vec<_>>(),
        &mask_literals(src).lines().collect::<Vec<_>>(),
        &src.lines().collect::<Vec<_>>(),
    )
}

#[test]
fn same_name_in_other_functions_does_not_supply_a_counter() {
    let counter = r#"fn first() {
    let dir = std::env::temp_dir().join(format!("{}-{}", process::id(), NEXT.fetch_add(1, Relaxed)));
}"#;
    let pid = r#"fn second() {
    let dir = std::env::temp_dir().join(format!("{}", process::id()));
}"#;
    for src in [format!("{counter}\n{pid}"), format!("{pid}\n{counter}")] {
        let c = check(&src);
        assert_eq!(c.sites.len(), 2);
        assert_eq!(c.recall_blind.len(), 1);
        assert!(c.silent.is_empty());
    }
}

#[test]
fn a_neighboring_function_does_not_hide_a_fixed_path() {
    let c = check(
        r#"fn fixed() {
    let dir = std::env::temp_dir().join("fixed");
}
fn unique() {
    let dir = std::env::temp_dir().join(format!("{}-{}", process::id(), NEXT.fetch_add(1, Relaxed)));
}"#,
    );
    assert_eq!(c.sites.len(), 2);
    assert_eq!(c.silent.len(), 1);
}

#[test]
fn a_later_binding_does_not_change_an_earlier_reference() {
    let c = check(
        r#"fn f() {
    let key = "fixed";
    let dir = std::env::temp_dir().join(format!("{key}-{}", process::id()));







    let key = NEXT.fetch_add(1, Relaxed);
}"#,
    );
    assert_eq!(c.recall_blind.len(), 1);
}

#[test]
fn a_plain_shadow_hides_a_counter_binding() {
    let c = check(
        r#"fn f() {
    let key = NEXT.fetch_add(1, Relaxed);
    let key = "fixed";
    let dir = std::env::temp_dir().join(format!("{key}-{}", process::id()));
}"#,
    );
    assert_eq!(c.recall_blind.len(), 1);
}

#[test]
fn leaving_a_block_drops_its_binding_and_restores_the_outer_one() {
    let c = check(
        r#"fn f() {
    let key = format!("{}-{}", process::id(), NEXT.fetch_add(1, Relaxed));
    {
        let key = "fixed";
        let inner = std::env::temp_dir().join(format!("{key}-{}", process::id()));
    }
    let outer = std::env::temp_dir().join(format!("{key}"));
}"#,
    );
    assert_eq!(c.sites.len(), 2);
    assert_eq!(c.recall_blind, vec![4]);
    assert!(c.silent.is_empty());
}

#[test]
fn a_sibling_block_cannot_borrow_a_counter() {
    let c = check(
        r#"fn f() {
    {
        let key = NEXT.fetch_add(1, Relaxed);
    }
    {
        let key = "fixed";
        let dir = std::env::temp_dir().join(format!("{key}-{}", process::id()));
    }
}"#,
    );
    assert_eq!(c.recall_blind.len(), 1);
}

#[test]
fn nested_functions_do_not_capture_outer_bindings() {
    let c = check(
        r#"fn outer() {
    let key = NEXT.fetch_add(1, Relaxed);
    fn inner(key: &str) {
        let dir = std::env::temp_dir().join(format!("{key}-{}", process::id()));
    }
}"#,
    );
    assert_eq!(c.recall_blind.len(), 1);
}

#[test]
fn visible_counter_bindings_work_in_both_format_styles() {
    for expr in [r#"format!("한글-{key}")"#, r#"format!("한글-{}", key)"#] {
        let c = check(&format!(
            r#"fn f() {{
    let key = format!("{{}}-{{}}", process::id(), NEXT.fetch_add(1, Relaxed));
    {{
        let dir = std::env::temp_dir().join({expr});
    }}
}}"#
        ));
        assert_eq!(c.uniquified.len(), 1);
        assert!(c.recall_blind.is_empty());
        assert!(c.silent.is_empty());
    }
}

#[test]
fn same_named_counter_does_not_hide_a_weak_clock_in_another_function() {
    let c = check(
        r#"fn counter() {
    let key = NEXT.fetch_add(1, Relaxed);
}
fn clock() {
    let key = SystemTime::now();
    let dir = std::env::temp_dir().join(format!("{key}-{}", process::id()));
}"#,
    );
    assert_eq!(c.weak_only.len(), 1);
}

#[test]
fn comments_and_literals_cannot_declare_counter_bindings() {
    let c = check(
        r#"fn f() {
    let key = "fixed";
    // let key = NEXT.fetch_add(1, Relaxed);
    let text = "let key = NEXT.fetch_add(1, Relaxed);";
    let dir = std::env::temp_dir().join(format!("{key}-{}", process::id()));
}"#,
    );
    assert_eq!(c.recall_blind.len(), 1);
}

#[test]
fn a_helper_call_in_another_function_does_not_make_a_clock_per_call() {
    let c = check(
        r#"fn tempdir() {
    let dir = std::env::temp_dir().join(format!("{}-{}", process::id(), SystemTime::now()));
}
fn caller() {
    let dir = tempdir();
}"#,
    );
    assert_eq!(c.weak_only.len(), 1);
}

#[test]
fn a_comment_reference_does_not_supply_the_missing_axis() {
    let c = check(
        r#"fn f() {
    let key = NEXT.fetch_add(1, Relaxed);
    let dir = std::env::temp_dir().join(format!("{}", process::id()));
    // key is not part of this path.
}"#,
    );
    assert_eq!(c.recall_blind.len(), 1);
}
