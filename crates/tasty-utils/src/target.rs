//! 요청이 지목한 대상을 아무도 안 가졌을 때 돌려주는 **프로토콜 문구**.
//!
//! # 왜 leaf utils 에 있나
//!
//! 이 문장은 호스트와 plugin **양쪽**이 낸다. 호스트는 `request_target` 에서, plugin 은
//! 자기 핸들러에서 — 둘은 **별개 프로세스**라 타입을 공유할 수 없다. 그래서 한동안
//! 두 벌이 따로 살았고, 실제로 어긋났다: 호스트는 `(named by '<메서드>')` 에 요청의
//! 메서드 이름을 넣는데 복제본은 인자 이름(`'surface'`)을 박아 두어 **문장의 뜻이
//! 달랐다.** 소비자가 이 문구를 문자열로 가른다면 두 벌은 표류할 수밖에 없다.
//!
//! 그래서 정본을 **양쪽이 이미 의존하는 leaf crate** 로 내렸다. 바이트 동일이
//! 테스트의 단언이 아니라 **호출 구조**로 보장된다 — 같은 입력이면 같은 함수가 낸다.
//!
//! # 왜 `t()` 를 안 거치나
//!
//! 이건 사람에게 보이는 UI 문구가 아니라 **에이전트가 읽는 프로토콜 문자열**이다.
//! 로케일마다 다른 바이트를 내면 그 순간 위의 보장이 깨진다. lang 파일에 키를 두는
//! 것은 "번역 가능하다" 는 선언이고, 이 문장에 대해 그 선언은 거짓이다.

/// 요청이 지목한 대상을 **아무 engine 도 안 가졌을 때** 돌려줄 메시지.
///
/// 예전에는 이 자리에서 포커스된 창으로 넘겼다. 그러면 대상을 잘못 적은 요청이
/// **다른 창에서 조용히 성공한다** — 실측(2026-09-05): 존재하지 않는 `workspace_id`
/// 를 실은 `workspace.create` 가 포커스된 창에 워크스페이스를 만들고 성공을 돌려줬다.
/// `docs/design/policies/focus.md` 의 "silent fallback 금지" 가 그것이다.
///
/// 문구가 창을 말하지 않는 이유: 헤드리스에도 같은 판정이 걸리는데 거기엔 창이 없다.
/// 두 조합에서 참인 문장이어야 한다.
///
/// `kind` 는 리소스 종류의 표시 이름(`"surface"` · `"workspace category"` 등),
/// `method` 는 **그 대상을 이름으로 지목한 요청의 메서드**다 — 그 판정을 내리려고
/// 내부적으로 부른 호출의 이름이 아니다.
pub fn unowned_target_message(kind: &str, id: u64, method: &str) -> String {
    format!(
        "no live {kind} {id} (named by '{method}'); \
         list the resource to get a live id — a named target is never resolved by focus"
    )
}

/// 이 메시지가 **"그 대상은 살아 있지 않다"** 는 거절인가 — [`unowned_target_message`] 의
/// 역방향.
///
/// 같은 모듈에 두는 이유: 이 판정은 위 함수가 만드는 바이트에 의존한다. 판정을 소비자
/// 쪽에 손으로 적어 두면 문구가 바뀔 때 **조용히** 안 맞게 된다 — 이 판정을 쓰는 곳이
/// plugin 이라, 그 침묵은 "대상이 살아 있다" 로 떨어진다(좁게 틀리는 방향이라 안전하지만
/// 기능은 죽는다). 왕복을 아래 테스트가 고정한다.
///
/// `method` 는 안 본다 — 거절의 **주체**는 판정에 무관하고, 그 자리에는 판정하는 쪽이
/// 모르는 이름(자기가 부른 내부 호출)이 들어갈 수 있다.
pub fn says_no_live_target(message: &str, kind: &str, id: u64) -> bool {
    message.contains(&format!("no live {kind} {id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_names_the_kind_the_id_and_the_method() {
        assert_eq!(
            unowned_target_message("surface", 424242, "agent_stream.turn_start"),
            "no live surface 424242 (named by 'agent_stream.turn_start'); list the resource \
             to get a live id — a named target is never resolved by focus"
        );
    }

    /// 만드는 쪽과 알아보는 쪽이 붙어 있다 — 문구를 바꾸면 이 왕복이 먼저 깨진다.
    #[test]
    fn what_this_module_writes_this_module_can_read_back() {
        let msg = unowned_target_message("surface", 424_242, "agent_stream.unwatch");
        assert!(says_no_live_target(&msg, "surface", 424_242));
        // 양방향 — 다른 id·다른 종류는 안 걸린다.
        assert!(!says_no_live_target(&msg, "surface", 424_243));
        assert!(!says_no_live_target(&msg, "pane", 424_242));
        assert!(!says_no_live_target(
            "host call timed out",
            "surface",
            424_242
        ));
    }

    /// 여러 낱말 종류도 그대로 실린다(`Kind::label` 이 그런 값을 낸다).
    #[test]
    fn a_multi_word_kind_is_not_mangled() {
        assert!(
            unowned_target_message("workspace category", 3, "workspace_category.rename")
                .starts_with("no live workspace category 3 (named by 'workspace_category.rename')")
        );
    }
}
