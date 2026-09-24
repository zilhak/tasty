//! passkey 로컬 파일을 관리한다. list/show는 이름과 종류만 출력하고 값은 GUI Reveal로만 확인한다.
//! inline 값은 `passkeys/<name>` 파일에 0600 권한으로 저장한다.
//! 값 노출 정책은 docs/design/systems/memory.md#passkey-저장과-열람을 따른다.

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
                let arr: Vec<_> = passkeys
                    .passkeys
                    .iter()
                    .map(|k| serde_json::json!({ "name": k.name, "kind": k.kind }))
                    .collect();
                outln!("{}", serde_json::to_string_pretty(&arr)?)?;
            } else if passkeys.passkeys.is_empty() {
                outln!("{}", t("cli.passkey.list_empty"))?;
            } else {
                // CJK 표시 폭에 맞춘 패딩까지 번역된 헤더에 포함한다.
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
                // name/kind는 JSON 키와 같은 이름을 유지한다. 값 숨김 안내만 번역한다.
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
