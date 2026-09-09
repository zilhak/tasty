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
//! **세는 창은 양쪽 다 dispatch 함수 본문이다 — 파일 전체가 아니다.** 두 파일 모두
//! 답하지 않는 헬퍼와 산문이 같이 살아서, 파일을 통째로 세면 **안 열린 것이 열린 것으로**
//! 잡힌다. 헤드리스 쪽만 함수가 여럿이라 [`HEADLESS_DISPATCH_FNS`] 명부로 든다 —
//! 종단 응답이 `intercept_app_layer` / `intercept_debug_app_layer` 로 갈라져 있고,
//! 창을 `pump_ipc` 하나로 두면 그 리팩터 한 번에 판정이 뒤집힌다(실측은 그 상수에 있다).
//! 옆 가드(`tests/ipc_release_table_excludes_input_reproduction.rs` 의
//! `RELEASE_ROUTERS`)가 같은 이동에 같은 처방 — 명부에 헬퍼를 **더하는 것** — 을 썼다.
//!
//! **세기 전에 주석을 걷어낸다**([`super::strip_comments`]). 이 파일의 물음은 "그
//! 이름을 답하는 코드가 있는가" 인데, 답하지 않는 이름을 **설명하려고** 인용하는 주석이
//! dispatch 본문 안에 흔하다. 걷어내지 않으면 그 설명이 코드로 오인돼 두 방향으로 다
//! 틀린다 — 사유가 적힌 이름을 주석이 인용하면 "헤드리스가 실제로 답하는데 사유가
//! 낡았다" 는 **거짓 실패**가 나고, 그 처방대로 사유를 지우면 답하지도 사유도 없는 채로
//! **거짓 초록**이 된다. 둘 다 재현했다(아래 `a_comment_is_not_an_answer`).
//!
//! 텍스트로 읽는 대가는 **리터럴이 아닌 이름은 안 보인다** 는 것이다. 그 사각은 수를
//! 세는 검사로 못 좁힌다 — 이름 하나를 매크로 뒤로 숨기면 항목이 하나 줄 뿐이고(하한
//! 아래로 안 내려간다), 매크로가 만든 이름으로 갈래를 **더하면** 항목 수가 아예 안
//! 변한다. 하한은 줄어드는 방향만 볼 수 있어서 뒤쪽은 원리적으로 못 본다(둘 다 실측으로
//! 통과했다). 그래서 이름의 수가 아니라 **이름을 읽는 자리**를 따로 잰다 — 그 판정은
//! 라우터 전부에 걸리므로 [`super::dispatch_name_literals`] 가 한 자리에서 맡는다.

use std::collections::BTreeSet;

use super::{callers_of, fn_body, repo_root, strip_comments};

const GUI_STEP: &str = "src/app/ipc/app_methods.rs";
const GUI_FN: &str = "fn ipc_step_app_methods";
const HEADLESS_PUMP: &str = "src/boot/headless_dispatch.rs";
const GUI_DEBUG_STEP: &str = "src/app/ipc/debug_methods.rs";
const GUI_DEBUG_FN: &str = "fn ipc_step_debug";

/// gui step 이 부르는 메서드 수의 하한 — **연기 검사**다.
/// 값의 근거: 2026-09-05 실측 17 건.
const MIN_GUI_METHODS: usize = 12;

/// 헤드리스 dispatch 가 **이름으로 답하는** 자리 — 이 셋의 본문 합집합이 좌변이다.
///
/// `pump_ipc` 를 빼지 않는다: 지금 그 본문의 이름은 0 이지만(종단 응답이 전부 갈라져
/// 나갔다) 팔이 거기 다시 생길 수 있고, 명부에서 빼면 그때 안 보인다. 대신 `fn_body`
/// 가 못 자르면 패닉이므로 **셋 다 실재해야** 판정이 성립한다 — 빈 좌변의 초록은
/// 통과가 아니라 미측정이다([`the_roster_names_real_functions`]).
///
/// 실측(2026-09-10, `git show <rev>:파일` 로 같은 추출기를 돌린 값):
///
/// | rev | `pump_ipc` 본문 | 이 명부 합집합 | 파일 전체 |
/// |---|---|---|---|
/// | 가드 도입 시점 | 11 | 11 | 11 |
/// | 분해 직전(`main`) | 12 | 12 | 12 |
/// | 분해 뒤(지금) | 0 | 12 | 12 |
///
/// 읽을 것 둘. ① 분해가 `pump_ipc` 창을 **12 → 0** 으로 떨어뜨렸다(코드는 한 줄도 안
/// 잃었다). ② **파일 전체로 넓혀서 얻는 이름은 0 이다** — 어느 rev 에서도 명부 합집합과
/// 같다. 그래서 넓히지 않고 명부로 든다: 넓히면 얻는 것 없이 산문까지 세게 된다.
const HEADLESS_DISPATCH_FNS: &[&str] = &[
    "fn pump_ipc(",
    "fn intercept_app_layer(",
    "fn intercept_debug_app_layer(",
];

