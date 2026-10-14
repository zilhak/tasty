//! 어느 소유자도 받지 않은 worker 완료의 halt 사유. 소유자 장부의 불변식 위반이라 위반한 요청을 사후에
//! 특정할 수 있도록 ticket·응답 종류·마지막 command 제출 종류를 남긴다.
use super::*;

/// command ticket 별 마지막 제출을 보관하는 수. 정산된 command 의 늦은 완료를 설명하려고 Pending 보다
/// 오래 남긴다. 항목은 ticket 과 정적 문자열이라 크기가 고정된다.
const SUBMISSION_LOG: usize = 256;
/// halt 사유에 넣는 worker 오류 문구의 최대 문자 수.
const ERROR_EXCERPT: usize = 160;
/// 기록 범위 밖이거나 command 가 아닌 ticket 의 표기.
const NO_SUBMISSION: &str = "none recorded";

#[derive(Default)]
pub(super) struct CommandSubmissions {
    recent: VecDeque<(u64, &'static str)>,
}

impl CommandSubmissions {
    pub(super) fn record(&mut self, ticket: u64, work: &'static str) {
        if self.recent.len() == SUBMISSION_LOG {
            self.recent.pop_front();
        }
        self.recent.push_back((ticket, work));
    }

    fn last(&self, ticket: u64) -> Option<&'static str> {
        self.recent
            .iter()
            .rev()
            .find_map(|(recorded, work)| (*recorded == ticket).then_some(*work))
    }
}

impl JournalApplication {
    /// opening 까지 확인한 뒤에도 소유자가 없는 완료를 설명한다. engine binding 응답은 opening 장부와의
    /// 불일치로, 그 밖의 응답은 소유자 없는 완료로 구분한다.
    pub(super) fn unowned_completion(
        &self,
        ticket: u64,
        result: &Result<ResultValue, String>,
    ) -> String {
        let kind = match result {
            Ok(value) => value.kind().to_owned(),
            Err(error) => format!("error \"{}\"", excerpt(error, ERROR_EXCERPT)),
        };
        describe(
            ticket,
            &kind,
            matches!(result, Ok(ResultValue::Bound(_))),
            Owners {
                openings: self.opening.len(),
                commands: self.commands.pending_count(),
                creations: self.creations.len(),
            },
            self.command_submissions.last(ticket),
        )
    }
}

struct Owners {
    openings: usize,
    commands: usize,
    creations: usize,
}

fn describe(
    ticket: u64,
    kind: &str,
    binding: bool,
    owners: Owners,
    submitted: Option<&str>,
) -> String {
    let owners = format!(
        "openings={} commands={} creations={}",
        owners.openings, owners.commands, owners.creations
    );
    if binding {
        return format!("engine binding for ticket {ticket} matches no opening engine ({owners})");
    }
    let submitted = submitted.unwrap_or(NO_SUBMISSION);
    format!(
        "journal completion for ticket {ticket} has no owner: result {kind}, last command submission {submitted} ({owners})"
    )
}

/// 문자 경계에서 자른 앞부분. 잘렸으면 끝에 줄임표를 붙인다.
pub(super) fn excerpt(text: &str, limit: usize) -> std::borrow::Cow<'_, str> {
    match text.char_indices().nth(limit) {
        Some((end, _)) => format!("{}…", &text[..end]).into(),
        None => text.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owners() -> Owners {
        Owners {
            openings: 0,
            commands: 2,
            creations: 1,
        }
    }

    #[test]
    fn a_binding_without_an_opening_stays_an_opening_error() {
        let message = describe(7, "Bound", true, owners(), None);
        assert!(message.contains("ticket 7"), "{message}");
        assert!(message.contains("no opening engine"), "{message}");
        assert!(!message.contains("has no owner"), "{message}");
    }

    #[test]
    fn the_last_command_submission_names_the_settled_request() {
        let mut log = CommandSubmissions::default();
        log.record(7, "Admit");
        log.record(8, "Resolve");
        log.record(7, "ReadCommand");
        let message = describe(7, "Command", false, owners(), log.last(7));
        assert!(
            message.contains("last command submission ReadCommand"),
            "{message}"
        );
        assert!(message.contains("commands=2 creations=1"), "{message}");
    }

    #[test]
    fn the_submission_log_keeps_only_its_bound() {
        let mut log = CommandSubmissions::default();
        for ticket in 0..(SUBMISSION_LOG as u64 + 1) {
            log.record(ticket, "Admit");
        }
        assert_eq!(log.recent.len(), SUBMISSION_LOG);
        assert_eq!(log.last(0), None);
        assert_eq!(log.last(SUBMISSION_LOG as u64), Some("Admit"));
    }

    #[test]
    fn excerpts_cut_on_character_boundaries() {
        assert_eq!(excerpt("가나다", 2), "가나…");
        assert_eq!(excerpt("가나", 2), "가나");
    }
}
