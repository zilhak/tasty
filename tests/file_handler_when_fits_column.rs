//! Recent 행 "언제" 열은 **최대 어휘로 예약**된다 — 그 어휘가 정말 최대인지 강제한다.
//!
//! 배경: 열 폭을 `--tasty-fh-when-width`(56px = D2Coding 11px 열 칸 10 개)로 **고정**한
//! 것은 버킷이 바뀔 때 옆의 handler id 가 reflow 되지 않게 하려는 것이다. 그 예약은
//! "가장 넓은 어휘가 `YYYY-MM-DD` 열 칸" 이라는 전제 위에 서 있다. 전제가 깨지면 증상이
//! 조용하다 — 넘치는 문구는 **말줄임되어** 사라지고, 부분적으로 잘린 시각은 다른 시각으로
//! 읽힌다(그것이 애초에 id 쪽을 양보시킨 이유다).
//!
//! 영어만 보면 안 드러난다. `{}m ago` 는 영어에서 7 칸이지만 ko `{}분 전` 은 CJK 글리프
//! 둘이 각각 두 칸이라 7 칸이고, 다른 번역이 들어오면 그 관계가 바뀐다. 그래서 세 로케일을
//! 모두 돈다 — 선례는 `tests/toast_message_fits_cap.rs` 다(같은 lang 파일 순회 · 평탄화).
//!
//! **이 검사가 재는 것과 안 재는 것.** 재는 것은 mono 열 칸 수다. 실제 픽셀 폭은 폰트가
//! 정하고(프로포셔널 폴백이 끼면 달라진다) 여기서는 안 본다 — 그쪽은 Gate 5 캡처가 본다.

use std::collections::BTreeMap;
use std::path::Path;

/// 지원 언어 — 정본을 가리킨다(`toast_message_fits_cap.rs` 와 같은 이유).
const LANGS: &[&str] = &tasty_i18n::BUILTIN_CODES;

/// 예약된 열이 담는 칸 수. `--tasty-fh-when-width` 56px ÷ D2Coding 11px 한 칸 5.5px.
const COLUMN_CELLS: usize = 10;

/// 각 버킷의 키와 **그 버킷이 만들 수 있는 가장 큰 `n`**.
///
/// 상한은 버킷 경계가 정한다: 분은 59(60 이면 시간), 시간은 23(24 면 yesterday),
/// 일은 6(7 이면 날짜). 어휘가 아니라 **경계**에서 나온 수라 여기 다시 적어도 갈리지
/// 않는다 — 경계를 옮기면 `file_handler_picker.rs` 의 경계 시험이 먼저 빨개진다.
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
        "검사한 문구가 {checked} 개뿐이다 — 카탈로그를 못 읽었다"
    );
    assert!(
        problems.is_empty(),
        "예약된 '언제' 열보다 넓은 문구가 있다. 열 폭(`component.fh-when-width`)을 \
         넓히거나 그 로케일의 문구를 줄여라 — 넘치면 말줄임되어 다른 시각으로 읽힌다:\n  {}",
        problems.join("\n  ")
    );
}

/// 절대 날짜(`≥ 7 d`)가 예약 폭을 정한 어휘라는 것 — 정확히 열 칸이다.
///
/// 이 갈래는 lang 키가 없다(로케일 중립 절대 날짜). 그래서 위 순회가 안 보고, 여기서
/// 따로 못 박는다 — 이것이 최대 어휘라는 전제가 곧 예약 폭의 근거다.
#[test]
fn the_absolute_date_is_exactly_the_reserved_width() {
    assert_eq!(mono_cells("2026-09-13"), COLUMN_CELLS);
}

/// mono 열 칸 수 — CJK(한중일 표의문자 · 한글 음절 · 전각 구두점)는 두 칸.
///
/// 전체 East Asian Width 표가 아니라 이 어휘가 실제로 쓰는 범위만 본다. 모자라면
/// **적게 세는** 쪽으로 틀리므로, 그때는 넘치는 문구를 놓친다 — 범위를 넓히는 것이
/// 처방이지 상한을 올리는 것이 아니다.
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

/// lang 파일 하나를 평탄화한 키→값 맵으로. `toast_message_fits_cap.rs` 와 같은 경로.
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
