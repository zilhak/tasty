//! egui-mesh surface 의 **bootstrap 순서**를 못 박는다 — `surface.create` 는 그 surface 의
//! 첫 `surface.set_context` 보다 **먼저** 같은 plugin 요청 채널에 놓여야 한다.
//!
//! # 계약
//!
//! 두 메시지는 한 plugin 프로세스의 같은 `req_tx`(mpsc)로 나가므로 **넣은 순서대로**
//! 도착한다. create 가 먼저 가야 plugin 이 생성 params(예: markdown 의 `file`)를 손에
//! 쥔 채 첫 set_context 를 렌더한다. 뒤집히면 plugin 은 자기가 무엇을 그리는지 모르는
//! 상태로 한 프레임을 그린다 — 빈 화면이거나 기본값으로 그린 화면이고, 다음 프레임에
//! 조용히 고쳐지므로 재현이 어렵다. 채널 자체는 정상이고 에러도 없다.
//!
//! # 왜 좌변을 도출하는가 — 산문이 든 명부가 이미 틀렸다
//!
//! `crates/tasty-host-plugin/src/manager/events.rs` 의 doc 은 호출자를 **하나**로 적고
//! 있었다: *"caller(`MainView::forward_egui_mesh_context`)가 … 1회 호출한다."*
//! 실측하면 **셋**이다 — `forward_egui_mesh_context` ·
//! `forward_mesh_to_attach_subscribers` · `forward_mesh_frames_for_engine`. 산문이
//! 명부를 들면 명부가 늘 때 산문은 안 따라오고, 그 사실을 아무도 안 본다.
//!
//! 그래서 이 가드는 자리를 이름으로 지목하지 않고 **create 호출을 스캔해서** 좌변을
//! 만든다. 넷째 호출자가 생기면 자동으로 판정 대상이 된다. 대신 그 대가로 잃는 것이
//! 있다 — 아래 "초록이 뜻하지 않는 것" 의 마지막 항목.
//!
//! # 왜 이 자리가 정적으로만 판정 가능한가
//!
//! 채널의 FIFO 성질은 런타임으로 잴 수 있다(그 크레이트에 stub `PluginProcess` 가 있고
//! `processes` 가 `pub` 이다). 그런데 그건 `mpsc` 의 명제지 이 계약이 아니다. 계약은
//! **호출자가 어느 순서로 넣는가**이고, 그 호출자는 `MainView` 와 `CoreState` 를 들고
//! 매 프레임 도는 렌더 경로다 — 실행으로 관측하려면 앱을 세워야 한다.
//!
//! # 초록이 뜻하는 것
//!
//! **create 를 부르는 함수마다, 같은 함수 안에 set_context 호출이 있고 create 가 그보다
//! 앞선다.** 그것뿐이다.
//!
//! 초록이 뜻하지 **않는** 것:
//!
//! - **두 호출이 같은 surface 를 가리킨다는 뜻이 아니다.** 인자는 안 본다. 한 함수가
//!   surface A 를 create 하고 surface B 에 set_context 를 보내도 통과한다.
//! - **create 가 실제로 먼저 실행된다는 뜻이 아니다.** 조건 분기·루프 안이면 텍스트
//!   순서와 실행 순서가 갈린다.
//! - **한 번만 보낸다는 뜻이 아니다.** 중복 create 는 안 센다.
//! - **호출을 헬퍼 뒤로 묶으면 좌변이 한 칸 내려간다**(R1057 형태). 둘 다 같은 헬퍼로
//!   옮기면 그 헬퍼가 좌변이 되어 여전히 판정되지만, create 만 옮기면 그 헬퍼에
//!   set_context 가 없어 **빨개진다**(안전한 방향). 반대로 set_context 만 옮기면 원래
//!   함수에 create 만 남아 역시 빨개진다.
//! - **두 호출이 같은 surface 를 가리킨다는 뜻은 여전히 아니다.** 첫 인자(plugin) 말고는
//!   안 본다.
//! - **한 함수의 set_context 가 여럿이면 가장 이른 것 하나만 짝으로 본다.** 나머지는
//!   미측정이다. 이 레포에 그 실물이 있다 — `src/view/main/egui_mesh.rs` 의
//!   `fn forward_egui_mesh_context` 는 set_context 를 두 자리에서 보내고(:490 · :523),
//!   뒤쪽은 create 와 짝이 없는 pending_full 재전송이라 이 계약의 대상이 아니다.
//!   오늘은 create 가 둘보다 앞이라 판정이 맞아떨어지지만, **두 루프의 순서가 바뀌면
//!   무관한 set_context 가 짝으로 잡혀 거짓 빨강이 난다.**
//!
//! # 이 한계가 왜 텍스트로는 안 닫히는가 — 그리고 무엇을 재면 닫히는가
//!
//! 위 두 자리를 갈라내려면 "이 `plugin_id` 와 저 `plugin_id` 가 같은 바인딩인가" 를
//! 물어야 하는데, 그 둘은 **이름이 같고 바인딩이 다르다**(:511 이 셰도잉한다). 텍스트
//! 동일성으로는 원리적으로 답이 안 나온다 — 이름이 같다는 사실이 곧 반대 증거이기
//! 때문이다.
//!
//! **재면 깨진다.** 필요한 것은 이름 해석(스코프)이다: 함수 본문의 `let` 위치와 블록
//! 경계를 잡아 각 사용처를 가장 가까운 선행 바인딩에 귀속시키면, 두 `plugin_id` 가
//! 서로 다른 바인딩임이 **값으로** 나온다. 그러면 짝짓기를 "가장 이른 set_context" 가
//! 아니라 "같은 바인딩을 넘기는 set_context" 로 다시 정의할 수 있고 위 거짓 빨강도
//! 함께 사라진다. 블록 경계를 세는 도구는 이 모듈이 이미 쓴다(`fn_spans` 가
//! `block_after` 로 함수 본문을 자른다) — **불가능해서 안 한 것이 아니라 아직 안 잰
//! 것이다.** 지금 그것을 안 지은 근거는 좌변이 3 쌍이고 그중 이 형태가 1 건이라,
//! 스코프 계산을 새로 들이는 비용이 그 1 건보다 크다는 판단이다. 좌변이 늘거나 위
//! 거짓 빨강이 실제로 나면 그 판단이 뒤집힌다.
//!
//! 재바인딩 검사가 `create` 와 `set` **사이 구간**만 보는 것도 같은 이유로 옳다. 구간
//! 밖의 셰도잉(:511)은 그 쌍이 넘기는 값에 영향이 없다 — 검사를 함수 전체로 넓히면
//! 오늘 트리가 그 자리에서 빨개지고, 그것은 결함이 아니라 거짓 빨강이다.
//!
//! # FIFO 전제 — 우연이 아니라 보장이고, 그 보장을 읽는다
//!
//! 두 메시지가 같은 순서로 도착한다는 것은 **같은 plugin 프로세스의 같은 `req_tx`** 로
//! 나갈 때만 성립한다. 세 자리를 재 보니 전부 첫 인자로 **같은 지역 바인딩**을 넘기고
//! 있었고(`&plugin_id`), 두 호출 사이에 그 이름을 다시 묶는 자리가 **0** 이었다. 즉
//! 전제가 우연이 아니라 코드 모양으로 보장돼 있다 — 그러면 그 모양을 읽는 것이
//! 판정이다. [`the_two_sends_address_the_same_plugin`] 이 그 자리를 본다.
//!
//! 그래도 남는 것: 같은 이름이 **같은 값**이라는 보장은 이름이 그 사이에 다시 묶이지
//! 않는다는 사실에서 온다. 그래서 재바인딩(`let <이름>` · `<이름> =`)도 함께 본다.
//!
//! 그 보장이 성립하려면 인자가 **경로**여야 한다. 호출식(`x.plugin_id()`)·인덱싱은 두
//! 번 써서 같은 값이 나온다는 보장이 없어, 두 인자가 텍스트로 같아도 값이 같다는 결론이
//! 안 나온다. 그래서 이 판정기는 그 자리를 초록으로 넘기지 않고 **빨개진다** — 판정
//! 불가를 통과로 접지 않는 것이고, 처방은 그 값을 지역 변수에 한 번 묶어 두 호출에 같은
//! 이름으로 넘기는 것이다(그러면 전제가 코드 모양으로 돌아온다). 오늘 좌변 세 자리는
//! 전부 경로(`&plugin_id`)라 이 요구가 아무것도 안 움직인다.
//!
//! 비교는 `&` · `*` · `mut` 를 벗기고 한다. 그 셋은 값의 차이가 아니라 표기의 차이라,
//! 원문끼리 견주면 한쪽만 `&` 를 붙인 것이 **거짓 빨강**이 된다. 이 레포에 그 표기가
//! 이미 갈려 있다 — 같은 파일 `forward_egui_mesh_context` 안에서 한 자리는 `&plugin_id`
//! 를, 다른 자리는 `plugin_id` 를 넘긴다.

