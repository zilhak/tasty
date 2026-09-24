//! 최근 파일의 시각 문구가 예약된 열 너비에 들어가는지 모든 언어에서 확인한다.
//! 문자별 고정폭 칸 수만 근사하며 실제 폰트나 폴백의 픽셀 폭은 검사하지 않는다.

use std::collections::BTreeMap;
use std::path::Path;

const LANGS: &[&str] = &tasty_i18n::BUILTIN_CODES;

/// 날짜 YYYY-MM-DD의 10 칸을 기준으로 예약한 열 너비다.
const COLUMN_CELLS: usize = 10;

/// 각 시간 버킷의 최대 값이다. 분 59, 시간 23, 일 6을 넘으면 다음 형식으로 바뀐다.
/// 버킷 경계가 바뀌면 이 입력도 함께 확인한다.
const BUCKETS: &[(&str, Option<i64>)] = &[
    ("file_handler.picker.when_just_now", None),
    ("file_handler.picker.when_minutes", Some(59)),
    ("file_handler.picker.when_hours", Some(23)),
    ("file_handler.picker.when_yesterday", None),
    ("file_handler.picker.when_days", Some(6)),
];

#[test]
fn every_relative_time_word_fits_the_reserved_column_in_every_locale() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("lang");
    let mut problems = Vec::new();
    let mut checked = 0usize;

    for lang in LANGS {
        let catalog = load(&root, lang);
        for (key, max_n) in BUCKETS {
            let Some(pattern) = catalog.get(*key) else {
                problems.push(format!("{lang}: `{key}` 가 없다"));
                continue;
            };
            let rendered = match max_n {
                Some(n) => pattern.replacen("{}", &n.to_string(), 1),
                None => pattern.clone(),
            };
            checked += 1;
            let cells = mono_cells(&rendered);
            if cells > COLUMN_CELLS {
                problems.push(format!(
                    "{lang} `{key}` = {rendered:?} → {cells} 칸 (예약 {COLUMN_CELLS} 칸)"
                ));
            }
        }
    }

    assert!(
        checked >= LANGS.len() * BUCKETS.len(),
        "검사한 문구가 {checked} 개로 부족하다. 언어 카탈로그와 키를 확인한다."
    );
    assert!(
        problems.is_empty(),
        "시각 문구가 예약 열보다 넓다. 번역과 실제 표시 폭을 확인해 component.fh-when-width 또는 문구를 조정한다:\n  {}",
        problems.join("\n  ")
    );
}

/// 절대 날짜는 번역 키가 없어 별도로 확인한다.
#[test]
fn the_absolute_date_is_exactly_the_reserved_width() {
    assert_eq!(mono_cells("2026-09-13"), COLUMN_CELLS);
}

/// 일부 CJK 범위를 두 칸으로 센다. 전체 East Asian Width 표가 아니므로 범위 밖 넓은 문자는 놓칠 수 있다.
fn mono_cells(s: &str) -> usize {
    s.chars()
        .map(|c| {
            let w = c as u32;
            let wide = (0x1100..=0x115F).contains(&w)      // 한글 자모
                || (0x2E80..=0xA4CF).contains(&w)          // CJK 부수 · 한자 · 가나
                || (0xAC00..=0xD7A3).contains(&w)          // 한글 음절
                || (0xF900..=0xFAFF).contains(&w)          // CJK 호환 한자
                || (0xFE30..=0xFE6F).contains(&w)          // CJK 호환 형태
                || (0xFF00..=0xFF60).contains(&w)          // 전각 ASCII
                || (0xFFE0..=0xFFE6).contains(&w);
            usize::from(wide) + 1
        })
        .sum()
}

fn load(root: &Path, lang: &str) -> BTreeMap<String, String> {
    let path = root.join(format!("{lang}.toml"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} 를 읽을 수 없다: {e}", path.display()));
    let value: toml::Value = text
        .parse()
        .unwrap_or_else(|e| panic!("{} 파싱 실패: {e}", path.display()));
    let mut flat = std::collections::HashMap::new();
    tasty_i18n::flatten_catalog_toml("", &value, &mut flat);
    flat.into_iter().collect()
}
