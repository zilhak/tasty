//! `tasty-icons`의 Lucide 출처 주석이 기준표·배포물의 고지 세트와 맞는지 확인한다.
//! - 출처 주석은 바로 다음 줄이 아이콘 정의(`stroke_icon!`·`fill_icon!`)다.
//! - 출처 주석을 단 (정의 이름, Lucide 이름, 같은 도형·고쳐 온 도형) 집합이 아래 `CREDITS`와 같다.
//!   `CREDITS`는 디자인 icons.json의 `source` 필드를 옮긴 기준표다. 고지 문서에는 수를 적지 않는다.
//! - 출처 주석마다 라이선스 본문 두 개를 정확히 적고, 그 본문은 `LICENSES/`에 있으며
//!   `THIRD_PARTY_LICENSES.md`가 링크한다.
//!
//! 주석이 가리키는 도형이 실제로 Lucide와 같은지는 검사하지 않는다.

use std::collections::BTreeSet;

const ICONS: &str = "crates/tasty-icons/src/lib.rs";
const INVENTORY: &str = "THIRD_PARTY_LICENSES.md";
const TEXTS_DIR: &str = "LICENSES";
const CREDIT_PREFIX: &str = "// 출처: Lucide \"";
/// 출처 주석마다 적는 라이선스 본문.
const REQUIRED_TEXTS: [&str; 2] = ["Feather-MIT.txt", "Lucide-ISC.txt"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Same,
    Adapted,
}

/// 디자인 icons.json(`source` 필드)의 기준표: (tasty-icons 정의, Lucide 이름, 종류).
/// "Same shape as"는 `Same`, "Adapted from"은 `Adapted`다.
const CREDITS: &[(&str, &str, Kind)] = &[
    ("PLUS", "plus", Kind::Same),                     // design `plus`
    ("CLOSE", "x", Kind::Same),                       // design `close`
    ("REFRESH", "rotate-cw", Kind::Adapted),          // design `refresh`
    ("EDIT", "pen-line", Kind::Adapted),              // design `edit`
    ("TRASH", "trash", Kind::Adapted),                // design `trash`
    ("CHECK", "check", Kind::Same),                   // design `check`
    ("MORE", "ellipsis", Kind::Adapted),              // design `more`
    ("STAR", "star", Kind::Adapted),                  // design `star`
    ("STAR_FILL", "star", Kind::Adapted),             // design `starFill`
    ("CHEVRON_RIGHT", "chevron-right", Kind::Same),   // design `chevronRight`
    ("ARROW_RIGHT", "arrow-right", Kind::Adapted),    // design `arrowRight`
    ("CHEVRON_DOWN", "chevron-down", Kind::Same),     // design `chevronDown`
    ("CHEVRON_UP", "chevron-up", Kind::Same),         // design `chevronUp`
    ("CHEVRON_LEFT", "chevron-left", Kind::Same),     // design `chevronLeft`
    ("CHEVRONS_LEFT", "chevrons-left", Kind::Same),   // design `chevronsLeft`
    ("CHEVRONS_RIGHT", "chevrons-right", Kind::Same), // design `chevronsRight`
    ("MOVE", "move", Kind::Same),                     // design `move`
    ("FOLDER_OPEN", "folder-open", Kind::Adapted),    // design `folderOpen`
    ("IMAGE", "image", Kind::Adapted),                // design `image`
    ("LAYOUT_GRID", "layout-grid", Kind::Same),       // design `layoutGrid`
    ("LIST", "list", Kind::Same),                     // design `listView`
    ("REMOTE", "terminal", Kind::Same),               // design `remote`
    ("GIT_TREE", "git-commit-vertical", Kind::Same),  // design `gitTree`
    ("EYE", "eye", Kind::Adapted),                    // design `eye`
    ("GLOBE", "globe", Kind::Adapted),                // design `globe`
    ("GLOBE_OFF", "globe", Kind::Adapted),            // design `globeOff`
    ("ALERT_CIRCLE", "circle-alert", Kind::Adapted),  // design `alertCircle`
    ("HELP_CIRCLE", "circle-help", Kind::Same),       // design `helpCircle`
    ("BELL", "bell", Kind::Adapted),                  // design `bell`
    ("THEME", "moon", Kind::Adapted),                 // design `theme`
    ("SUN", "sun-medium", Kind::Adapted),             // design `sun`
    ("HASH", "hash", Kind::Same),                     // design `hash`
    ("CMD_KEY", "command", Kind::Same),               // design `cmdKey`
    ("INFO", "info", Kind::Adapted),                  // design `info`
    ("LINK", "link", Kind::Adapted),                  // design `link`
];

