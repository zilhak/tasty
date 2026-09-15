//! Local presentation of clap help. Argument syntax remains clap's responsibility.
use clap::{ArgAction, Command};
use tasty_i18n::t;

pub(crate) fn localize(mut cmd: Command) -> Command {
    if tasty_i18n::current_language() == "en" {
        return cmd;
    }
    // Build first so clap's generated help/version arguments can be translated too.
    cmd.build();
    cmd = cmd.mut_args(|arg| match *arg.get_action() {
        ArgAction::Help | ArgAction::HelpShort | ArgAction::HelpLong => arg
            .help(t("cli.help_frame.help").to_owned())
            .long_help(None::<&str>),
        ArgAction::Version => arg.help(t("cli.help_frame.version").to_owned()),
        _ => localize_annotations(arg),
    });
    let mut template = format!(
        "{{before-help}}{{about-with-newline}}\n{} {{usage}}\n",
        t("cli.help_frame.usage")
    );
    if cmd.get_subcommands().any(|s| !s.is_hide_set()) {
        template.push_str(&format!(
            "\n{}\n{{subcommands}}",
            t("cli.help_frame.commands")
        ));
    }
    if cmd.get_positionals().any(|a| !a.is_hide_set()) {
        template.push_str(&format!(
            "\n{}\n{{positionals}}",
            t("cli.help_frame.arguments")
        ));
    }
    if cmd
        .get_arguments()
        .any(|a| !a.is_positional() && !a.is_hide_set())
    {
        template.push_str(&format!("\n{}\n{{options}}", t("cli.help_frame.options")));
    }
    template.push_str("{after-help}");
    cmd = cmd.help_template(template);
    let names: Vec<_> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_owned())
        .collect();
    for name in names {
        cmd = cmd.mut_subcommand(&name, |sub| {
            if name == "help" {
                sub.about(t("cli.help_frame.help_command").to_owned())
            } else {
                localize(sub)
            }
        });
    }
    cmd
}

fn localize_annotations(mut arg: clap::Arg) -> clap::Arg {
    if !arg.get_action().takes_values() {
        return arg;
    }
    let mut notes = Vec::new();
    if !arg.is_hide_default_value_set() && !arg.get_default_values().is_empty() {
        let values = arg
            .get_default_values()
            .iter()
            .map(|v| v.to_string_lossy())
            .collect::<Vec<_>>()
            .join(", ");
        notes.push(tasty_i18n::t_fmt("cli.help_frame.default", &values));
        arg = arg.hide_default_value(true);
    }
    if !arg.is_hide_possible_values_set() {
        let values = arg
            .get_possible_values()
            .into_iter()
            .filter(|v| !v.is_hide_set())
            .map(|v| v.get_name().to_owned())
            .collect::<Vec<_>>();
        if !values.is_empty() {
            notes.push(tasty_i18n::t_fmt(
                "cli.parse.valid_values",
                &values.join(", "),
            ));
            arg = arg.hide_possible_values(true);
        }
    }
    if !notes.is_empty() {
        let notes = notes.join("; ");
        if let Some(help) = arg.get_help() {
            let help = format!("{help} [{notes}]");
            arg = arg.help(help);
        }
        if let Some(help) = arg.get_long_help() {
            let help = format!("{help} [{notes}]");
            arg = arg.long_help(help);
        }
    }
    arg
}
