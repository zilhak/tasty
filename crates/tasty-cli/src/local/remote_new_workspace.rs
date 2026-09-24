//! remote_create의 공용 생성 경로를 호출하고 새 workspace ID를 출력한다.
//! 출력한 ID는 tasty remote attach --workspace에 쓸 수 있다. 로컬 사용자 상태는 바꾸지 않는다.

use anyhow::{Context, Result};
use tasty_i18n::{t, t_args};

use crate::out::outln;
use crate::remote_create;
use crate::ssh::SshTarget;

/// 호출자가 profile/ssh 중 하나로 접속 설정을 정한 뒤 호출한다.
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
        // 자동화가 읽는 JSON 키는 번역하지 않는다.
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
