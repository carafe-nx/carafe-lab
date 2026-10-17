//! The Switch firmware Lab installs into Eden's NAND: a folder of `.nca` files as dumped from a console.

use std::fs;
use std::path::Path;

/// A firmware file: its name and size in bytes.
pub type FirmwareFile = (String, u64);

/// Returns the `.nca` files of the folder `dir`, sorted by name.
///
/// # Errors
///
/// A message when the folder cannot be read or has no `.nca` files.
pub fn read_folder(dir: &Path) -> Result<Vec<FirmwareFile>, String> {
    let entries = fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let files = nca_files(entries.filter_map(Result::ok).filter_map(|entry| {
        let metadata = entry.metadata().ok()?;
        metadata.is_file().then(|| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                metadata.len(),
            )
        })
    }));
    if files.is_empty() {
        return Err(format!(
            "{}: no .nca files, so not a firmware folder",
            dir.display()
        ));
    }
    Ok(files)
}

/// Returns the `.nca` files among `files`, sorted by name; the extension is matched without regard to case.
#[must_use]
pub fn nca_files(files: impl IntoIterator<Item = FirmwareFile>) -> Vec<FirmwareFile> {
    let mut found: Vec<FirmwareFile> = files
        .into_iter()
        .filter(|(name, _)| {
            Path::new(name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("nca"))
        })
        .collect();
    found.sort();
    found
}

/// Returns a short mark of `files` that changes when any name or size changes, to tell whether the firmware in
/// Eden's NAND is still the chosen one.
#[must_use]
pub fn stamp(files: &[FirmwareFile]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (name, size) in files {
        for byte in name.bytes().chain(size.to_le_bytes()).chain([0]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{}-{hash:016x}", files.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_nca_files_count_and_come_sorted() {
        let files = nca_files([
            ("b.cnmt.nca".to_owned(), 2),
            ("readme.txt".to_owned(), 9),
            ("A.NCA".to_owned(), 1),
        ]);
        assert_eq!(
            files,
            [("A.NCA".to_owned(), 1), ("b.cnmt.nca".to_owned(), 2)]
        );
    }

    #[test]
    fn the_stamp_follows_names_and_sizes() {
        let one = vec![("a.nca".to_owned(), 1)];
        let bigger = vec![("a.nca".to_owned(), 2)];
        assert_eq!(stamp(&one), stamp(&one.clone()));
        assert_ne!(stamp(&one), stamp(&bigger));
        assert!(stamp(&one).starts_with("1-"));
    }
}
