//! `tasty tool passkey ...` — Passkey(자격증명) CRUD (로컬 파일, IPC 미경유).
//!
//! `~/.tasty/passkeys.toml`(0600) 를 직접 읽고 쓴다. 프로필이 이름으로 참조한다.
//! **값 비노출 정책(ADR-0016)**: list/show 는 name + kind 만 출력하고 경로/내용을
//! 절대 보이지 않는다(실제 값은 GUI Reveal 로만 확인). add 는 쓰기라 허용 — inline 은
//! `~/.tasty/passkeys/<name>` 0600 파일로 materialize 된다.

use std::io::Read;

use anyhow::Result;
use clap::Subcommand;

use tasty_remote_profiles::Passkeys;

#[derive(Subcommand)]
pub enum PasskeyCommands {
    /// Passkey 추가/교체. `--path <file>`(파일 참조) 또는 `--inline`(비밀 입력) 중 하나.
    Add {
        /// 고유 식별자(프로필이 참조). 영숫자/-/_ 만 허용.
        #[arg(long)]
        name: String,
        /// path kind — 사용자 소유 기존 키 파일 경로(`-i`). `--inline` 과 배타.
        #[arg(long)]
        path: Option<String>,
        /// inline kind — 비밀을 0600 파일로 materialize. 값은 `--value` 또는 stdin.
        #[arg(long)]
        inline: bool,
        /// inline 값(미지정 시 stdin 에서 읽음). `--inline` 과 함께 쓴다.
        #[arg(long)]
        value: Option<String>,
    },
    /// 저장된 Passkey 목록(name + kind 만 — 값 비노출).
    List {
        #[arg(long)]
        json: bool,
    },
    /// 한 Passkey 의 name + kind 출력(값 비노출).
    Show {
        #[arg(long)]
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Passkey 제거(inline 이면 관리 파일도 삭제).
    Remove {
        #[arg(long)]
        name: String,
    },
}

/// `tasty tool passkey ...` 로컬 분기 진입점(IPC 미경유).
pub fn run(command: &PasskeyCommands) -> Result<()> {
    match command {
        PasskeyCommands::Add { name, path, inline, value } => {
            if path.is_some() && *inline {
                anyhow::bail!("--path 와 --inline 은 함께 쓸 수 없습니다.");
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
                    anyhow::bail!("inline 값이 비어 있습니다 (--value 또는 stdin).");
                }
                passkeys.upsert_inline(name, &secret)?;
            } else {
                anyhow::bail!("--path <file> 또는 --inline 중 하나가 필요합니다.");
            }
            passkeys.save()?;
            println!("{} passkey '{name}'.", if replaced { "갱신:" } else { "추가:" });
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
                println!("{}", serde_json::to_string_pretty(&arr)?);
            } else if passkeys.passkeys.is_empty() {
                println!("저장된 passkey 가 없습니다 (tasty tool passkey add ...).");
            } else {
                println!("{:<24} KIND", "NAME");
                for k in &passkeys.passkeys {
                    println!("{:<24} {}", k.name, k.kind);
                }
            }
            Ok(())
        }
        PasskeyCommands::Show { name, json } => {
            let passkeys = Passkeys::load();
            let Some(k) = passkeys.get(name) else {
                anyhow::bail!("passkey '{name}' 을 찾을 수 없습니다.");
            };
            // 값(경로/내용)은 노출하지 않는다 — name + kind 만.
            if *json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "name": k.name, "kind": k.kind }))?);
            } else {
                println!("name : {}", k.name);
                println!("kind : {}", k.kind);
                println!("value: (마스킹 — GUI Reveal 로만 확인)");
            }
            Ok(())
        }
        PasskeyCommands::Remove { name } => {
            let mut passkeys = Passkeys::load();
            if passkeys.remove(name) {
                passkeys.save()?;
                println!("제거: passkey '{name}'.");
            } else {
                anyhow::bail!("passkey '{name}' 을 찾을 수 없습니다.");
            }
            Ok(())
        }
    }
}
