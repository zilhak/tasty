//! Translate clap's structured parse diagnostics without changing wire errors.
use clap::error::{ContextKind, ErrorKind};
use tasty_i18n::{t, t_fmt};

pub(crate) fn render(err: &clap::Error) -> String {
    if tasty_i18n::current_language() == "en" {
        return err.to_string();
    }
    let key = match err.kind() {
        ErrorKind::InvalidValue => "invalid_value",
        ErrorKind::UnknownArgument => "unknown_argument",
        ErrorKind::InvalidSubcommand => "invalid_subcommand",
        ErrorKind::NoEquals => "no_equals",
        ErrorKind::ValueValidation => "value_validation",
        ErrorKind::TooManyValues => "too_many_values",
        ErrorKind::TooFewValues => "too_few_values",
        ErrorKind::WrongNumberOfValues => "wrong_number",
        ErrorKind::ArgumentConflict => "conflict",
        ErrorKind::MissingRequiredArgument => "missing_argument",
        ErrorKind::MissingSubcommand => "missing_subcommand",
        ErrorKind::InvalidUtf8 => "invalid_utf8",
        ErrorKind::Io => "io",
        ErrorKind::Format => "format",
        _ => "parse",
    };
    let mut out = format!(
        "{} {}\n",
        t("cli.help_frame.error"),
        t(&format!("cli.parse.{key}"))
    );
    for (kind, value) in err.context() {
        let key = match kind {
            ContextKind::InvalidSubcommand | ContextKind::InvalidArg => "argument",
            ContextKind::PriorArg => "prior",
            ContextKind::InvalidValue => "value",
            ContextKind::ValidValue => "valid_values",
            ContextKind::ValidSubcommand => "valid_commands",
            ContextKind::ActualNumValues => "actual",
            ContextKind::ExpectedNumValues => "expected",
            ContextKind::MinValues => "minimum",
            ContextKind::SuggestedCommand
            | ContextKind::SuggestedSubcommand
            | ContextKind::SuggestedArg
            | ContextKind::SuggestedValue => "suggestion",
            // Usage and opaque clap prose are not argument values. Contextual help
            // below supplies syntax from the command tree instead of translating text.
            _ => continue,
        };
        out.push_str(&t_fmt(&format!("cli.parse.{key}"), &value.to_string()));
        out.push('\n');
    }
    if let Some(source) = std::error::Error::source(err) {
        out.push_str(&t_fmt("cli.parse.cause", &source.to_string()));
        out.push('\n');
    }
    out
}
