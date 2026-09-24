//! 호스트와 plugin이 공유하는 대상 부재 프로토콜 문구.
//! 소비자가 문자열을 판별하므로 번역하지 않는다. 생성과 판별을 함께 유지한다.

/// 요청의 명시적 대상을 가진 engine이 없을 때 반환한다. 포커스된 다른 대상으로 대체하지 않는다.
/// kind는 리소스 표시 이름, method는 대상을 지정한 원래 요청 메서드다.
pub fn unowned_target_message(kind: &str, id: u64, method: &str) -> String {
    format!(
        "no live {kind} {id} (named by '{method}'); \
         list the resource to get a live id — a named target is never resolved by focus"
    )
}

/// 대상 부재 문구가 포함됐는지 검사한다. method는 비교하지 않는다.
/// 정확한 구조 파싱이 아니라 생성 함수와 같은 접두 문구를 찾는 검사다.
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
