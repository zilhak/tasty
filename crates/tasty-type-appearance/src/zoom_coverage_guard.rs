//! 배율 커버리지 가드 — "`Theme` 의 길이 접근자가 담은 숫자는 `ui_zoom` 을 탄다" 를
//! 소스에서 강제한다.
//!
//! # 무엇을 막는가
//! 본체는 egui `zoom_factor` 를 1.0 으로 고정하고(`src/gfx/gpu.rs` `update_scale_factor`)
//! UI 배율을 오직 `Theme::with_colors_and_zoom` 안에서만 적용한다. 그래서 `Theme` 를
//! 거치지 않은 길이는 `ui_scale` 을 못 따라간다. 이 크레이트의 길이 접근자 중 숫자를
//! **직접 담은** 것은 `LogicalPx((N * self.ui_zoom).round())` 형태여야 하는데, 그 곱을
//! 빠뜨려도 zoom 1 에서는 값이 같아 어떤 값 테스트도 울지 않는다 — 배율 0.85 / 1.2 에서만
//! 갈라진다. 그 형태를 잡는 장치가 여기 말고는 없다.
//!
//! 결함의 모양은 "조금 다른 값" 이 아니라 **상자만 고정되고 내용은 커지는 것**이다.
//! 접근자가 담는 값은 26~340 이고 그 안에 들어가는 폰트·간격·글리프는 전부 배율을 타므로,
//! 1.2 에서 내용이 상자를 넘고 0.85 에서 빈 공간이 남는다.
//!
//! # 대상이 아닌 것
//! 숫자를 안 담고 다른 필드/접근자로 위임하는 접근자(`self.item_height_interactive`,
//! `self.autocomplete_max_height()`)는 대상이 아니다 — 위임처가 이미 `zoomed()` 를 지난다.
//! 위임처가 배율을 안 타는 경우는 이 가드가 아니라 그 필드의 문제다.
//!
//! # 왜 lib 유닛 테스트인가 (관례 예외 — `tests/` 로 되돌리지 마라)
//! `shadow_policy_guard` 와 같은 이유다. `tests/*.rs` 는 자동 채널이 **헤드리스 조합 하나**
//! 뿐이라(정본 `docs/dev-guide/ci-gates.md`) 기본 조합에서는 스캔이 자동으로 실행되지
//! 않는다. 크레이트 `src/` 안 `#[cfg(test)]` 로 두면 Windows 잡(`--lib --bins`)과 헤드리스
//! 잡(전체 스위트) 두 잡에서 **실행**된다. 순수 텍스트 스캔이라
//! egui 없이도 선다.
//!
//! # 거짓 초록 방지
//! 스캔 대상은 `CARGO_MANIFEST_DIR/src` 의 두 파일이다 — 레포 루트를 거슬러 올라가지
//! 않으므로 worktree 배치에 영향받지 않는다. 그래도 파일을 못 읽으면 접근자가 0 개가 되어
//! 가드가 초록이 되므로, [`ACCESSOR_FLOOR`] 하한이 그 경우를 잡는다.

use std::fs;
use std::path::PathBuf;

/// 숫자를 직접 담은 길이 접근자의 하한. 실측 80 (2026-09-05). 파일을 못 읽거나 파서가
/// 죽으면 0 이 되는데, 그때 가드는 "위반 0" 이라 **초록**이 된다 — 이 하한만이 그걸 잡는다.
/// 접근자가 줄어서 걸리면 하한을 낮추기 전에 왜 줄었는지 먼저 본다.
const ACCESSOR_FLOOR: usize = 70;

/// 스캔 대상 — 이 크레이트에서 `LogicalPx` 접근자를 정의하는 파일 전부.
const SCAN_FILES: &[&str] = &["theme.rs", "generated_component.rs"];

fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// 접근자 하나의 판정 결과.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// 숫자를 담았고 `ui_zoom` 을 곱한다 — 정상.
    ZoomAware,
    /// 숫자를 담았는데 `ui_zoom` 이 없다 — 위반.
    LiteralWithoutZoom,
    /// 숫자를 안 담는다(위임) — 대상 아님.
    Delegating,
}

/// 본문에 `LogicalPx(` 로 감싼 소수 리터럴이 있는지. `f32` 접미(`340.0f32`)도 같이 본다.
fn holds_numeric_literal(body: &str) -> bool {
    let mut rest = body;
    while let Some(at) = rest.find("LogicalPx(") {
        let after = &rest[at + "LogicalPx(".len()..];
        let after = after.strip_prefix('(').unwrap_or(after);
        let after = after.trim_start();
        if after.starts_with(|c: char| c.is_ascii_digit()) {
            return true;
        }
        rest = &rest[at + "LogicalPx(".len()..];
    }
    false
}

fn verdict_of(body: &str) -> Verdict {
    if !holds_numeric_literal(body) {
        Verdict::Delegating
    } else if body.contains("self.ui_zoom") {
        Verdict::ZoomAware
    } else {
        Verdict::LiteralWithoutZoom
    }
}