use std::path::{Path, PathBuf};

use super::{enclosing_fn, first_arg, fn_spans, line_of, mask_non_code, rust_sources};

/// bootstrap 을 여는 호출.
const CREATE: &str = ".send_egui_mesh_surface_create(";
/// 그 뒤에 와야 하는 호출.
const SET_CONTEXT: &str = ".send_surface_set_context(";
/// 두 메서드가 **정의**된 파일 — 여기 있는 이름은 호출이 아니다.
const SENDER_DEF: &str = "crates/tasty-host-plugin/src/manager/events.rs";
/// 가드가 자기 상수를 호출로 세지 않도록 빼는 디렉토리.
const GUARD_DIRS: &[&str] = &["src/source_guards/", "crates/tasty-doc-guards/"];

fn rel_str(rel: &Path) -> String {
    rel.to_string_lossy().replace('\\', "/")
}

/// create 를 **부르는** 파일들 — 곧 bootstrap 을 여는 자리.
fn bootstrap_files() -> Vec<(PathBuf, String)> {
    rust_sources()
        .into_iter()
        .filter(|(rel, _)| {
            let rel = rel_str(rel);
            rel != SENDER_DEF && !GUARD_DIRS.iter().any(|dir| rel.starts_with(dir))
        })
        .map(|(rel, src)| (rel, mask_non_code(&src)))
        .filter(|(_, masked)| masked.contains(CREATE))
        .collect()
}

