//! gui 의 `app_methods` step 이 이름을 부르는 메서드마다, **헤드리스가 답하거나
//! 왜 못 답하는지가 적혀 있다.**
//!
//! ## 초록이 무엇을 뜻하는가 (먼저 읽을 것)
//!
//! 이 가드가 초록이라고 해서 **두 조합이 같은 집합에 답하는 것이 아니다.** 그 명제는
//! 참이 아니고, 참으로 만들 수도 없다 — `window.list` 가 읽는 것은 `App.view` 이고
//! 헤드리스에는 창이 없다. 창이 없으면 답이 정의되지 않는 메서드가 실제로 있다.
//!
//! 초록이 뜻하는 것은 이것뿐이다: **차이가 전부 이 파일에 사유와 함께 열거돼 있다.**
//! 새 app 층 메서드를 gui 에 더하면서 헤드리스를 안 보면 이 가드가 그 자리에서 막고,
//! 답할 수 없다면 사유를 적게 한다. 빈칸으로 두면 다음 사람이 "빠뜨린 것" 으로 읽고
//! 처음부터 다시 센다 — [`docs/identity.md`] 원칙 2 가 걸린 자리에서 그 재측정이
//! 반복되는 것이 이 가드가 막으려는 것이다.
//!
//! ## 왜 텍스트로 읽는가
//!
//! 두 라우터의 dispatch 는 `match`/`if` 안의 문자열 리터럴이라 밖으로 꺼낼 상수가
//! 없다. 값으로 읽을 수 있는 것(읽기 전용 `plugin.*` 표)은 값으로 읽는다.
//!
//! 판정은 **각 dispatch 함수의 본문만** 본다. 같은 메서드 이름이 doc 주석·다른 헬퍼에
//! 흔하게 등장해서, 파일 전체를 세면 안 열린 것이 열린 것으로 잡힌다.

use std::collections::BTreeSet;

use super::{fn_body, repo_root};

const GUI_STEP: &str = "src/app/ipc/app_methods.rs";
const GUI_FN: &str = "fn ipc_step_app_methods";
const HEADLESS_PUMP: &str = "src/boot/headless_dispatch.rs";
const HEADLESS_FN: &str = "fn pump_ipc";

/// gui step 이 부르는 메서드 수의 하한 — **연기 검사**다.
/// 값의 근거: 2026-09-05 실측 17 건.
const MIN_GUI_METHODS: usize = 12;

/// 헤드리스 펌프가 이름으로 답하는 메서드 수의 하한.
/// 값의 근거: 2026-09-05 실측 6 건.
const MIN_HEADLESS_METHODS: usize = 4;

/// gui 의 app 층 step 에는 있고 **헤드리스에는 의도적으로 없는** 메서드와 그 사유.
///
/// 사유는 전부 같은 형태다 — *읽는 것이 `App.view` 인데 헤드리스에 그 필드가 없다*
/// (`src/app.rs` 에서 `#[cfg(feature = "gui")]`). 그럼에도 메서드마다 한 줄씩 적는
/// 이유는, "이 스위트는 GUI 가 필요하다" 같은 뭉뚱그림이 **어느 것이 진짜 창을
/// 요구하고 어느 것이 그냥 안 열린 것인지**를 지워 버리기 때문이다.
const NOT_IN_HEADLESS: &[(&str, &str)] = &[
    (
        "remote.attach",
        "mirror workspace 를 띄울 창이 필요하다 — winit proxy 로 창 생성 이벤트를 보낸다",
    ),
    (
        "system.gpu_stats",
        "창마다의 GpuState 와 wgpu 전역 리포트를 센다. 헤드리스엔 GPU 컨텍스트가 없다",
    ),
    (
        "ui.screenshot",
        "창 표면을 읽어 파일로 쓴다. 그릴 창이 없으면 하는 일 자체가 없다",
    ),
    ("view.close", "`window.close` 의 다른 이름 — 같은 사유"),
    ("view.create", "`window.create` 의 다른 이름 — 같은 사유"),
    ("view.focus", "`window.focus` 의 다른 이름 — 같은 사유"),
    ("view.list", "`window.list` 의 다른 이름 — 같은 사유"),
    (
        "window.close",
        "`App.view.views` 에서 창을 닫는다. 헤드리스엔 그 레지스트리가 없다",
    ),
    (
        "window.create",
        "winit 이벤트루프에 창 생성을 맡긴다. 헤드리스엔 이벤트루프가 없다",
    ),
    (
        "window.focus",
        "포커스 전환이라 애초에 debug 격리(ADR-0115)이고, 대상도 창이다",
    ),
    (
        "window.list",
        "`App.view.views` 와 `focused_view_id` 를 읽는다. 창이 없으면 빈 목록이 아니라 \
         **개념이 없다** — 빈 목록을 돌려주면 '창이 0 개인 GUI' 로 읽혀 호출자가 \
         `window.create` 를 시도하게 된다",
    ),
];

fn read(rel: &str) -> String {
    let p = repo_root().join(rel);
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
        .replace("\r\n", "\n")
}