/// `pub fn <name>(&self) -> LogicalPx` 본문을 훑어 (이름, 판정) 을 모은다.
/// 본문의 끝은 rustfmt 가 내는 4칸 들여쓰기 `}` 다.
fn scan(text: &str) -> Vec<(String, Verdict)> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end()).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let name = line
            .trim_start()
            .strip_prefix("pub fn ")
            .filter(|_| line.contains("(&self) -> LogicalPx"))
            .and_then(|r| r.split('(').next())
            .map(str::to_string);
        if let Some(name) = name {
            let mut body = String::new();
            let mut j = i + 1;
            while j < lines.len() && lines[j] != "    }" {
                body.push_str(lines[j]);
                body.push('\n');
                j += 1;
            }
            out.push((name, verdict_of(&body)));
            i = j;
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_all() -> Vec<(String, Verdict)> {
        let dir = src_dir();
        let mut all = Vec::new();
        for f in SCAN_FILES {
            let text = fs::read_to_string(dir.join(f)).unwrap_or_default();
            all.extend(scan(&text));
        }
        all
    }

    /// 이 크레이트의 길이 접근자가 담은 숫자는 전부 `ui_zoom` 을 탄다.
    /// 편입 시점 위반 0 · 면제 0 이라 allowlist 가 없다 — 생기면 그때 사유를 적는다.
    #[test]
    fn every_literal_bearing_length_accessor_follows_ui_zoom() {
        let all = scan_all();
        let literal_bearing: Vec<_> = all
            .iter()
            .filter(|(_, v)| *v != Verdict::Delegating)
            .collect();
        assert!(
            literal_bearing.len() >= ACCESSOR_FLOOR,
            "숫자를 담은 접근자 {} < 하한 {} — 파일을 못 읽었거나 파서가 죽었다\
             (그 경우 위반 0 이라 조용히 초록이 된다). 전체 접근자 {}",
            literal_bearing.len(),
            ACCESSOR_FLOOR,
            all.len()
        );
        let violations: Vec<&str> = literal_bearing
            .iter()
            .filter(|(_, v)| *v == Verdict::LiteralWithoutZoom)
            .map(|(n, _)| n.as_str())
            .collect();
        assert!(
            violations.is_empty(),
            "길이 접근자 {} 개가 `self.ui_zoom` 을 안 곱한다 — zoom 1 에서만 맞고 \
             0.85 / 1.2 에서 상자가 내용을 못 따라간다: {:?}\n\
             (숫자를 담은 접근자 {} · 위임 접근자 {})",
            violations.len(),
            violations,
            literal_bearing.len(),
            all.len() - literal_bearing.len()
        );
    }

    /// 판정기 자체 — 세 갈래가 실제로 갈리는지. 위 테스트는 위반 0 이라 초록이므로,
    /// 이것이 없으면 "판정기가 아무것도 못 잡는데 초록" 과 구분되지 않는다.
    #[test]
    fn the_verdict_discriminates_the_three_shapes() {
        assert_eq!(
            verdict_of("        LogicalPx((340.0 * self.ui_zoom).round())"),
            Verdict::ZoomAware
        );
        assert_eq!(
            verdict_of("        LogicalPx(340.0)"),
            Verdict::LiteralWithoutZoom
        );
        assert_eq!(
            verdict_of("        LogicalPx((340.0f32 * self.ui_zoom).round())"),
            Verdict::ZoomAware
        );
        assert_eq!(
            verdict_of("        self.item_height_interactive"),
            Verdict::Delegating
        );
        assert_eq!(
            verdict_of("        self.autocomplete_max_height()"),
            Verdict::Delegating
        );
        // 곱셈이 있어도 `ui_zoom` 이 아니면 위반이다(다른 배율 축을 곱한 경우).
        assert_eq!(
            verdict_of("        LogicalPx((340.0 * self.view_zoom).round())"),
            Verdict::LiteralWithoutZoom
        );
    }

    /// 스캐너가 진짜 소스에서 접근자를 뽑는지 — 이름 하나를 실측으로 확인한다.
    /// 파서가 0 개를 뽑아도 하한이 잡지만, 하한은 "몇 개" 만 보고 "무엇" 은 안 본다.
    #[test]
    fn the_scanner_finds_a_known_accessor_in_the_real_source() {
        let all = scan_all();
        let found = all
            .iter()
            .find(|(n, _)| n == "plugins_side_panel_width")
            .expect("plugins_side_panel_width 를 못 찾았다 — 스캐너가 소스를 못 읽는다");
        assert_eq!(found.1, Verdict::ZoomAware);
        // 위임 접근자도 실제로 잡히는지 (한쪽 갈래만 나오면 파서가 편향된 것이다).
        let delegating = all
            .iter()
            .find(|(n, _)| n == "input_height")
            .expect("input_height 를 못 찾았다");
        assert_eq!(delegating.1, Verdict::Delegating);
    }
}
