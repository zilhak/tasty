use super::*;
use sha2::{Digest, Sha256};

fn detect(src: &str) -> ScriptDetection {
    detect_scripts(src.as_bytes())
}

#[test]
fn inline_and_local_scripts_are_detected() {
    assert_eq!(
        detect("<p>x</p><script>a()</script>"),
        ScriptDetection::Scripts
    );
    assert_eq!(
        detect(r#"<script src="app.js"></script>"#),
        ScriptDetection::Scripts
    );
    assert_eq!(
        detect(r#"<script type="module">a()</script>"#),
        ScriptDetection::Scripts
    );
    assert_eq!(
        detect(r#"<script type="">a()</script>"#),
        ScriptDetection::Scripts
    );
    assert_eq!(
        detect(r#"<script type="text/javascript; charset=utf-8">a()</script>"#),
        ScriptDetection::Scripts,
        "매개변수가 붙어도 보수적으로 스크립트로 본다"
    );
}

#[test]
fn data_blocks_are_not_scripts() {
    assert_eq!(
        detect(
            r#"<script type="application/json">{"onclick": "<a href='javascript:x'>"}</script>"#
        ),
        ScriptDetection::None
    );
    assert_eq!(
        detect(r#"<script type="text/template"><div onclick="x()"></div></script>"#),
        ScriptDetection::None
    );
}

#[test]
fn tag_and_attribute_names_are_case_insensitive() {
    assert_eq!(detect("<SCRIPT>a()</SCRIPT>"), ScriptDetection::Scripts);
    assert_eq!(
        detect(r#"<DIV ONCLICK="a()">x</DIV>"#),
        ScriptDetection::Scripts
    );
    assert_eq!(
        detect(r#"<script TYPE="Application/JSON">{}</script>"#),
        ScriptDetection::None
    );
}

#[test]
fn unquoted_attributes_are_read() {
    assert_eq!(detect("<body onload=init()>"), ScriptDetection::Scripts);
    assert_eq!(
        detect("<a href=javascript:go()>x</a>"),
        ScriptDetection::Scripts
    );
    assert_eq!(
        detect("<script type=application/json>{}</script>"),
        ScriptDetection::None
    );
}

#[test]
fn tags_inside_comments_are_ignored() {
    assert_eq!(
        detect("<!-- <script>a()</script> <div onclick=x> -->"),
        ScriptDetection::None
    );
    assert_eq!(
        detect("<!--[if IE]><script>a()</script><![endif]--><p>ok</p>"),
        ScriptDetection::None
    );
    assert_eq!(
        detect("<!-- note --><script>a()</script>"),
        ScriptDetection::Scripts
    );
}

#[test]
fn javascript_urls_ignore_case_and_whitespace() {
    assert_eq!(
        detect(r#"<a href="  JaVaScRiPt:alert(1)">x</a>"#),
        ScriptDetection::Scripts
    );
    assert_eq!(
        detect("<a href=\"java\tscr\nipt:alert(1)\">x</a>"),
        ScriptDetection::Scripts
    );
    assert_eq!(
        detect(r#"<form action="javascript:x()"></form>"#),
        ScriptDetection::Scripts
    );
    assert_eq!(
        detect(r#"<button formaction="javascript:x()">b</button>"#),
        ScriptDetection::Scripts
    );
    assert_eq!(
        detect(r#"<iframe src="javascript:x()"></iframe>"#),
        ScriptDetection::Scripts
    );
    assert_eq!(
        detect(r#"<a href="javascript-guide.html">x</a>"#),
        ScriptDetection::None
    );
    assert_eq!(
        detect(r#"<a title="javascript:x()">x</a>"#),
        ScriptDetection::None,
        "URL 속성만 본다"
    );
}

#[test]
fn remote_only_scripts_are_reported_separately() {
    assert_eq!(
        detect(r#"<script src="https://cdn.example/app.js"></script>"#),
        ScriptDetection::ScriptsRemoteOnly
    );
    assert_eq!(
        detect(
            r#"<script src="HTTP://cdn.example/a.js"></script><script src="http://x/b.js"></script>"#
        ),
        ScriptDetection::ScriptsRemoteOnly
    );
    assert_eq!(
        detect(r#"<script src="https://cdn.example/a.js"></script><script>b()</script>"#),
        ScriptDetection::Scripts
    );
    assert_eq!(
        detect(r#"<script src="//cdn.example/a.js"></script>"#),
        ScriptDetection::Scripts,
        "file:// 문서에서 //host 는 원격이 아니다"
    );
}

#[test]
fn inline_svg_scripts_are_detected() {
    assert_eq!(
        detect(r#"<svg><script>a()</script></svg>"#),
        ScriptDetection::Scripts
    );
    assert_eq!(
        detect(r#"<svg><script xlink:href="https://x/a.js"/></svg>"#),
        ScriptDetection::ScriptsRemoteOnly
    );
    assert_eq!(
        detect(r#"<svg><set attributeName="x" onbegin="a()"/></svg>"#),
        ScriptDetection::Scripts
    );
}

#[test]
fn text_that_only_looks_like_a_tag_is_not_a_script() {
    assert_eq!(detect("<p>a < script b</p>"), ScriptDetection::None);
    assert_eq!(detect("<p>&lt;script&gt;</p>"), ScriptDetection::None);
    assert_eq!(
        detect(r#"<div title="<script>a()</script>">x</div>"#),
        ScriptDetection::None
    );
    assert_eq!(
        detect("<textarea><script>a()</script></textarea>"),
        ScriptDetection::None
    );
    assert_eq!(
        detect("<title><script>a()</script></title>"),
        ScriptDetection::None
    );
    assert_eq!(
        detect("<style>a::after{content:'<div onclick=x>'}</style>"),
        ScriptDetection::None
    );
    assert_eq!(detect("<p>plain</p>"), ScriptDetection::None);
}

#[test]
fn attributes_starting_with_on_are_treated_as_handlers() {
    assert_eq!(
        detect("<p one>x</p>"),
        ScriptDetection::Scripts,
        "on 으로 시작하는 속성은 보수적으로 핸들러로 본다"
    );
    assert_eq!(detect("<p on>x</p>"), ScriptDetection::None);
}

#[test]
fn a_raw_text_element_ends_at_its_own_closing_tag() {
    assert_eq!(
        detect("<textarea>x</textarea><script>a()</script>"),
        ScriptDetection::Scripts
    );
    assert_eq!(
        detect(r#"<script type="text/template"></scripts><div onclick=x></script>"#),
        ScriptDetection::None,
        "</scripts 는 닫는 태그가 아니다"
    );
}

#[test]
fn a_script_after_the_limit_is_not_detected_but_changes_the_fingerprint() {
    let head = "<p>".to_string() + &"x".repeat(100) + "</p>";
    let tail = "<script>document.title='TAIL'</script>";
    let short = scan_reader(head.as_bytes(), head.len()).expect("scan");
    let appended = format!("{head}{tail}");
    let long = scan_reader(appended.as_bytes(), head.len()).expect("scan");
    assert_eq!(short.detection, ScriptDetection::None);
    assert_eq!(
        long.detection,
        ScriptDetection::None,
        "상한 뒤는 감지하지 않는다"
    );
    assert_ne!(
        short.fingerprint, long.fingerprint,
        "지문은 파일 전체를 덮는다"
    );
    let expected: [u8; 32] = Sha256::digest(appended.as_bytes()).into();
    assert_eq!(long.fingerprint, Fingerprint(expected));
}

#[test]
fn a_script_inside_the_limit_is_detected_across_read_chunks() {
    let mut doc = "a".repeat(READ_CHUNK - 3);
    doc.push_str("<script>a()</script>");
    let s = scan_reader(doc.as_bytes(), SCAN_LIMIT_BYTES).expect("scan");
    assert_eq!(s.detection, ScriptDetection::Scripts);
}
