//! Plugin identifiers shared by manifest validation and user catalog paths.

/// The manifest's plugin ID grammar. Path consumers must additionally require a
/// normal path component: the historical grammar also accepts `.` and `..`.
pub fn is_valid_plugin_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        && s.contains('.')
}
