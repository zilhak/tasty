//! 모달 상태의 **쓰는 자리가 둘뿐**이라는 것을 고정한다.
//!
//! `ui.state` 가 내는 `modal_open`/`active_modal_id` 는 `View::active_modal_id` 의 사본이다.
//! 사본을 둔 이유는 조회 경로(`AppState` 만 받는다)가 `View` 에 안 닿기 때문이고, `&View` 를
//! 전파하면 View 가 없는 헤드리스 호출자에서 **"모달 없음" 과 "View 가 없음" 이 같은 모양**이
//! 되기 때문이다.
//!
//! 사본은 원본과 어긋날 수 있다. 어긋나지 않는 **유일한 근거**가 "원본을 세우는 그 자리에서
//! 함께 쓴다" 이므로, 그 근거를 값으로 지킨다. 누가 다른 곳에서 원본이나 사본을 건드리면
//! 여기서 걸린다 — 안 걸리면 그날부터 `ui.state` 는 조용히 틀린 값을 낸다.

use std::path::Path;

use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};

/// 이 단정들은 **부정형**이다 — "위반 0" 과 "아무 파일도 안 읽었다" 가 같은 초록이다.
/// 그래서 인구의 하한을 순회가 자기 실패문과 함께 강제한다. 직접 `read_dir` 을 쓰지 않는
/// 이유이기도 하다: 공용 순회를 쓰면 하한을 빠뜨릴 수 없다.
const SRC_FLOOR: Floor = Floor {
    min: 587,
    // 좌변의 사실은 `tasty_doc_guards::floored_walk::populations::SRC_RS` 하나가 갖는다 — 이 값을 여기에도 적어 두었을
    // 때 두 자리가 591 과 598 로 갈렸고, 어느 쪽도 그날의 실제 수가 아니었다.
    measured: tasty_doc_guards::floored_walk::populations::SRC_RS.measured,
    measured_on: tasty_doc_guards::floored_walk::populations::SRC_RS.measured_on,
    counted_on: tasty_doc_guards::floored_walk::populations::SRC_RS.counted_on,
    why_this_gap: "이 모수는 `src/` 의 `.rs` 개수다. **이 가드의 순회 생존 계기는 이 \
                   하한 하나뿐이라**(형제 가드 `layering` 과 달리 깊이 하한도 앵커도 없다) \
                   부분 사망을 이것만 본다. 그래서 여유를 움직임의 단위에서 파생시킨다: \
                   `src/` 를 건드린 1257 커밋(2026-07-01~09-08)에서 감소 사건은 13 건이고 \
                   가장 큰 것이 **9**(2026-07-02 의 렌더 경로 삭제), 나머지 12 건은 2 \
                   이하다. 여유 18 은 그 최대 정리가 **두 번 겹치는** 폭이다 — 한 회차에 \
                   정리가 둘 들어오는 것까지 견디고 열아홉째 파일부터 짖는다. ★ 단위 밖 \
                   사건 하나를 이름으로 적는다: 크레이트 분해가 `src/` 에서 수십 개를 \
                   한꺼번에 옮기는 것. 그 폭은 **이 창에서 관측되지 않았다**(다른 모수에서 \
                   난 대이동을 이 모수의 배수로 쓰지 않는다 — 같은 낱말이 모수마다 40 배 \
                   다른 폭을 뜻한다는 것이 ADR-0226 의 근거다). 그 사건이 나면 이 하한이 \
                   먼저 짖고 그것이 옳다: 실패문은 하한을 내리라 하지 않고 다시 재라고 한다.",
};

fn sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let found: Vec<Walked> = walk_with_floor(
        &root.join("src"),
        root,
        &SRC_FLOOR,
        Descend::SkipBuildCaches,
        &|f| f.rel.ends_with(".rs"),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    found
        .into_iter()
        .map(|f| {
            let t = std::fs::read_to_string(&f.path)
                .unwrap_or_else(|e| panic!("read {}: {e}", f.path.display()));
            (f.rel, t)
        })
        .collect()
}