/// 본문에 나타나는 `"<something>.<something>"` 꼴 문자열 리터럴 — 메서드 이름 후보.
fn method_literals(body: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = body;
    while let Some(at) = rest.find('"') {
        let after = &rest[at + 1..];
        let Some(end) = after.find('"') else { break };
        let lit = &after[..end];
        let dotted = lit.split('.').count() >= 2;
        let shaped = !lit.is_empty()
            && lit
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_')
            && !lit.ends_with('.');
        if dotted && shaped {
            out.insert(lit.to_string());
        }
        rest = &after[end + 1..];
    }
    out
}

fn gui_methods() -> BTreeSet<String> {
    let src = read(GUI_STEP);
    let body = fn_body(&src, GUI_FN)
        .unwrap_or_else(|| panic!("{GUI_STEP} 에서 `{GUI_FN}` 본문을 못 잘랐다"));
    method_literals(&body)
}

fn headless_methods() -> BTreeSet<String> {
    let src = read(HEADLESS_PUMP);
    let body = fn_body(&src, HEADLESS_FN)
        .unwrap_or_else(|| panic!("{HEADLESS_PUMP} 에서 `{HEADLESS_FN}` 본문을 못 잘랐다"));
    method_literals(&body)
}

/// gui app 층 step 의 모든 메서드는 **헤드리스가 답하거나, 왜 못 답하는지가 적혀 있다.**
#[test]
fn every_gui_app_layer_method_is_answered_headless_or_carries_a_reason() {
    let gui = gui_methods();
    assert!(
        gui.len() >= MIN_GUI_METHODS,
        "gui app 층 step 에서 메서드를 {} 개밖에 못 뽑았다(하한 {MIN_GUI_METHODS}, \
         2026-09-05 실측 17). 대조군이 죽었다 — 추출기나 함수 이름을 확인해라",
        gui.len()
    );
    let headless = headless_methods();
    assert!(
        headless.len() >= MIN_HEADLESS_METHODS,
        "헤드리스 펌프에서 메서드를 {} 개밖에 못 뽑았다(하한 {MIN_HEADLESS_METHODS}, \
         2026-09-05 실측 6)",
        headless.len()
    );

    let excused: BTreeSet<&str> = NOT_IN_HEADLESS.iter().map(|(m, _)| *m).collect();
    let missing: Vec<&String> = gui
        .iter()
        .filter(|m| !headless.contains(*m) && !excused.contains(m.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "gui 의 app 층 step 이 답하는데 헤드리스는 답하지도, 왜 못 답하는지 적혀 있지도 \
         않다. 헤드리스는 CLI 전용 실행 형태라 이 빈칸은 `docs/identity.md` 원칙 2 의 \
         구멍이다 — `src/boot/headless_dispatch.rs` 에서 답하게 하거나, 답할 수 없으면 \
         `NOT_IN_HEADLESS` 에 사유를 적어라: {missing:?}"
    );

    // 반대 방향. 사유가 적혀 있는데 실제로는 헤드리스가 답하면, 그 사유가 낡은 것이다.
    let stale: Vec<&str> = NOT_IN_HEADLESS
        .iter()
        .map(|(m, _)| *m)
        .filter(|m| headless.contains(*m))
        .collect();
    assert!(
        stale.is_empty(),
        "헤드리스가 실제로 답하는데 `NOT_IN_HEADLESS` 가 아직 못 답한다고 말한다 — \
         사유를 지워라: {stale:?}"
    );
    // 그리고 gui 가 부르지도 않는 이름이 사유 목록에 남아 있으면 그것도 낡은 것이다.
    let orphan: Vec<&str> = NOT_IN_HEADLESS
        .iter()
        .map(|(m, _)| *m)
        .filter(|m| !gui.contains(*m))
        .collect();
    assert!(
        orphan.is_empty(),
        "gui app 층 step 이 더 이상 부르지 않는 이름이 사유 목록에 남아 있다: {orphan:?}"
    );
}

/// 사유가 **비어 있지 않고 서로 다르다.**
///
/// 같은 문장을 복사해 채우면 목록은 통과하는데 정보가 0 이 된다 — 그러면 이 가드는
/// "뭉뚱그림 금지" 라는 자기 목적을 잃는다.
#[test]
fn each_reason_says_something_and_says_it_once() {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for (method, reason) in NOT_IN_HEADLESS {
        assert!(
            reason.len() >= 10,
            "`{method}` 의 사유가 너무 짧아 아무것도 말하지 않는다"
        );
        assert!(
            seen.insert(reason),
            "`{method}` 가 다른 항목과 **글자 그대로 같은** 사유를 쓴다. 같은 이유라면 \
             무엇이 다른지를 적어라 — 뭉뚱그림을 막는 것이 이 목록의 목적이다"
        );
    }
}

/// 자르기가 함수 본문에서 멈춘다 — 파일 전체를 읽으면 안 열린 것이 열린 것으로 잡힌다.
#[test]
fn the_cut_stops_at_the_dispatch_function() {
    let src = "\
fn before() { let m = \"ns.before\"; }
fn pump_ipc(app: &mut App) {
    if m == \"ns.inside\" { go(); }
}
fn after() { let m = \"ns.after\"; }
";
    let body = fn_body(src, "fn pump_ipc").expect("본문을 잘라야 한다");
    let found = method_literals(&body);
    assert!(found.contains("ns.inside"));
    assert!(
        !found.contains("ns.before") && !found.contains("ns.after"),
        "본문 밖 리터럴을 집었다: {found:?}"
    );
}
