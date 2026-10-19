use std::path::Path;

use tasty_doc_guards::adr_index::{INDEX, collect};
use tasty_doc_guards::adr_inventory::{EMPTY_MARKER, violations};
use tasty_doc_guards::temp_scratch::Scratch;

fn prepare(root: &Path) {
    std::fs::create_dir_all(root.join("docs/adr")).expect("directory");
}
fn write_adr(root: &Path) {
    std::fs::write(root.join("docs/adr/0001-exception.md"),
        "# ADR-0001: exception\n\n- **Status**: Accepted\n- **Date**: 2026-10-19\n- **Tags**: test\n- **Group**: exceptions\n"
    ).expect("write");
}

#[test]
fn empty_requires_explicit_intent_and_an_existing_directory() {
    let scratch = Scratch::new("adr-empty-inventory");
    let root = scratch.path();
    assert!(violations(root, &[], EMPTY_MARKER).is_err());
    prepare(root);
    assert!(!violations(root, &[], "# ADR").expect("read").is_empty());
    assert!(
        violations(root, &[], EMPTY_MARKER)
            .expect("read")
            .is_empty()
    );
    assert!(
        !violations(root, &[], &format!("{EMPTY_MARKER}\n{EMPTY_MARKER}"))
            .expect("read")
            .is_empty()
    );
}

#[test]
fn an_empty_marker_cannot_hide_a_missing_or_new_decision() {
    let scratch = Scratch::new("adr-inventory-missing");
    let root = scratch.path();
    prepare(root);
    write_adr(root);
    assert!(
        !violations(root, &[], EMPTY_MARKER)
            .expect("read")
            .is_empty()
    );
    let adrs = collect(root).expect("collect");
    assert!(
        !violations(root, &adrs, EMPTY_MARKER)
            .expect("read")
            .is_empty()
    );
    assert!(violations(root, &adrs, "# ADR").expect("read").is_empty());
    std::fs::write(root.join("docs/adr/1-malformed.md"), "# wrong").expect("write");
    assert!(!violations(root, &adrs, "# ADR").expect("read").is_empty());
}

#[test]
fn generator_accepts_declared_empty_and_rejects_undeclared_empty() {
    let scratch = Scratch::new("adr-empty-bin");
    let root = scratch.path();
    prepare(root);
    for (index, expected) in [
        (format!("# ADR\n\n{EMPTY_MARKER}\n"), 0),
        ("# ADR\n".into(), 2),
    ] {
        std::fs::write(root.join(INDEX), index).expect("index");
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_adr-index"))
            .arg("--check")
            .arg(root)
            .output()
            .expect("run");
        assert_eq!(output.status.code(), Some(expected), "{output:?}");
    }
}