#[derive(Debug, PartialEq, Eq)]
struct Credit {
    line: usize,
    definition: String,
    lucide: String,
    kind: Kind,
    texts: BTreeSet<String>,
}

/// `stroke_icon!(NAME, …` 또는 `stroke_icon!(` 다음 줄의 `NAME,`에서 정의 이름을 읽는다.
fn definition_name(lines: &[&str], at: usize) -> Option<String> {
    let first = lines.get(at)?.trim_start();
    let after = first
        .strip_prefix("stroke_icon!(")
        .or_else(|| first.strip_prefix("fill_icon!("))?;
    let head = if after.trim().is_empty() {
        lines.get(at + 1)?.trim_start()
    } else {
        after.trim_start()
    };
    let name: String = head
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
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
        let Some(definition) = definition_name(&lines, i + 1) else {
            let next = lines.get(i + 1).map(|l| l.trim_start()).unwrap_or("");
            problems.push(format!(
                "{n}행: 출처 주석 바로 다음 줄이 아이콘 정의가 아니다: {next}"
            ));
            continue;
        };
        credits.push(Credit {
            line: n,
            definition,
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
    let required: BTreeSet<String> = REQUIRED_TEXTS.iter().map(|t| t.to_string()).collect();
    for c in &credits {
        if c.texts != required {
            problems.push(format!(
                "{}행 {} (Lucide \"{}\"): 라이선스 본문이 {:?} 다. {:?} 여야 한다",
                c.line, c.definition, c.lucide, c.texts, required
            ));
        }
        for text in &c.texts {
            if !root.join(TEXTS_DIR).join(text).is_file() {
                problems.push(format!(
                    "{}행 {}: {TEXTS_DIR}/{text} 가 없다",
                    c.line, c.definition
                ));
            }
            if !inventory.contains(&format!("]({TEXTS_DIR}/{text})")) {
                problems.push(format!(
                    "{}행 {}: {INVENTORY} 가 {TEXTS_DIR}/{text} 를 링크하지 않는다",
                    c.line, c.definition
                ));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "{ICONS} 의 Lucide 출처 주석 문제:\n{}",
        problems.join("\n")
    );
    let found: BTreeSet<(String, String, Kind)> = credits
        .iter()
        .map(|c| (c.definition.clone(), c.lucide.clone(), c.kind))
        .collect();
    assert_eq!(
        found.len(),
        credits.len(),
        "같은 정의에 출처 주석이 두 번 있다"
    );
    let expected: BTreeSet<(String, String, Kind)> = CREDITS
        .iter()
        .map(|(d, l, k)| (d.to_string(), l.to_string(), *k))
        .collect();
    assert_eq!(
        expected.len(),
        CREDITS.len(),
        "기준표에 같은 행이 두 번 있다"
    );
    let missing: Vec<_> = expected.difference(&found).collect();
    let extra: Vec<_> = found.difference(&expected).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{ICONS} 의 출처 주석이 디자인 icons.json 기준표와 다르다.\n\
         기준표에 있는데 주석이 없거나 다른 것: {missing:?}\n기준표에 없는 주석: {extra:?}"
    );
}

#[test]
fn credit_reader_takes_the_definition_kind_and_texts() {
    let src = "// 출처: Lucide \"x\", 같은 도형. v(LICENSES/a.txt, LICENSES/b.txt).\n\
               stroke_icon!(CLOSE, \"close\", r#\"\"#);\n\
               // 출처: Lucide \"star\", 고쳐 온 도형. v(LICENSES/a.txt).\n\
               fill_icon!(\n\
               \x20   STAR_FILL,\n\
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
                definition: "CLOSE".into(),
                lucide: "x".into(),
                kind: Kind::Same,
                texts: BTreeSet::from(["a.txt".to_string(), "b.txt".to_string()]),
            },
            Credit {
                line: 3,
                definition: "STAR_FILL".into(),
                lucide: "star".into(),
                kind: Kind::Adapted,
                texts: BTreeSet::from(["a.txt".to_string()]),
            },
        ]
    );
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(problems[0].starts_with("6행:"), "{problems:?}");
    assert!(problems[1].starts_with("9행:"), "{problems:?}");
    assert!(problems[2].starts_with("10행:"), "{problems:?}");
}
