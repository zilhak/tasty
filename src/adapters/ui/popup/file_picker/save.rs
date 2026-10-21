//! Main popup and Settings chooser share save-name validation and overwrite detection.
//! Only directory receipts are inspected; choosing a path never writes a file.
use super::matches_filters;
use crate::core::fs_list::DirEntryInfo;
use std::path::Path;

pub(crate) fn confirmed_path(
    dir: &Path,
    name: &str,
    ready: bool,
    entries: &[DirEntryInfo],
) -> Option<String> {
    let name = name.trim();
    let mut components = Path::new(name).components();
    let plain = matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(_)), None)
    ) && !name.contains(['/', '\\']);
    (ready && plain && !entries.iter().any(|e| e.is_dir && e.name == name))
        .then(|| dir.join(name).to_string_lossy().into_owned())
}

pub(crate) fn overwrites(name: &str, entries: &[DirEntryInfo], filters: &[String]) -> bool {
    entries
        .iter()
        .any(|e| !e.is_dir && e.name == name.trim() && matches_filters(filters, &e.name))
}