/// `&mut foo` · `&foo` · `*foo` 에서 이름만. 이 셋은 **값 동일성을 안 깨뜨리는**
/// 접두라 비교 전에 벗긴다 — 한쪽만 `&` 를 붙이는 것은 값의 차이가 아니다.
fn binding_name(arg: &str) -> &str {
    let mut s = arg.trim();
    loop {
        let t = s.trim_start_matches(['&', '*']).trim_start();
        let t = t.strip_prefix("mut ").unwrap_or(t).trim_start();
        if t == s {
            return s;
        }
        s = t;
    }
}

/// 인자가 **경로**인가 — 식별자와 `.` · `::` 만으로 이루어졌는가.
///
/// 경로가 아닌 것(호출식 `x.plugin_id()` · 인덱싱 `v[i]` · 연산식)은 두 번 써서 같은
/// 값이 나온다는 보장이 없다. 그런 자리에서는 텍스트 동일성이 값 동일성을 **함의하지
/// 않으므로**, 이 판정기는 초록을 주지 않고 빨개진다 — 판정 불가를 통과로 접지 않는
/// 것이 이 레포가 게이트 rc=2 를 다루는 방식과 같다.
fn is_path(arg: &str) -> bool {
    let s = binding_name(arg);
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == ':')
}

#[test]
fn the_population_is_not_empty() {
    let files: Vec<String> = bootstrap_files().iter().map(|(r, _)| rel_str(r)).collect();
    assert!(
        !files.is_empty(),
        "`{CREATE}` 를 부르는 자리를 하나도 못 찾았다 — 이름이 바뀌었거나 스캔 루트가 \
         깨졌다. 빈 좌변의 초록은 통과가 아니라 미측정이다"
    );
}

