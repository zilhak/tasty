//! Persisted content/OS/headless owners must not regain user viewport fields.
//! This lexical guard checks named struct bodies and types, not aliases or call graphs.
use tasty_doc_guards::{repo_root, source_text::mask_non_code};

fn body(source: &str, name: &str) -> String {
    let code = mask_non_code(source);
    let start = code
        .find(&format!("struct {name} {{"))
        .expect("owner struct exists");
    let open = start + code[start..].find('{').unwrap();
    let mut depth = 0;
    for (i, c) in code[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return code[open + 1..open + i].to_owned();
                }
            }
            _ => {}
        }
    }
    panic!("owner struct body is complete");
}

fn contains_viewport(fields: &str) -> bool {
    fields
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|word| {
            matches!(
                word,
                "scroll_offset" | "TerminalViewport" | "TerminalViewports" | "terminal_views"
            )
        })
}

#[test]
fn content_pty_and_headless_owners_do_not_store_user_viewports() {
    let root = repo_root();
    let owners = [
        ("crates/tasty-terminal/src/lib.rs", "TerminalState"),
        ("crates/tasty-terminal/src/lib.rs", "Terminal"),
        ("crates/tasty-terminal/src/scrollback.rs", "Scrollback"),
        ("crates/tasty-terminal/src/pty.rs", "Pty"),
        ("crates/tasty-terminal/src/pty.rs", "PtyState"),
        ("src/state/command.rs", "CommandContext"),
    ];
    for (path, name) in owners {
        let source = std::fs::read_to_string(root.join(path)).unwrap();
        assert!(
            !contains_viewport(&body(&source, name)),
            "{name} regained display state in {path}"
        );
    }
    let view = std::fs::read_to_string(root.join("src/state/main.rs")).unwrap();
    assert!(
        contains_viewport(&body(&view, "MainViewState")),
        "GUI owner is missing its viewport"
    );
}

#[test]
fn guard_detects_reintroduced_offset_and_nested_viewport_owners() {
    for fields in [
        "scroll_offset: usize",
        "reading: Option<TerminalViewport>",
        "terminal_views: TerminalViewports",
    ] {
        assert!(contains_viewport(&body(
            &format!("struct Terminal {{ {fields} }}"),
            "Terminal"
        )));
    }
    assert!(!contains_viewport(&body(
        "struct Terminal { rows: usize, /* scroll_offset */ note: &'static str }",
        "Terminal"
    )));
}
