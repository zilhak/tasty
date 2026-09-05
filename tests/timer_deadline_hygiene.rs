//! 파생 데드라인 스핀 재유입 가드 — 소스 수준에서 두 규칙을 강제한다.
//!
//! 배경: 같은 버그 클래스가 이미 **두 번** 터졌다. 외부 상태에서 파생한 절대
//! 데드라인(`마지막으로 읽은 시각 + 주기`, `처음 dirty 가 된 시각 + 디바운스`,
//! `다음 재시도 시각`)은 그 외부 상태가 갱신을 멈추면 **영원히 과거**에 머문다.
//! 그 값을 매 프레임 다시 등록하면 `WaitUntil(과거)` 가 즉시 wake 를 무한 반복해
//! 코어 하나가 100% 로 점유된다. 등록 누락(누수 = 한 번 더 깨어남)과 달리 실패
//! 비용이 크고, 두 사례 모두 단위 테스트로는 잡히지 않는 **호출부 한 줄**이었다.
//!
//! 규칙과 배경 서술은 [`docs/dev-guide/timer-hub.md`] "파생 데드라인은 반드시
//! 바닥친다" 절. 선례(소스 스캔 테스트): `crates/tasty-doc-guards/tests/no_todo_file_citation.rs`,
//! `tests/no_emoji_in_source.rs`.
//!
//! **R1 — 파생 데드라인은 `arm_derived` 를 통해서만 등록한다.**
//! `src/app/timers.rs` 에서 `hub.once_at(` 을 직접 부르면 fail. 새 `Tick` 을
//! 추가하면서 바닥치기를 빠뜨리는 것이 이 클래스의 재발 경로다.
//!
//! **R2 — DAG 폴링 요청 목록이 비어도 `poll` 을 호출한다.**
//! `src/adapters/ui/egui_panels.rs` 에서 `requests` 의 빈 여부로 `poll` 을 건너뛰면
//! fail. 빈 목록은 "이 창에 보이는 DAG 뷰가 없다" 는 정보이고, 그때 `visible` 이
//! 비워져야 호스트가 타이머를 걷는다. 배경 탭 전환은 `drop_view` 를 부르지 않으므로
//! 이 경로가 유일한 정리 지점이다.

use std::path::Path;

/// 주석/문서 줄을 뺀 실제 코드 줄만 본다 — 규칙을 설명하는 주석이 스스로를
/// 위반으로 만들면 안 된다.
fn code_lines(src: &str) -> impl Iterator<Item = (usize, &str)> {
    src.lines().enumerate().filter_map(|(i, l)| {
        let t = l.trim_start();
        if t.starts_with("//") {
            None
        } else {
            Some((i + 1, l))
        }
    })
}

fn read(rel: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn derived_deadlines_go_through_the_floor_helper() {
    const FILE: &str = "src/app/timers.rs";
    let src = read(FILE);

    // 통로 자체가 사라지면(이름 변경 등) 규칙이 조용히 무력화된다.
    assert!(
        src.contains("fn arm_derived("),
        "{FILE}: `arm_derived` 가 없다 — 파생 데드라인 등록 통로가 사라졌다. \
         이름을 바꿨다면 이 테스트와 docs/dev-guide/timer-hub.md 도 함께 고쳐라."
    );
    assert!(
        src.contains("fn not_before_next_period("),
        "{FILE}: 바닥치기 함수 `not_before_next_period` 가 없다."
    );

    let offenders: Vec<usize> = code_lines(&src)
        .filter(|(_, l)| l.contains("hub.once_at(") && !l.contains("fn arm_derived"))
        .map(|(n, _)| n)
        .collect();
    // `arm_derived` 본문의 단 한 줄만 허용된다.
    assert_eq!(
        offenders.len(),
        1,
        "{FILE}:{offenders:?} — 파생 데드라인은 `arm_derived` 로만 등록한다. \
         `hub.once_at` 을 직접 부르면 이미 지난 데드라인이 그대로 등록돼 \
         이벤트 루프가 스핀한다(docs/dev-guide/timer-hub.md)."
    );
    let body_start = src
        .find("fn arm_derived(")
        .expect("checked above: arm_derived exists");
    let line_of_call = src[..src.find("hub.once_at(").expect("checked above")]
        .lines()
        .count();
    let line_of_fn = src[..body_start].lines().count();
    assert!(
        line_of_call > line_of_fn,
        "{FILE}: 유일하게 허용되는 `hub.once_at` 은 `arm_derived` 본문의 것이다 \
         (fn at line {line_of_fn}, call at line {line_of_call})."
    );
}

#[test]
fn dag_view_polling_is_not_skipped_on_an_empty_frame() {
    const FILE: &str = "src/adapters/ui/egui_panels.rs";
    let src = read(FILE);

    assert!(
        src.contains("dag_views.poll(engine, &requests);"),
        "{FILE}: DAG 뷰 폴링 호출이 없다 — 옮겼다면 이 테스트도 함께 옮겨라."
    );
    let offenders: Vec<usize> = code_lines(&src)
        .filter(|(_, l)| l.contains("requests.is_empty()"))
        .map(|(n, _)| n)
        .collect();
    assert!(
        offenders.is_empty(),
        "{FILE}:{offenders:?} — `requests` 가 비어도 `poll` 을 호출해야 한다. \
         빈 목록은 '보이는 DAG 뷰 없음' 이고, 그때 `visible` 이 비워져야 호스트가 \
         폴링 타이머를 걷는다. 배경 탭 전환은 `drop_view` 를 부르지 않으므로 이 \
         경로가 유일한 정리 지점이다(docs/dev-guide/timer-hub.md)."
    );
}