const OWNER: &str = "src/app/modal.rs";

/// 원본을 **쓰는** 자리인가 — 읽는 자리(`if let Some(id) = self.view.active_modal_id`)는 아니다.
fn writes_the_original(t: &str) -> bool {
    t.contains("view.active_modal_id = ") || t.contains("view.active_modal_id.take")
}

fn writes_the_mirror(t: &str) -> bool {
    t.contains("state.active_modal_id = ")
}

/// 사본의 짝 — **어느** 모달인가. 창 id 와 같은 자리에서 같이 움직여야 한다.
fn writes_the_kind(t: &str) -> bool {
    t.contains("state.active_modal_kind = ")
}

/// ★ 원본을 옮기는 **모든** 파일이 사본도 함께 옮긴다.
///
/// 처음엔 "쓰는 파일이 하나뿐" 으로 적었는데 그건 **거짓이었다** — `app/event_handler.rs` 의
/// macOS 최소화 경로가 모달을 drop 하면서 원본을 지운다. 그 자리를 안 고쳤으면 사본은
/// `Some` 인 채로 파킹돼 dock 복귀 때 "모달이 열려 있다" 고 말하고, 그 모달은 이미 없으므로
/// **되돌릴 경로가 없다.**
///
/// 그래서 단정을 "한 파일" 이 아니라 **"짝지어 움직인다"** 로 세운다. 사본을 둔 설계가 서는
/// 근거가 그것이고, 파일 수는 그 근거가 아니었다.
#[test]
fn every_file_that_moves_the_original_moves_the_mirror_too() {
    let offenders: Vec<String> = sources()
        .into_iter()
        .filter(|(_, t)| writes_the_original(t) && !writes_the_mirror(t))
        .map(|(rel, _)| rel)
        .collect();
    assert!(
        offenders.is_empty(),
        "`view.active_modal_id` 를 옮기면서 `AppState` 쪽 사본을 안 옮기는 파일이 있다: \
         {offenders:?}. 그 자리를 지나면 `ui.state` 의 `modal_open` 이 실제와 어긋난 채 남는다 — \
         특히 사본만 `Some` 으로 남는 방향은 되돌릴 경로가 없다."
    );
}

/// 사본을 쓰는 파일은 반드시 원본도 쓴다 — 반대 방향.
///
/// 이쪽이 없으면 "사본만 따로 만지는" 자리가 생겨도 안 걸린다. 그러면 사본은 더 이상
/// 원본의 거울이 아니라 **두 번째 진실**이 되고, 둘이 갈릴 때 어느 쪽이 맞는지 아무도 모른다.
#[test]
fn nobody_touches_the_mirror_alone() {
    let offenders: Vec<String> = sources()
        .into_iter()
        .filter(|(rel, t)| rel != "src/state.rs" && writes_the_mirror(t) && !writes_the_original(t))
        .map(|(rel, _)| rel)
        .collect();
    assert!(
        offenders.is_empty(),
        "원본 없이 사본만 건드리는 파일이 있다: {offenders:?}. 사본은 거울이지 \
         두 번째 진실이 아니다."
    );
}

/// 여닫는 자리는 여전히 `app/modal.rs` 하나이고, 사본을 **두 번** 쓴다.
#[test]
fn the_open_close_pair_still_lives_in_one_file() {
    let files = sources();
    let owner = files
        .iter()
        .find(|(rel, _)| rel == OWNER)
        .map(|(_, t)| t.as_str())
        .unwrap_or_else(|| panic!("`{OWNER}` 를 모수에서 못 찾았다 — 파일이 옮겨졌다"));
    let writes = owner.matches("state.active_modal_id = ").count();
    assert_eq!(
        writes, 2,
        "여닫는 파일이 사본을 두 번 써야 한다(열기 · 닫기). 지금 {writes} 곳이다."
    );
}