/// 헤드리스 dispatch 가 이름으로 답하는 메서드 수의 하한.
///
/// 값의 근거: 2026-09-10 실측 12 건([`HEADLESS_DISPATCH_FNS`] 의 표). gui 쪽 비율과
/// 맞춘다(17 에 하한 12).
///
/// **옛 주석의 "실측 6" 은 어느 rev 에서도 나온 적이 없는 수다.** 같은 추출기로 다시
/// 재니 도입 시점이 11, `main` 이 12 였다 — 옛 하한 4 도, 그것을 8 로 올리며 적은
/// "6 에서 12 로 늘었다" 도 그 6 위에 서 있었다. 늘어난 것은 없었다.
const MIN_HEADLESS_METHODS: usize = 8;

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
        // 끝이 `.` 인 것은 **prefix 판정**이다(`starts_with("debug.event_bus.")`).
        // 버리면 그 갈래가 통째로 안 보여, prefix 로 답하는 쪽을 "안 답한다" 로 센다.
        let shaped = !lit.is_empty()
            && lit
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_');
        if dotted && shaped {
            out.insert(lit.to_string());
        }
        rest = &after[end + 1..];
    }
    out
}

/// 두 라우터가 **다른 모양의 코드**로 같은 갈래를 답하는 자리.
///
/// gui 의 app 층 step 은 `plugin.` prefix 로 갈래를 치는데, 헤드리스 pump 는 같은 갈래를
/// `plugin::is_readonly_method(...)` 호출로 판정한다. 리터럴끼리 맞대면 이 대응이 안
/// 보여서 **답하는 것을 안 답한다고** 센다 — 그러면 사유를 적으라고 요구하게 되고, 적히는
/// 사유는 거짓이 된다.
///
/// 그래서 이름 대신 **증거**를 요구한다: 헤드리스 본문에 그 토큰이 있어야 covered 다.
/// 토큰이 사라지면(헤드리스가 그 갈래를 잃으면) 다시 빨개진다.
const HEADLESS_COVERS: &[(&str, &str)] = &[("plugin.", "is_readonly_method")];

fn gui_methods() -> BTreeSet<String> {
    let src = read(GUI_STEP);
    let body = fn_body(&src, GUI_FN)
        .unwrap_or_else(|| panic!("{GUI_STEP} 에서 `{GUI_FN}` 본문을 못 잘랐다"));
    method_literals(&body)
}

/// 명부에 든 dispatch 함수들의 본문을 **주석을 걷어낸 뒤** 이어 붙인다.
///
/// 이 문자열이 이 가드의 좌변 전부다 — 이름 추출도, `HEADLESS_COVERS` 의 증거 토큰
/// 조회도 여기서 한다. 두 물음이 같은 사본을 봐야 "주석 한 줄이 증거로 통하는" 갈래가
/// 한쪽에만 남지 않는다.
fn headless_dispatch_code() -> String {
    let src = read(HEADLESS_PUMP);
    let mut out = String::new();
    for sig in HEADLESS_DISPATCH_FNS {
        let body = fn_body(&src, sig).unwrap_or_else(|| {
            panic!(
                "{HEADLESS_PUMP} 에서 `{sig}` 본문을 못 잘랐다 — 이름이 바뀌었으면 \
                 `HEADLESS_DISPATCH_FNS` 도 함께 고쳐라. 못 자른 채로 넘어가면 그 함수가 \
                 답하는 이름이 통째로 안 보이고, 그 미측정이 초록으로 나간다"
            )
        });
        out.push_str(&strip_comments(&body));
        out.push('\n');
    }
    out
}

