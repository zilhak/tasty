//! `tasty-icons`의 Lucide 출처 주석이 배포물의 고지 세트와 맞는지 확인한다.
//! - 출처 주석은 바로 다음 줄이 아이콘 정의(`stroke_icon!`·`fill_icon!`)다.
//! - 출처 주석이 이름 붙인 라이선스 본문은 `LICENSES/`에 있고 `THIRD_PARTY_LICENSES.md`가 링크한다.
//! - 출처 주석을 단 정의 수가 아래 기준 수와 같다. 기준 수는 디자인 icons.json의 `source`
//!   필드(같은 도형·고쳐 온 도형)를 센 값이다. 고지 문서에는 수를 적지 않는다.
//!
//! 주석이 가리키는 도형이 실제로 Lucide와 같은지는 검사하지 않는다.

use std::collections::BTreeSet;

const ICONS: &str = "crates/tasty-icons/src/lib.rs";
const INVENTORY: &str = "THIRD_PARTY_LICENSES.md";
const TEXTS_DIR: &str = "LICENSES";
const CREDIT_PREFIX: &str = "// 출처: Lucide \"";
/// 출처 주석마다 있어야 하는 라이선스 본문.
const REQUIRED_TEXTS: [&str; 2] = ["Lucide-ISC.txt", "Feather-MIT.txt"];
/// 디자인 icons.json `source`의 "Same shape as" 수.
const SAME_SHAPE: usize = 17;
/// 디자인 icons.json `source`의 "Adapted from" 수.
const ADAPTED: usize = 18;

#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Same,
    Adapted,
}

#[derive(Debug, PartialEq, Eq)]
struct Credit {
    line: usize,
    lucide: String,
    kind: Kind,
    texts: BTreeSet<String>,
}

/// 출처 주석을 읽는다. 형식이 틀렸거나 다음 줄이 정의가 아니면 그 줄의 문제를 돌려준다.
fn read_credits(src: &str) -> (Vec<Credit>, Vec<String>) {
    let lines: Vec<&str> = src.lines().collect();
    let mut credits = Vec::new();
    let mut problems = Vec::new();
    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim_start();
        let n = i + 1;
        if !line.starts_with("//") || !line.contains("Lucide") {
            continue;
        }
        let Some(rest) = line.strip_prefix(CREDIT_PREFIX) else {
            problems.push(format!(
                "{n}행: Lucide를 말하는 주석이 출처 형식이 아니다: {line}"
            ));
            continue;
        };
        let Some((lucide, rest)) = rest.split_once('"') else {
            problems.push(format!(
                "{n}행: Lucide 아이콘 이름의 따옴표가 닫히지 않았다"
            ));
            continue;
        };
        let kind = if rest.starts_with(", 같은 도형.") {
            Kind::Same
        } else if rest.starts_with(", 고쳐 온 도형.") {
            Kind::Adapted
        } else {
            problems.push(format!(
                "{n}행: '같은 도형'·'고쳐 온 도형' 중 하나가 아니다: {line}"
            ));
            continue;
        };
        let needle = format!("{TEXTS_DIR}/");
        let texts: BTreeSet<String> = rest
            .match_indices(&needle)
            .map(|(pos, _)| {
                rest[pos + needle.len()..]
                    .split([',', ')', ' '])
                    .next()
                    .unwrap_or("")
                    .to_string()
            })
            .collect();
        let next = lines.get(i + 1).map(|l| l.trim_start()).unwrap_or("");
        if !(next.starts_with("stroke_icon!(") || next.starts_with("fill_icon!(")) {
            problems.push(format!(
                "{n}행: 출처 주석 바로 다음 줄이 아이콘 정의가 아니다: {next}"
            ));
            continue;
        }
        credits.push(Credit {
            line: n,
            lucide: lucide.to_string(),
            kind,
            texts,
        });
    }
    (credits, problems)
}

#[test]
fn every_lucide_credit_names_shipped_license_texts() {
    let root = tasty_doc_guards::repo_root();
    let src = std::fs::read_to_string(root.join(ICONS))
        .unwrap_or_else(|e| panic!("{ICONS} 을 읽지 못했다: {e}"));
    let inventory = std::fs::read_to_string(root.join(INVENTORY))
        .unwrap_or_else(|e| panic!("{INVENTORY} 을 읽지 못했다: {e}"));
    let (credits, mut problems) = read_credits(&src);
    for c in &credits {
        for required in REQUIRED_TEXTS {
            if !c.texts.contains(required) {
                problems.push(format!(
                    "{}행 Lucide \"{}\": {TEXTS_DIR}/{required} 를 적지 않았다",
                    c.line, c.lucide
                ));
            }
        }
        for text in &c.texts {
            let path = root.join(TEXTS_DIR).join(text);
            if !path.is_file() {
                problems.push(format!(
                    "{}행 Lucide \"{}\": {TEXTS_DIR}/{text} 가 없다",
                    c.line, c.lucide
                ));
            }
            if !inventory.contains(&format!("]({TEXTS_DIR}/{text})")) {
                problems.push(format!(
                    "{}행 Lucide \"{}\": {INVENTORY} 가 {TEXTS_DIR}/{text} 를 링크하지 않는다",
                    c.line, c.lucide
                ));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "{ICONS} 의 Lucide 출처 주석 문제:\n{}",
        problems.join("\n")
    );
    let same = credits.iter().filter(|c| c.kind == Kind::Same).count();
    let adapted = credits.iter().filter(|c| c.kind == Kind::Adapted).count();
    assert_eq!(
        (same, adapted),
        (SAME_SHAPE, ADAPTED),
        "{ICONS} 의 출처 주석 수(같은 도형, 고쳐 온 도형)가 디자인 icons.json 기준 수와 다르다"
    );
}

#[test]
fn credit_reader_takes_the_kind_texts_and_definition() {
    let src = "// 출처: Lucide \"x\", 같은 도형. v(LICENSES/a.txt, LICENSES/b.txt).\n\
               stroke_icon!(CLOSE, \"close\", r#\"\"#);\n\
               // 출처: Lucide \"star\", 고쳐 온 도형. v(LICENSES/a.txt).\n\
               fill_icon!(\n\
               // 출처: Lucide \"y\", 같은 도형. v(LICENSES/a.txt).\n\
               \n\
               stroke_icon!(Y, \"y\", r#\"\"#);\n\
               // Lucide 원본을 본뜸\n\
               // 출처: Lucide \"z\", 비슷한 도형.\n";
    let (credits, problems) = read_credits(src);
    assert_eq!(
        credits,
        vec![
            Credit {
                line: 1,
                lucide: "x".into(),
                kind: Kind::Same,
                texts: BTreeSet::from(["a.txt".to_string(), "b.txt".to_string()]),
            },
            Credit {
                line: 3,
                lucide: "star".into(),
                kind: Kind::Adapted,
                texts: BTreeSet::from(["a.txt".to_string()]),
            },
        ]
    );
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(problems[0].starts_with("5행:"), "{problems:?}");
    assert!(problems[1].starts_with("8행:"), "{problems:?}");
    assert!(problems[2].starts_with("9행:"), "{problems:?}");
}
