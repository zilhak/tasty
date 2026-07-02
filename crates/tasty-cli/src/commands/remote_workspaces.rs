//! `tasty remote workspaces` — SSH 너머(또는 loopback 직결) 원격 tasty 인스턴스의
//! 워크스페이스 목록 조회(browse). RA02 원격 추가 팝업 우측 목록의 데이터 소스이자,
//! 에이전트가 원격 attach 대상 id 를 발견하는 경로다.
//!
//! 실제 조회 능력(접속 스펙 resolve → 터널/loopback → `workspace.list` + `attach.list`
//! 병합)은 [`crate::remote_browse`] 가 담당한다 — 로컬 IPC method `remote.workspaces`
//! 와 **동일한 함수를 공유**한다(원칙 2: CLI/IPC 양면, GUI 전용 금지). 이 모듈은 그 위에
//! 접속 스펙 resolve 위임 + 텍스트/JSON 출력만 얹는 얇은 래퍼다.
//!
//! 순수 조회 — 로컬 사용자 상태(focus/닫은항목 히스토리/선택·스크롤·커서)에 닿지 않는다
//! (원칙 1).

use anyhow::{Context, Result};
use tasty_i18n::{t, t_args};

use crate::remote_browse;
use crate::ssh::SshTarget;

/// `tasty remote workspaces --ssh user@host` / `--profile <name>` 진입점.
///
/// 접속 스펙(target/remote_tasty/port_mode/port_file)은 호출자(run.rs 선처리)가
/// profile/ssh 상호배타 가드를 거쳐 resolve 해 넘긴다. 여기서는 browse 후 출력만 한다.
/// 어느 단계든 실패하면 조용한 hang 없이 명확한 에러로 종료한다(팝업의 "연결 실패" 표시용).
pub fn run_remote_workspaces(
    target: SshTarget,
    remote_tasty: &str,
    port_mode: &str,
    port_file: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let dest = target.destination.clone();
    let list = remote_browse::browse(&target, remote_tasty, port_mode, port_file)
        .with_context(|| t_args("cli.remote_workspaces.browse_failed", &[dest.as_str()]))?;

    if json_output {
        // 구조화 데이터라 i18n 대상 아님 — 02 팝업/에이전트가 파싱할 원본.
        let json = serde_json::to_string_pretty(&list)
            .context(t("cli.remote_workspaces.serialize_failed"))?;
        println!("{json}");
        return Ok(());
    }

    if list.is_empty() {
        println!("{}", t_args("cli.remote_workspaces.none", &[dest.as_str()]));
        return Ok(());
    }

    println!(
        "{}",
        t_args(
            "cli.remote_workspaces.header",
            &[dest.as_str(), &list.len().to_string()],
        )
    );
    for ws in &list {
        let id_str = ws.id.to_string();
        let pane_str = ws.pane_count.to_string();
        let busy_str = ws.busy_count.to_string();
        if let Some(holder) = ws.holder {
            println!(
                "{}",
                t_args(
                    "cli.remote_workspaces.row_attached",
                    &[&id_str, &ws.name, &pane_str, &busy_str, &holder.to_string()],
                )
            );
        } else {
            println!(
                "{}",
                t_args(
                    "cli.remote_workspaces.row_free",
                    &[&id_str, &ws.name, &pane_str, &busy_str],
                )
            );
        }
    }
    Ok(())
}
