//! `tasty tool passkey ...` 실행 — Passkey(자격증명) CRUD (로컬 파일, IPC 미경유).
//!
//! `~/.tasty/passkeys.toml`(0600) 를 직접 읽고 쓴다. 프로필이 이름으로 참조한다.
//! **값 비노출 정책(ADR-0016)**: list/show 는 name + kind 만 출력하고 경로/내용을
//! 절대 보이지 않는다(실제 값은 GUI Reveal 로만 확인). add 는 쓰기라 허용 — inline 은
//! `~/.tasty/passkeys/<name>` 0600 파일로 materialize 된다.
//!
//! 선언(`PasskeyCommands`)은 [`crate::commands::passkey`] 에 남는다.

use std::io::Read;

use anyhow::Result;
use tasty_i18n::{t, t_fmt};
use tasty_remote_profiles::Passkeys;

use crate::commands::passkey::PasskeyCommands;
use crate::out::outln;

/// `tasty tool passkey ...` 로컬 분기 진입점(IPC 미경유).
pub fn run(command: &PasskeyCommands) -> Result<()> {
    match command {
        PasskeyCommands::Add {
            name,
            path,
            inline,
            value,
        } => {
            if path.is_some() && *inline {
                anyhow::bail!("{}", t("cli.passkey.path_xor_inline"));
            }
            let mut passkeys = Passkeys::load();
            let replaced = passkeys.get(name).is_some();
            if let Some(p) = path {
                passkeys.upsert_path(name, p.clone())?;
            } else if *inline {
                let secret = match value {
                    Some(v) => v.clone(),
                    None => {
                        let mut buf = String::new();
                        std::io::stdin().read_to_string(&mut buf)?;
                        // 끝 개행 1개는 제거(여러 줄 키 본문은 보존).
                        if buf.ends_with('\n') {
                            buf.pop();
                            if buf.ends_with('\r') {
                                buf.pop();
                            }
                        }
                        buf
                    }
                };
                if secret.is_empty() {
                    anyhow::bail!("{}", t("cli.passkey.inline_empty"));
                }
                passkeys.upsert_inline(name, &secret)?;
            } else {
                anyhow::bail!("{}", t("cli.passkey.needs_path_or_inline"));
            }
            passkeys.save()?;
            let key = if replaced {
                "cli.passkey.updated"
            } else {
                "cli.passkey.added"
            };
            outln!("{}", t_fmt(key, name))?;
            Ok(())
        }
        PasskeyCommands::List { json } => {
            let passkeys = Passkeys::load();
            if *json {
                // 값 비노출 — name + kind 만.
                let arr: Vec<_> = passkeys
                    .passkeys
                    .iter()
                    .map(|k| serde_json::json!({ "name": k.name, "kind": k.kind }))
                    .collect();
                outln!("{}", serde_json::to_string_pretty(&arr)?)?;
            } else if passkeys.passkeys.is_empty() {
                outln!("{}", t("cli.passkey.list_empty"))?;
            } else {
                // 헤더는 컬럼 패딩까지 값에 담는다 — CJK 는 터미널 표시 폭이 2배라
                // 코드에서 문자 수로 패딩하면 ko/ja 헤더가 데이터 행과 어긋난다
                // (`remote_profile.rs` 의 표 헤더와 같은 처리).
                outln!("{}", t("cli.passkey.list_header"))?;
                for k in &passkeys.passkeys {
                    outln!("{:<24} {}", k.name, k.kind)?;
                }
            }
            Ok(())
        }
        PasskeyCommands::Show { name, json } => {
            let passkeys = Passkeys::load();
            let Some(k) = passkeys.get(name) else {
                anyhow::bail!("{}", t_fmt("cli.passkey.not_found", name));
            };
            // 값(경로/내용)은 노출하지 않는다 — name + kind 만.
            if *json {
                outln!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({ "name": k.name, "kind": k.kind })
                    )?
                )?;
            } else {
                outln!("name : {}", k.name)?;
                outln!("kind : {}", k.kind)?;
                // `name` / `kind` 라벨은 그대로 둔다 — 바로 위 `--json` 분기가
                // 내보내는 실제 키라 같은 이름이라야 두 출력이 대응된다. 번역
                // 대상은 그 옆의 자연어(마스킹 안내)뿐이다.
                outln!("value: {}", t("cli.passkey.value_masked"))?;
            }
            Ok(())
        }
        PasskeyCommands::Remove { name } => {
            let mut passkeys = Passkeys::load();
            if passkeys.remove(name) {
                passkeys.save()?;
                outln!("{}", t_fmt("cli.passkey.removed", name))?;
            } else {
                anyhow::bail!("{}", t_fmt("cli.passkey.not_found", name));
            }
            Ok(())
        }
    }
}
