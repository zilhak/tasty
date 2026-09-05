//! 휠 1노치 거리의 **런타임 출처가 하나로 남아 있는가** — ADR-0130 의 집행.
//!
//! 그 ADR 의 결정 ② 는 "휠을 포인트로 환산하는 모든 지점이 egui
//! `Options::line_scroll_speed` 를 읽는다" 다. 값을 한 번 맞추는 것과 맞은 채로 있는
//! 것은 다르다 — 표면이 하나 더 생기면서 노치를 다시 상수로 박아도, 기존 테스트들은
//! 초록이다. 그것들은 환산 **함수**에 노치를 인자로 넘겨 산술만 보기 때문이다.
//!
//! 그래서 여기서는 함수가 아니라 **자리**를 센다. 값이 같은지가 아니라 출처가 하나인지를
//! 묻는다 — 같은 문자가 아니라 같은 출처다.
//!
//! 동작 축의 짝은 `src/plugin_bridge/wire_scroll.rs` 의 `one_notch_per_context` 다.
//! 그쪽은 컨텍스트에 기본값 아닌 값을 심고 세 경로가 그것을 집어 오는지 실행으로 잰다.
//! 이쪽은 그 셋 말고 **네 번째가 생기는 것**을 본다.

use tasty_doc_guards::cfg_predicate as cfg_span;

use std::path::{Path, PathBuf};

/// 휠 Line 델타를 다루는 자리임을 알리는 표지.
///
/// 세 번째가 필요하다 — popup·banner 는 단위를 `*unit` 으로 그대로 넘기므로 살아 있는
/// 코드에 `MouseWheelUnit::Line` 이 안 나온다(그 이름은 자기 테스트 안에만 있다).
/// 앞의 둘만 보면 환산 자리 다섯 중 셋만 세고, 빠진 둘은 조용하다.
const LINE_UNIT_MARKS: [&str; 3] = [
    "MouseWheelUnit::Line",
    "MouseScrollDelta::LineDelta",
    "wheel_delta_to_points(",
];

/// 런타임 단일 출처를 읽는 형태.
const RUNTIME_SOURCE_MARKS: [&str; 2] = ["line_scroll(", "line_scroll_speed"];

/// 런타임 값이 아니라 **기본값 상수**를 쓰는 형태 — 설정을 바꿔도 안 따라온다.
const FROZEN_DEFAULT: &str = "DEFAULT_WHEEL_LINE_SCROLL";

/// Line 을 다루지만 환산하지는 않는 자리의 면제. (경로, 사유) 로 적는다.
/// 사유는 이름이 아니라 성질이어야 한다 — "여기는 원래 괜찮다" 는 사유가 아니다.
const ALLOWLIST: &[(&str, &str)] = &[(
    "src/view/main/debug_input.rs",
    "debug 주입기 — 요청받은 단위를 환산하지 않고 그대로 넘긴다(`to_egui` · \
     `to_winit_delta`). 여기서 접으면 그 단위를 재현하려던 검증이 다른 환산 경로를 \
     재고도 통과한다. 그 성질을 같은 파일의 `every_unit_reaches_egui_as_itself` 와 \
     `winit_level_maps_line_and_point_to_the_two_winit_deltas` 가 지킨다.",
)];

