//! A bounded, nonrecursive entry list for fixtures whose complete contents matter.

use super::{Floor, Walked, normalized_rel};
use std::path::Path;

/// Collects every direct entry, including directories and symlinks, without following them.
/// Read failures are errors, not a partial successful list. At most `max_entries` paths are
/// retained; observing one more entry rejects the list instead of returning a truncated set.
#[must_use = "Handle enumeration, floor and entry-limit failures."]
pub fn entries_with_floor(
    root: &Path,
    rel_base: &Path,
    floor: &Floor,
    max_entries: usize,
) -> Result<Vec<Walked>, String> {
    floor.validate()?;
    if max_entries < floor.min {
        return Err(format!(
            "direct-entry limit {max_entries} is below floor {}",
            floor.min
        ));
    }
    let entries = std::fs::read_dir(root)
        .map_err(|error| format!("could not enumerate {}: {error}", root.display()))?;
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("could not read an entry in {}: {error}", root.display()))?;
        if out.len() == max_entries {
            return Err(format!(
                "direct-entry limit {max_entries} exceeded in {}",
                root.display()
            ));
        }
        let path = entry.path();
        out.push(Walked {
            rel: normalized_rel(&path, rel_base),
            path,
        });
    }
    if out.len() < floor.min {
        return Err(format!(
            "{} collected {} direct entries, below floor {} (measured {}). Do not lower the floor to pass; check the fixture and enumeration.",
            root.display(),
            out.len(),
            floor.min,
            floor.measured
        ));
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{floored_walk::CountedOn, temp_scratch::Scratch};
    use std::fs;

    fn floor(n: usize) -> Floor {
        Floor {
            min: n,
            measured: n,
            measured_on: "2026-10-24",
            counted_on: CountedOn::SyntheticTree,
            why_this_gap: "This owned fixture has exactly the declared direct entries; no missing entry is acceptable and nested contents are not its population.",
        }
    }

    #[test]
    fn files_and_directories_are_counted_once_without_descending() {
        let root = Scratch::new("direct-entries-shallow");
        fs::write(root.path().join("file"), b"fixture").unwrap();
        fs::create_dir(root.path().join("directory")).unwrap();
        fs::write(root.path().join("directory/nested"), b"nested").unwrap();
        let entries = entries_with_floor(root.path(), root.path(), &floor(2), 2).unwrap();
        assert_eq!(
            entries.iter().map(|e| e.rel.as_str()).collect::<Vec<_>>(),
            ["directory", "file"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn dangling_and_directory_symlinks_are_entries_without_following_targets() {
        let root = Scratch::new("direct-entries-links");
        let outside = Scratch::new("direct-entries-link-target");
        fs::write(outside.path().join("must-not-be-collected"), b"outside").unwrap();
        fs::write(root.path().join("file"), b"fixture").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("directory-link")).unwrap();
        std::os::unix::fs::symlink("absent", root.path().join("dangling-link")).unwrap();
        let entries = entries_with_floor(root.path(), root.path(), &floor(3), 3).unwrap();
        assert_eq!(
            entries.iter().map(|e| e.rel.as_str()).collect::<Vec<_>>(),
            ["dangling-link", "directory-link", "file"]
        );
    }

    #[test]
    fn a_third_entry_is_rejected_instead_of_returning_a_truncated_pair() {
        let root = Scratch::new("direct-entries-excess");
        for name in ["a", "b", "c"] {
            fs::write(root.path().join(name), b"fixture").unwrap();
        }
        let error = entries_with_floor(root.path(), root.path(), &floor(2), 2).unwrap_err();
        assert!(error.contains("direct-entry limit 2 exceeded"), "{error}");
    }

    #[test]
    fn an_empty_fixture_is_not_a_successful_empty_population() {
        let root = Scratch::new("direct-entries-empty");
        let error = entries_with_floor(root.path(), root.path(), &floor(2), 2).unwrap_err();
        assert!(error.contains("0 direct entries, below floor 2"), "{error}");
    }

    #[test]
    fn a_failed_directory_read_does_not_become_an_empty_population() {
        let root = Scratch::new("direct-entries-read-error");
        let file = root.path().join("not-a-directory");
        fs::write(&file, b"fixture").unwrap();
        let error = entries_with_floor(&file, root.path(), &floor(2), 2).unwrap_err();
        assert!(error.contains("could not enumerate"), "{error}");
    }

    #[test]
    fn a_limit_below_the_floor_is_refused_before_reading() {
        let root = Scratch::new("direct-entries-invalid-limit");
        let absent = root.path().join("absent");
        let error = entries_with_floor(&absent, root.path(), &floor(2), 1).unwrap_err();
        assert_eq!(error, "direct-entry limit 1 is below floor 2");
    }
}
