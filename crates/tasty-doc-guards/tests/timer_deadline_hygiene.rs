//! 과거 데드라인을 반복 등록하면 이벤트 루프가 즉시 깨기를 반복할 수 있다.
//! 파생 데드라인은 arm_derived로 등록해 현재 또는 지난 시각만 다음 주기로 미룬다.
//! 미래 데드라인은 다음 주기보다 가까워도 그대로 유지한다.
//! 또한 DAG 요청이 비어도 poll을 호출해야 보이지 않는 뷰의 타이머를 정리할 수 있다.
//! 배경 탭으로 바뀔 때는 drop_view가 호출되지 않아 빈 요청 처리가 필요하다.
//! 규칙은 docs/dev-guide/timer-hub.md에 있다.
//!
//! 이 검사는 호출 문자열의 개수·순서와 requests.is_empty 사용 여부를 확인한다.
//! 함수 본문이나 제어 흐름을 완전히 해석하지는 않는다.

/// 주석으로 시작하는 줄만 제외한다. 문자열 내부까지 구분하지는 않는다.
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
    let p = tasty_doc_guards::repo_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn derived_deadlines_go_through_the_floor_helper() {
    const FILE: &str = "src/app/timers.rs";
    let src = read(FILE);

    assert!(
        src.contains("fn arm_derived("),
        "{FILE}: `arm_derived` 가 없다 — 파생 데드라인 등록 통로가 사라졌다. \
         이름을 바꿨다면 이 테스트와 docs/dev-guide/timer-hub.md 도 함께 고쳐라."
    );
    assert!(
        src.contains("fn not_before_next_period("),
        "{FILE}: 데드라인을 제한하는 not_before_next_period 함수가 없다."
    );

    let offenders: Vec<usize> = code_lines(&src)
        .filter(|(_, l)| l.contains("hub.once_at(") && !l.contains("fn arm_derived"))
        .map(|(n, _)| n)
        .collect();
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
        "{FILE}: hub.once_at 호출이 arm_derived 정의보다 앞에 있다(fn {line_of_fn}, call {line_of_call}). 등록 함수의 본문에서 호출해야 한다."
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