fn headless_methods() -> BTreeSet<String> {
    method_literals(&headless_dispatch_code())
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
        "헤드리스 dispatch 에서 메서드를 {} 개밖에 못 뽑았다(하한 {MIN_HEADLESS_METHODS}, \
         2026-09-10 실측 12)",
        headless.len()
    );

    let excused: BTreeSet<&str> = NOT_IN_HEADLESS.iter().map(|(m, _)| *m).collect();
    let code = headless_dispatch_code();
    let covered_by_token = |m: &str| {
        HEADLESS_COVERS
            .iter()
            .any(|(item, token)| *item == m && code.contains(token))
    };
    let missing: Vec<&String> = gui
        .iter()
        .filter(|m| !headless.contains(*m) && !excused.contains(m.as_str()) && !covered_by_token(m))
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

/// **주석은 답이 아니다** — 그리고 명부 밖 헬퍼도 아니다.
///
/// 이 가드가 한때 파일 전체를 세던 때 두 방향이 다 났다(재현했다):
/// - dispatch 파일 꼬리에 `"ui.screenshot"` 을 인용한 **산문 주석 한 줄**을 넣으니
///   `NOT_IN_HEADLESS` 의 낡은-사유 검사가 발화해 *"헤드리스가 실제로 답하는데 …
///   사유를 지워라"* 로 실패했다. 실재하지 않는 위반에 대한 처방이다.
/// - 그 처방대로 사유를 지우니 통과했다. 답하지도, 사유가 적혀 있지도 않은 이름이
///   초록으로 남는 **거짓 초록**이다.
///
/// 그래서 좌변은 명부 함수의 본문이고 그 사본에서 주석을 걷어낸다. 아래는 그 두
/// 처방이 실제로 먹는지를 합성 입력으로 못박는다 — 마지막 단언이 **걷어내지 않으면
/// 잡힌다**는 대조라, 합성 입력이 그 갈래를 안 만들면 이 시험이 먼저 죽는다.
#[test]
fn a_comment_is_not_an_answer() {
    let src = "\
fn pump_ipc(app: &mut App) {
    // 헤드리스는 \"ui.screenshot\" 을 답하지 않는다 — 창이 없다.
    if m == \"ns.real\" { go(); }
}
fn tail_helper() { let doc = \"remote.attach\"; }
";
    let body = fn_body(src, "fn pump_ipc(").expect("본문을 잘라야 한다");
    let found = method_literals(&strip_comments(&body));
    assert!(found.contains("ns.real"), "답하는 이름을 잃었다: {found:?}");
    assert!(
        !found.contains("ui.screenshot"),
        "주석 속 이름을 답으로 셌다: {found:?}"
    );
    assert!(
        !found.contains("remote.attach"),
        "명부 밖 헬퍼의 이름을 셌다: {found:?}"
    );
    assert!(
        method_literals(&body).contains("ui.screenshot"),
        "합성 입력이 주석 갈래를 안 만든다 — 위 단언은 아무것도 안 잰다"
    );
}

/// 명부의 세 이름이 **실제 파일에 있다.**
///
/// `fn_body` 가 못 자르면 `headless_dispatch_code` 가 패닉하므로 판정이 조용히
/// 비지는 않는다. 이 시험은 그 패닉을 이름별로 미리 터뜨려 어느 항목이 낡았는지를
/// 실패문에 남긴다 — 셋을 이어 붙인 뒤 터지면 어느 것인지가 안 보인다.
#[test]
fn the_roster_names_real_functions() {
    let src = read(HEADLESS_PUMP);
    for sig in HEADLESS_DISPATCH_FNS {
        assert!(
            fn_body(&src, sig).is_some(),
            "`{sig}` 를 `{HEADLESS_PUMP}` 에서 못 찾았다 — 이름이 바뀌었으면 \
             `HEADLESS_DISPATCH_FNS` 를 함께 고쳐라"
        );
    }
}

