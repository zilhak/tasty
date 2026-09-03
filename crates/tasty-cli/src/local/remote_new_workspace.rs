//! `tasty remote new-workspace` — SSH 너머(또는 loopback 직결) 원격 tasty 인스턴스에
//! 워크스페이스를 새로 만든다. 출력된 id 를 `tasty remote attach --workspace <id>` 에
//! 넘기면 "원격에 만들고 그 자리에서 attach" 가 CLI 만으로 완성된다.
//!
//! 실제 생성 능력(엔드포인트 해석 → `workspace.create`)은 [`crate::remote_create`] 가
//! 담당한다 — 로컬 IPC `remote.attach` 의 `new_workspace` 옵션과 **동일한 함수를
//! 공유**한다(원칙 2: CLI/IPC 양면, GUI 전용 금지). 이 모듈은 그 위에 출력만 얹는
//! 얇은 래퍼다([`super::remote_workspaces`] 와 동형).
//!
//! 로컬 사용자 상태(focus/닫은항목 히스토리/선택)에 닿지 않는다 — 원격으로 나가는
//! client 로직이다(원칙 1).

use anyhow::{Context, Result};
use tasty_i18n::{t, t_args};

use crate::out::outln;
use crate::remote_create;
use crate::ssh::SshTarget;

/// `tasty remote new-workspace --ssh user@host` / `--profile <name>` 진입점.
///
/// 접속 스펙(target/remote_tasty/port_mode/port_file)은 호출자(run.rs 선처리)가
/// profile/ssh 상호배타 가드를 거쳐 resolve 해 넘긴다. 여기서는 생성 후 출력만 한다.
pub fn run_remote_new_workspace(
    target: SshTarget,
    remote_tasty: &str,
    port_mode: &str,
    port_file: Option<&str>,
    name: Option<&str>,
    cwd: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let dest = target.destination.clone();
    let created = remote_create::create(&target, remote_tasty, port_mode, port_file, name, cwd)
        .with_context(|| t_args("cli.remote_new_workspace.create_failed", &[dest.as_str()]))?;

    if json_output {
        // 구조화 데이터라 i18n 대상 아님 — 에이전트가 id 를 파싱해 attach 로 넘긴다.
        let json = serde_json::to_string_pretty(&created)
            .context(t("cli.remote_new_workspace.serialize_failed"))?;
        outln!("{json}")?;
        return Ok(());
    }

    outln!(
        "{}",
        t_args(
            "cli.remote_new_workspace.created",
            &[
                &created.id.to_string(),
                &created.name,
                dest.as_str(),
                &created.index.to_string(),
            ],
        )
    )?;
    Ok(())
}
