//! 매니페스트로 clap 명령 트리를 만든다. 요청 조립은 super::request가 담당한다.

use std::collections::HashSet;
use std::path::Path;

use clap::{Arg, ArgAction, Command};

use tasty_plugin_manifest::{CliArg, CliArgGroup, CliArgType, CliCommandDecl, Manifest};

use super::PluginCliEntry;

/// `~/.tasty/plugins/*` 스캔. 파싱 실패한 매니페스트는 stderr에 경고만 찍고 스킵.
pub fn discover_plugin_clis(plugins_root: &Path) -> Vec<PluginCliEntry> {
    let mut out = Vec::new();
    let Ok(read_dir) = std::fs::read_dir(plugins_root) else {
        return out;
    };
    for entry in read_dir.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        if !dir.join("tasty-plugin.toml").exists() {
            continue;
        }
        match Manifest::load(&dir) {
            Ok(manifest) => {
                let user_lang = tasty_utils::path::tasty_home().map(|home| home.join("lang"));
                let strings = tasty_i18n::plugin_catalog::load(
                    &dir.join(&manifest.lang_dir),
                    tasty_i18n::current_language(),
                    &manifest.id,
                    user_lang.as_deref(),
                );
                for mut cli in manifest.contributes.cli {
                    localize_decl(&mut cli, &strings);
                    out.push(PluginCliEntry { cli });
                }
            }
            Err(e) => {
                crate::out::errln!(
                    "{}",
                    tasty_i18n::t_fmt2(
                        "cli.plugin_cli.manifest_skipped",
                        &dir.display().to_string(),
                        &e.to_string()
                    )
                );
            }
        }
    }
    out
}

/// 호스트 정적 `Cli`에 plugin 서브커맨드를 추가한 `clap::Command`. `--help` 출력
/// 통합과 동적 파싱에 공통 사용.
pub fn build_augmented_cli(entries: &[PluginCliEntry]) -> Command {
    let mut cmd = crate::help_i18n::command();
    let host = host_command_names(&cmd);
    for entry in entries {
        if host.contains(&entry.cli.name) {
            crate::out::errln!(
                "{}",
                tasty_i18n::t_fmt("cli.plugin_cli.name_shadows_host_command", &entry.cli.name)
            );
            continue;
        }
        cmd = cmd.subcommand(build_cli_subcommand(&entry.cli));
    }
    cmd
}

/// 정적 명령과 alias를 clap 트리에서 읽는다.
pub(super) fn host_command_names(cmd: &Command) -> HashSet<String> {
    let mut names = HashSet::new();
    for sub in cmd.get_subcommands() {
        names.insert(sub.get_name().to_string());
        for alias in sub.get_all_aliases() {
            names.insert(alias.to_string());
        }
    }
    names
}

/// clap의 static 문자열 요구에 맞춰 프로세스 종료까지 보관한다.
/// 반복 호출하면 계속 누적되므로 CLI의 일회성 구성에만 사용한다.
fn leak_static(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn build_cli_subcommand(decl: &CliCommandDecl) -> Command {
    // arg_required_else_help: subcommand 누락 시 에러 메시지 대신 풀 도움말을 출력 —
    // 호스트의 derive 기반 CLI(tasty claude 등)와 동일한 UX.
    let mut top = Command::new(leak_static(&decl.name))
        .subcommand_required(true)
        .arg_required_else_help(true);
    if let Some(desc) = decl.description.as_deref().filter(|s| !s.is_empty()) {
        top = top.about(leak_static(desc));
    }
    for sub in &decl.subcommands {
        let mut sc = Command::new(leak_static(&sub.name));
        if let Some(desc) = sub.description.as_deref().filter(|s| !s.is_empty()) {
            sc = sc.about(leak_static(desc));
        }
        if let Some(group) = decl.arg_groups.get(&sub.args) {
            sc = apply_arg_group(sc, group);
        }
        top = top.subcommand(sc);
    }
    crate::help_i18n::localize_frame(top)
}

fn apply_arg_group(mut cmd: Command, group: &CliArgGroup) -> Command {
    for (idx, arg) in group.positional.iter().enumerate() {
        cmd = cmd.arg(build_arg(arg, Some(idx + 1)));
    }
    for arg in &group.flags {
        cmd = cmd.arg(build_arg(arg, None));
    }
    cmd
}

fn build_arg(arg: &CliArg, positional_index: Option<usize>) -> Arg {
    let mut a = Arg::new(leak_static(&arg.name));
    if let Some(i) = positional_index {
        a = a.index(i);
    } else if let Some(flag) = &arg.flag {
        a = a.long(leak_static(flag.trim_start_matches('-')));
    }
    a = a.required(arg.required);
    a = match arg.ty {
        CliArgType::Bool => a.action(ArgAction::SetTrue),
        // 반복 횟수를 잃지 않도록 Append로 받고 extract_value에서 중복을 거절한다.
        _ if arg.reject_repeat => a.action(ArgAction::Append),
        _ => a.action(ArgAction::Set),
    };
    if let Some(default) = &arg.default {
        let s = match default {
            toml::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        a = a.default_value(leak_static(&s));
    }
    if let Some(help) = arg.help.as_deref().filter(|s| !s.is_empty()) {
        a = a.help(leak_static(help));
    }
    a
}

/// Keep legacy manifest text when a key is absent; do not consult host namespaces.
fn localize_decl(decl: &mut CliCommandDecl, strings: &std::collections::HashMap<String, String>) {
    fn replace(
        value: &mut Option<String>,
        key: &Option<String>,
        strings: &std::collections::HashMap<String, String>,
    ) {
        if let Some(translated) = key
            .as_ref()
            .and_then(|key| strings.get(key))
            .filter(|v| !v.trim().is_empty())
        {
            *value = Some(translated.clone());
        }
    }
    replace(&mut decl.description, &decl.description_i18n_key, strings);
    for sub in &mut decl.subcommands {
        replace(&mut sub.description, &sub.description_i18n_key, strings);
    }
    for group in decl.arg_groups.values_mut() {
        for arg in group.positional.iter_mut().chain(&mut group.flags) {
            replace(&mut arg.help, &arg.help_i18n_key, strings);
        }
    }
}