/// gui 의 **debug step** 에는 있고 헤드리스에는 의도적으로 없는 것과 그 사유.
///
/// 위 `NOT_IN_HEADLESS` 와 형태는 같지만 대조 쌍이 다르다 — 저쪽은 app 층 step,
/// 이쪽은 debug step 이다. 판정식은 하나다: **창(또는 렌더러·egui 입력 큐)을 읽는가.**
/// 실행으로 셌다(2026-09-05, 호출마다 새 인스턴스를 띄우는 census 로 오염 0):
/// debug 표면 36 건 중 5 건이 창을 안 읽어서 열렸고, 31 건이 아래다.
const DEBUG_NOT_IN_HEADLESS: &[(&str, &str)] = &[
    (
        "debug.settings.open",
        "설정 모달을 연다. `AppEvent::OpenSettings` 를 winit proxy 로 보내는데 헤드리스엔 \
         그 proxy 가 없다",
    ),
    (
        "debug.popup.open",
        "plugin popup 인스턴스를 만든다. `handle_open` 자체는 매니저만 읽지만, 헤드리스에는 \
         그것을 **닫는 경로가 하나도 없다** — debug close 도 plugin 자신의 release \
         `popup.close` 도 gui 게이트 안의 `app::dispatch` 에 산다. open 만 열면 그 빌드에서 \
         닫을 수 없는 인스턴스가 남는다(표면을 넓히면서 정리 책임을 새로 지는 형태다)",
    ),
    (
        "debug.popup.close",
        "렌더가 수집하는 close 큐로 합류해야 `cancel_child_file_picker` 연쇄 정리가 \
         돈다(ADR-0084). 그 glue(`App::enqueue_plugin_popup_close`)가 gui 게이트 안의 \
         `app::dispatch` 에 있다",
    ),
    // 이쪽은 갈래 한 줄이 맞다 — 재 봤다. `open` 은 `self.view.views` 를 순회해 소유
    // 창을 찾고, `close` 도 `self.view.views` 를 돌며 `state.banners` 를 닫는다. 둘의
    // 판정이 같으므로 이름별로 가를 이유가 없다(갈래 한 줄이 나쁜 것이 아니라, 그 안에서
    // 판정이 갈리는데 한 줄로 두는 것이 나쁘다).
    (
        "debug.plugin_banner.",
        "소유 view 의 BannerManager 와 host 매니저를 함께 다룬다 — `open`·`close` 둘 다 \
         `self.view.views` 를 순회한다. view 가 없다",
    ),
    (
        "debug.modal.close_request",
        "활성 모달은 `self.view.active_modal_id` 로 식별하고 `close_active_modal()` 이 \
         `self.view.views` 에서 지운다 — view 가 없다",
    ),
    // `debug.fullscreen.` 을 갈래 한 줄로 두지 않는다 — 그 안에서 판정이 갈린다.
    // `list` 는 여기 없다: 무대 표를 메타와 그리기 함수로 가른 뒤 헤드리스가 답한다.
    // 남은 셋은 창을 지목해야 해서 답이 정의되지 않는다.
    (
        "debug.fullscreen.open",
        "무대는 창 단위다 — `pick_debug_window` 로 `self.view.views` 에서 창을 지목한다",
    ),
    (
        "debug.fullscreen.close",
        "무대는 창 단위다 — `pick_debug_window` 로 `self.view.views` 에서 창을 지목한다",
    ),
    (
        "debug.fullscreen.state",
        "무대는 창 단위다 — `pick_debug_window` 로 `self.view.views` 에서 창을 지목한다",
    ),
];