/// 자리 수 하한. 0 이면 아래 술어들은 위반을 못 찾는 것이 아니라 **볼 것이 없어서**
/// 초록이다. 실측 다섯(`wire_scroll` · `mouse` · `modifier_hint_overlay` ·
/// `popup_render` · `banner_render`)보다 하나 낮게 잡는다 — 자리가 정상적으로 하나
/// 줄 수는 있어도, 표지가 낡아 절반을 놓치는 것은 여기서 먼저 말해야 한다.
const MIN_CONVERSION_SITES: usize = 4;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn gather_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            gather_rs(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

struct Site {
    rel: String,
    reads_runtime_source: bool,
    frozen_default_lines: Vec<usize>,
}

/// `src/` 안에서 휠 Line 을 다루는 자리를 모은다. `#[cfg(test)]` 아래는 제외한다 —
/// 테스트는 노치를 스스로 정해 넣는 것이 정상이고, 그것이 곧 위 술어들의 대조군이다.
fn conversion_sites() -> Vec<Site> {
    let root = repo_root();
    let mut files = Vec::new();
    gather_rs(&root.join("src"), &mut files);
    files.sort();

    let mut out = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let rel = f
            .strip_prefix(&root)
            .unwrap_or(&f)
            .to_string_lossy()
            .replace('\\', "/");
        if ALLOWLIST.iter().any(|(p, _)| *p == rel) {
            continue;
        }
        let lines: Vec<&str> = text.lines().collect();
        let gated = cfg_span::cfg_gated_lines(&lines, "test");
        // 주석은 코드가 아니다. 양방향으로 중요하다 — 산문에 이름이 나왔다고 위반으로
        // 세지 않고, 산문에 `line_scroll_speed` 를 적었다고 읽은 것으로도 안 센다.
        let is_code = |i: usize| !gated[i] && !lines[i].trim_start().starts_with("//");
        let live = |needle: &str| {
            lines
                .iter()
                .enumerate()
                .any(|(i, l)| is_code(i) && l.contains(needle))
        };
        if !LINE_UNIT_MARKS.iter().any(|m| live(m)) {
            continue;
        }
        out.push(Site {
            reads_runtime_source: RUNTIME_SOURCE_MARKS.iter().any(|m| live(m)),
            frozen_default_lines: lines
                .iter()
                .enumerate()
                .filter(|(i, l)| is_code(*i) && l.contains(FROZEN_DEFAULT))
                .map(|(i, _)| i + 1)
                .collect(),
            rel,
        });
    }
    out
}

#[test]
fn the_population_of_conversion_sites_is_not_empty() {
    let n = conversion_sites().len();
    assert!(
        n >= MIN_CONVERSION_SITES,
        "휠 Line 을 다루는 자리가 {n} 개다(하한 {MIN_CONVERSION_SITES}). \
         순회가 깨졌거나 표지가 낡았다 — 아래 술어들은 볼 것이 없으면 공짜로 초록이다.\n\
           ★ 판별 — 이 모수는 다섯이고 **전부 이름이 있다**(`wire_scroll` · `mouse` · \
           `modifier_hint_overlay` · `popup_render` · `banner_render`). 그러니 수를 보지 말고 \
           **어느 이름이 빠졌는지**를 봐라. 빠진 자리의 파일이 아직 있으면 표지가 낡은 것이고(그 자리는 \
           여전히 휠을 다루는데 스캔이 못 알아본다), 파일 자체가 없으면 자리가 정말 사라진 것이다. \
           열거로 셀 수 있는 모수라 이 판별은 언제나 결정적이다 — 이름을 우리가 소유하고 있다.\n\
           ★ 이 하한을 내려서 통과시키지 마라 — 표지가 낡아 절반을 놓치는 것과 자리가 하나 준 것이 \
           같은 수로 나타나는데, 하한을 내리면 그 둘을 영영 안 가르게 된다.\n\
           자리가 정말 없어졌으면 값과 함께 위 doc 의 이름 목록에서도 그것을 지워라."
    );
}

#[test]
fn every_conversion_site_reads_the_runtime_option() {
    let bad: Vec<String> = conversion_sites()
        .into_iter()
        .filter(|s| !s.reads_runtime_source)
        .map(|s| format!("  {}", s.rel))
        .collect();
    assert!(
        bad.is_empty(),
        "휠 Line 을 다루면서 런타임 노치(egui `Options::line_scroll_speed`)를 안 읽는 \
         자리가 있다. 노치를 자기 값으로 정하면 그 표면만 설정을 안 따라오고, 같은 창에서 \
         휠 한 칸이 표면마다 다른 거리를 움직인다(ADR-0130). Line 을 환산하지 않고 \
         넘기기만 하는 자리라면 사유와 함께 ALLOWLIST 에 넣어라:\n{}",
        bad.join("\n")
    );
}

#[test]
fn no_conversion_site_freezes_the_notch_at_its_default() {
    let bad: Vec<String> = conversion_sites()
        .into_iter()
        .filter(|s| !s.frozen_default_lines.is_empty())
        .map(|s| format!("  {}:{:?}", s.rel, s.frozen_default_lines))
        .collect();
    assert!(
        bad.is_empty(),
        "환산 자리가 `{FROZEN_DEFAULT}` 를 직접 읽는다. 그것은 설정의 **기본값**이지 \
         지금 값이 아니다 — 사용자가 슬라이더를 옮겨도 이 자리만 옛 거리로 스크롤한다. \
         런타임 값은 egui 컨텍스트에서 읽어라(ADR-0130):\n{}",
        bad.join("\n")
    );
}