/// 그리고 그 둘은 각각 원본을 건드리는 함수 **안**에 있다.
#[test]
fn each_mirror_write_sits_inside_the_function_that_moves_the_original() {
    let files = sources();
    let owner = files
        .iter()
        .find(|(rel, _)| rel == OWNER)
        .map(|(_, t)| t.as_str())
        .expect("owner 파일");
    let open_at = owner
        .find("fn open_modal(")
        .expect("`open_modal` 을 못 찾았다");
    let close_at = owner
        .find("fn close_active_modal(")
        .expect("`close_active_modal` 을 못 찾았다");
    assert!(
        open_at < close_at,
        "두 함수의 순서가 바뀌었다 — 아래 구간 판정이 무의미해진다"
    );

    let opening = &owner[open_at..close_at];
    let closing = &owner[close_at..];
    assert!(
        opening.contains("state.active_modal_id = Some("),
        "모달을 여는 함수가 사본을 안 세운다 — 그러면 `ui.state` 는 모달이 떠도 없다고 말한다"
    );
    assert!(
        closing.contains("state.active_modal_id = None"),
        "모달을 닫는 함수가 사본을 안 지운다 — 그러면 닫힌 뒤에도 열려 있다고 말한다. \
         이쪽이 더 나쁘다: 값이 **영영** 안 돌아온다"
    );
}

/// ★ 종류는 창 id 와 **짝이다** — 한쪽만 움직이는 파일이 있으면 안 된다.
///
/// 갈리는 두 방향이 둘 다 나쁘고, 나쁜 방향이 서로 다르다:
/// · id 만 세우고 종류를 안 세우면 → "무언가 떠 있는데 무엇인지는 **옛 값**" 이다.
///   직전에 열렸던 모달의 종류가 그대로 남아, 그 종류를 기다리는 시험이 **자기가 안 연
///   창을 보고 통과한다.** 이쪽이 더 나쁘다 — 초록이 거짓이 된다.
/// · 종류만 세우고 id 를 안 세우면 → `modal_open` 이 거짓인데 종류는 `Some` 이다.
///   그 조합은 어떤 소비자도 안 다루는 모양이다.
#[test]
fn the_kind_moves_with_the_id() {
    let offenders: Vec<String> = sources()
        .into_iter()
        .filter(|(rel, t)| rel != "src/state.rs" && (writes_the_mirror(t) != writes_the_kind(t)))
        .map(|(rel, _)| rel)
        .collect();
    assert!(
        offenders.is_empty(),
        "`state.active_modal_id` 와 `state.active_modal_kind` 중 한쪽만 건드리는 파일이 있다: \
         {offenders:?}. 둘은 짝이다 — id 만 세우면 종류가 직전 모달의 값으로 남고, \
         그 종류를 기다리는 시험이 자기가 안 연 창을 보고 통과한다."
    );
}

/// 그리고 그 짝도 여닫는 두 함수 **안**에 각각 있다.
#[test]
fn each_kind_write_sits_inside_the_function_that_moves_the_original() {
    let files = sources();
    let owner = files
        .iter()
        .find(|(rel, _)| rel == OWNER)
        .map(|(_, t)| t.as_str())
        .expect("owner 파일");
    let open_at = owner
        .find("fn open_modal(")
        .expect("`open_modal` 을 못 찾았다");
    let close_at = owner
        .find("fn close_active_modal(")
        .expect("`close_active_modal` 을 못 찾았다");
    assert!(open_at < close_at, "두 함수의 순서가 바뀌었다");

    assert!(
        owner[open_at..close_at].contains("state.active_modal_kind = Some("),
        "모달을 여는 함수가 종류를 안 세운다 — `ui.state` 는 무언가 떠 있다고만 말하고 \
         무엇인지는 직전 값으로 답한다"
    );
    assert!(
        owner[close_at..].contains("state.active_modal_kind = None"),
        "모달을 닫는 함수가 종류를 안 지운다 — 닫힌 뒤에도 그 종류가 남는다"
    );
}