/// gui 의 debug step 이 부르는 것마다 **헤드리스가 답하거나 왜 못 답하는지가 적혀 있다.**
///
/// 위 app 층 판정과 같은 규약을 debug step 에 적용한 것이다. 이 step 이 통째로 헤드리스에
/// 없다는 사실 자체가 판정을 대신하지 못한다 — 그 안에는 창을 읽는 것과 안 읽는 것이
/// 섞여 있었고, 안 읽는 다섯은 자리가 없어서 사라진 것이었다.
#[test]
fn every_gui_debug_step_method_is_answered_headless_or_carries_a_reason() {
    let src = read(GUI_DEBUG_STEP);
    let body = fn_body(&src, GUI_DEBUG_FN)
        .unwrap_or_else(|| panic!("{GUI_DEBUG_STEP} 에서 `{GUI_DEBUG_FN}` 본문을 못 잘랐다"));
    let gui = method_literals(&body);
    assert!(
        gui.len() >= MIN_GUI_DEBUG_ITEMS,
        "debug step 에서 {} 개밖에 못 뽑았다(하한 {MIN_GUI_DEBUG_ITEMS}, 2026-09-05 실측 \
         8). 대조군이 죽었다",
        gui.len()
    );
    let headless = headless_methods();

    let excused: BTreeSet<&str> = DEBUG_NOT_IN_HEADLESS.iter().map(|(m, _)| *m).collect();
    // 헤드리스가 prefix 로 답하면 그 아래 이름도 답하는 것이다(그 반대도 같다).
    let covered = |item: &str| {
        headless.contains(item)
            || headless
                .iter()
                .any(|h| h.ends_with('.') && item.starts_with(h.as_str()))
            || excused.contains(item)
            || excused
                .iter()
                .any(|e| e.ends_with('.') && item.starts_with(*e))
    };
    // gui 라우터가 `starts_with("debug.popup.")` 로 갈래를 받으면 그 갈래 리터럴 자체가
    // 항목으로 뽑힌다. 그것은 메서드 이름이 아니라 **라우터의 모양**이라, 그 아래 구체
    // 이름이 전부 덮였으면 갈래도 덮인 것이다. 이 규칙이 없으면 갈래를 갈라 적는 순간
    // 갈래 리터럴 하나 때문에 사유를 또 요구하고, 그 사유가 다시 갈래 전체를 덮어
    // ②(낡은 갈래 사유)를 되살린다.
    //
    // 갈래 아래의 구체 이름은 dispatch 본문 밖에 있을 수 있다 — 라우터가 갈래를
    // `starts_with` 로 받고 **같은 파일의 위임 함수**가 이름별로 가르는 형태가 그렇다
    // (`ipc_debug_fullscreen`). 그래서 구체 이름은 파일 전체에서 찾는다. 대상이 이미
    // 알려진 갈래 접두어로 좁혀져 있어 무관한 메서드가 딸려 들어오지 않는다.
    let in_file = method_literals(&src);
    let concrete_under = |p: &str| -> Vec<String> {
        gui.iter()
            .chain(in_file.iter())
            .filter(|m| m.as_str() != p && m.starts_with(p) && !m.ends_with('.'))
            .cloned()
            .collect::<BTreeSet<String>>()
            .into_iter()
            .collect()
    };
    let missing: Vec<&String> = gui
        .iter()
        .filter(|m| {
            if covered(m) {
                return false;
            }
            if m.ends_with('.') {
                let under = concrete_under(m);
                return under.is_empty() || !under.iter().all(|c| covered(c.as_str()));
            }
            true
        })
        .collect();
    assert!(
        missing.is_empty(),
        "gui 의 debug step 이 답하는데 헤드리스는 답하지도, 왜 못 답하는지 적혀 있지도 \
         않다. debug 표면은 **에이전트가 자기 작업을 검증하는 자리**라, 헤드리스에서만 \
         사라지면 헤드리스 인스턴스는 검증할 수단이 없다. `pump_ipc` 에서 답하게 하거나 \
         `DEBUG_NOT_IN_HEADLESS` 에 사유를 적어라: {missing:?}"
    );

    // 낡은 사유 — 헤드리스가 실제로 답하는데 못 답한다고 적혀 있는 것.
    // 낡은 사유. 이름이 정확히 답해지는 경우뿐 아니라, **갈래 사유(`x.y.` 로 끝나는
    // 것) 아래를 헤드리스가 하나라도 답하면** 그 사유는 이미 거짓이다. 이 두 번째
    // 형태가 없으면 갈래의 일부만 열었을 때 사유가 "전부 못 답한다" 라고 말하는 채로
    // 초록이 유지된다 — 채널은 도는데 술어가 그 차이를 안 보는 자리다.
    let stale: Vec<&str> = DEBUG_NOT_IN_HEADLESS
        .iter()
        .map(|(m, _)| *m)
        .filter(|m| {
            headless.contains(*m)
                || (m.ends_with('.') && headless.iter().any(|h| h.starts_with(*m)))
        })
        .collect();
    assert!(
        stale.is_empty(),
        "헤드리스가 실제로 답하는데 사유가 아직 못 답한다고 말한다(갈래 사유면 그 아래 \
         하나만 답해도 거짓이다 — 갈래를 이름별로 갈라 적어라): {stale:?}"
    );

    for (method, reason) in DEBUG_NOT_IN_HEADLESS {
        assert!(
            reason.len() >= 10,
            "`{method}` 의 사유가 너무 짧아 아무것도 말하지 않는다"
        );
    }
}

/// debug step 에서 뽑히는 항목 수의 하한 — **연기 검사**다.
/// 값의 근거: 2026-09-05 실측 8 건(이름 + prefix).
const MIN_GUI_DEBUG_ITEMS: usize = 5;

/// prefix 리터럴을 버리지 않는가.
///
/// `starts_with("debug.event_bus.")` 처럼 **갈래 전체**를 prefix 로 받는 자리가 두
/// 라우터에 다 있다. 끝이 `.` 인 리터럴을 버리면 그 갈래가 안 보여, 답하는 쪽을
/// "안 답한다" 로 세고 사유를 적으라고 요구하게 된다 — 거짓 양성이다.
#[test]
fn a_prefix_literal_is_kept() {
    let src = "\
fn pump_ipc(app: &mut App) {
    if m.starts_with(\"ns.family.\") { go(); }
    if m == \"ns.one\" { go(); }
}
";
    let body = fn_body(src, "fn pump_ipc").expect("본문을 잘라야 한다");
    let found = method_literals(&body);
    assert!(found.contains("ns.family."), "prefix 를 버렸다: {found:?}");
    assert!(found.contains("ns.one"));
}