#[test]
fn every_mesh_bootstrap_sends_create_before_the_first_set_context() {
    let mut checked = 0usize;
    for (rel, masked) in bootstrap_files() {
        let rel = rel_str(&rel);
        let spans = fn_spans(&masked);
        for (create, _) in masked.match_indices(CREATE) {
            let (name, open, close, _) = enclosing_fn(&spans, create).unwrap_or_else(|| {
                panic!(
                    "{rel}:{} 의 `{CREATE}` 를 품는 함수를 못 찾았다 — 못 자른 상태의 \
                     초록은 통과가 아니니 함수 절단을 함께 고쳐라",
                    line_of(&masked, create)
                )
            });
            let set = masked[*open..*close]
                .find(SET_CONTEXT)
                .map(|p| p + *open)
                .unwrap_or_else(|| {
                    panic!(
                        "{rel} 의 `fn {name}` 이 `{CREATE}` 를 부르면서 `{SET_CONTEXT}` 를 \
                         안 부른다. 호출을 헬퍼로 갈랐다면 이 가드의 좌변이 그 갈림을 \
                         못 따라간 것이니 상수를 같이 고쳐라 — 두 호출이 서로 다른 \
                         함수로 흩어지면 순서를 정하는 자리가 없어진다"
                    )
                });
            assert!(
                create < set,
                "{rel} 의 `fn {name}`: `{CREATE}`(줄 {}) 가 `{SET_CONTEXT}`(줄 {}) 보다 \
                 뒤에 있다. 같은 req 채널이라 넣은 순서대로 도착하므로, plugin 이 생성 \
                 params 를 못 받은 채 첫 프레임을 그린다 — 빈 화면이 한 프레임 스치고 \
                 다음 프레임에 조용히 고쳐져서 재현이 어렵다",
                line_of(&masked, create),
                line_of(&masked, set),
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "create 호출을 하나도 안 봤다 — 위 모수 시험이 초록인데 여기서 0 이면 함수 \
         절단이 깨진 것이다"
    );
}

/// FIFO 전제를 읽는다 — 두 호출이 **같은 plugin** 을 가리키는가. 다르면 두 메시지가
/// 서로 다른 프로세스의 서로 다른 채널로 나가고, 순서 계약 자체가 성립하지 않는다.
#[test]
fn the_two_sends_address_the_same_plugin() {
    let mut checked = 0usize;
    for (rel, masked) in bootstrap_files() {
        let rel = rel_str(&rel);
        let spans = fn_spans(&masked);
        for (create, _) in masked.match_indices(CREATE) {
            let Some((name, open, close, _)) = enclosing_fn(&spans, create) else {
                continue;
            };
            let Some(set) = masked[*open..*close].find(SET_CONTEXT).map(|p| p + *open) else {
                continue; // 짝이 없는 것은 위 시험이 이미 빨갛게 만든다.
            };
            let a = first_arg(&masked, create).unwrap_or_else(|| {
                panic!(
                    "{rel}:{} 의 create 인자를 못 읽었다",
                    line_of(&masked, create)
                )
            });
            let b = first_arg(&masked, set).unwrap_or_else(|| {
                panic!(
                    "{rel}:{} 의 set_context 인자를 못 읽었다",
                    line_of(&masked, set)
                )
            });
            for (which, arg, at) in [("create", &a, create), ("set_context", &b, set)] {
                assert!(
                    is_path(arg),
                    "{rel} 의 `fn {name}`: {which} 의 첫 인자가 경로가 아니다 — `{arg}` \
                     (줄 {}). 호출식·인덱싱은 두 번 써서 같은 값이 나온다는 보장이 없어 \
                     두 인자가 텍스트로 같아도 같은 plugin 이라고 못 읽는다. 그 값을 \
                     **지역 변수에 한 번 묶어서** 두 호출에 같은 이름으로 넘겨라 — 그러면 \
                     전제가 코드 모양으로 보장되고 이 판정기가 그 모양을 읽는다",
                    line_of(&masked, at)
                );
            }
            // `&`·`*`·`mut` 를 벗기고 비교한다. 한쪽만 `&` 를 붙이는 것은 값의 차이가
            // 아닌데, 원문끼리 견주면 그것이 빨강이 된다(거짓 빨강).
            let (a, b) = (binding_name(&a), binding_name(&b));
            assert_eq!(
                a, b,
                "{rel} 의 `fn {name}`: create 는 `{a}` 로, set_context 는 `{b}` 로 보낸다. \
                 서로 다른 plugin 이면 두 메시지가 서로 다른 채널로 나가므로 순서 계약이 \
                 애초에 성립하지 않는다 — 같은 함수 안이라는 것만으로는 아무 보장이 없다"
            );
            let id = a;
            let span = &masked[create..set];
            let rebound = span.contains(&format!("let {id}"))
                || span.match_indices(id).any(|(i, _)| {
                    let rest = span[i + id.len()..].trim_start();
                    rest.starts_with('=') && !rest.starts_with("==")
                });
            assert!(
                !rebound,
                "{rel} 의 `fn {name}`: 두 호출 사이에서 `{id}` 가 다시 묶인다. 인자 텍스트가 \
                 같아도 값이 같다는 보장이 사라진다 — 같은 이름이 같은 plugin 을 뜻하는 것은 \
                 그 사이에 다시 묶이지 않을 때뿐이다",
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "짝지어진 create/set_context 를 하나도 안 봤다");
}