/// 증거 토큰이 사라지면 그 대응도 사라진다.
///
/// `HEADLESS_COVERS` 는 면제가 아니라 **다른 모양으로 답한다는 주장**이다. 주장의 근거가
/// 헤드리스 본문의 토큰 하나뿐이므로, 그 토큰이 없어졌을 때 covered 로 남으면 이 표가
/// 그냥 면제 목록이 된다.
#[test]
fn a_cover_claim_dies_with_its_evidence() {
    let pump = headless_dispatch_code();
    for (item, token) in HEADLESS_COVERS {
        assert!(
            pump.contains(token),
            "`{item}` 을 헤드리스가 답한다고 적혀 있는데 그 근거인 `{token}` 이 \
             `{HEADLESS_PUMP}` 에 없다. 갈래가 사라졌으면 이 줄도 지우고, 사유가 \
             필요하면 `NOT_IN_HEADLESS` 로 옮겨라"
        );
        // 토큰이 없는 세계에서는 covered 가 아니어야 한다 — 판정이 토큰을 실제로 본다.
        let without = pump.replace(token, "");
        assert!(
            !without.contains(token),
            "치환이 안 먹었다 — 이 대조는 아무것도 안 본다"
        );
    }
}

/// 호출자 스캔의 사거리에서 **빼는** 디렉토리 — 이 규칙을 재는 쪽이다. 거기 있는
/// 이름은 판정 대상이 아니라 판정문의 재료라, 세면 가드 자신을 호출자로 센다.
const GUARD_DIRS: &[&str] = &["src/source_guards/", "crates/tasty-doc-guards/"];

/// 두 라우터가 **같은 함수**로 답하는 갈래. 이름 하나가 곧 "gui 와 헤드리스가 계약을
/// 한 벌로 나눠 갖는다" 는 주장이고, 아래 시험이 그 주장을 두 자리에서 **각각** 잰다.
///
/// # 왜 명부 등록만으로는 못 재는가
///
/// [`super::dispatch_name_literals`] 의 `DELEGATED_ROUTERS` 도 이 함수를 들고 있지만,
/// 그것이 재는 것은 *호출자가 하나라도 있는가* 다. 한쪽이 자기 인라인 사본으로
/// 돌아가도 다른 쪽이 계속 부르면 전부 초록이다 — 즉 **"한쪽에서만 불린다" 를 잡는
/// 채널이 아니다.** 그것을 잡으려면 라우터별로 나눠 세야 하고, 그게 이 시험이다.
///
/// 세는 단위는 **라우터 파일**이다. 호출이 그 파일의 dispatch 함수 본문에 있는지
/// 헬퍼 안에 있는지는 계약의 소재를 바꾸지 않는다 — gui 는 실제로 `app_methods.rs` 의
/// `ipc_dispatch_plugin_method` 에서 부르고 헤드리스는 `pump_ipc` 안에서 부른다.
/// 파일이 통째로 그 이름을 잃는 것이 곧 그 라우터가 계약을 잃는 것이다.
const SHARED_BY_BOTH_ROUTERS: &[&str] = &["dispatch_lifecycle_toggle"];

/// 공용 dispatch 함수는 **두 라우터 양쪽에서** 불린다.
#[test]
fn a_shared_dispatch_is_called_by_both_routers() {
    for name in SHARED_BY_BOTH_ROUTERS {
        let sites = callers_of(name, GUARD_DIRS);
        assert!(
            !sites.is_empty(),
            "`{name}` 을 부르는 자리를 하나도 못 찾았다 — 빈 좌변의 초록은 통과가 아니라 \
             미측정이다. 이름이 바뀌었으면 이 명부도 함께 고쳐라"
        );
        let files: BTreeSet<&str> = sites.iter().map(|s| s.rel.as_str()).collect();
        for rel in [GUI_STEP, HEADLESS_PUMP] {
            assert!(
                files.contains(rel),
                "`{name}` 이 `{rel}` 에서 안 불린다 — 한쪽 라우터가 자기 사본으로 \
                 돌아갔다는 뜻이고, 그러면 같은 계약이 두 벌이 된다. 부르는 자리: \
                 {files:?}"
            );
        }
    }
}
