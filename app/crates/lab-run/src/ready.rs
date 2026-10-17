//! Ready NSPs: what is inside one, a place for it the emulator can boot it from, and a request to build it again
//! with the current runtime.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use carafe_core::BuildRecord;
use carafe_core::package::RECORD_FILE;
use carafe_core::pfs0::{self, PREFIX_SIZE};
use carafe_core::wizard::BuildRequest;

/// A `carafe.json` larger than this is not read.
const MAX_RECORD_BYTES: u64 = 1 << 20;

/// The folder inside the NSP folder where ready NSPs are linked under their Title IDs.
pub const READY_DIR: &str = "ready";

/// A ready NSP and the build record Carafe put next to its NCAs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyNsp {
    /// The NSP file.
    pub path: PathBuf,
    /// What it was built from, with which runtime and settings.
    pub record: BuildRecord,
    /// The file's size in bytes.
    pub size_bytes: u64,
}

/// Reads the build record of the NSP at `path`.
///
/// # Errors
///
/// A message when the file cannot be read, is not an NSP or was not built by Carafe.
pub fn read(path: &Path) -> Result<ReadyNsp, String> {
    let fail = |text: &str| format!("{}: {text}", path.display());
    let mut file = File::open(path).map_err(|error| fail(&error.to_string()))?;
    let size_bytes = file
        .metadata()
        .map_err(|error| fail(&error.to_string()))?
        .len();
    let prefix =
        read_at(&mut file, 0, PREFIX_SIZE as u64).map_err(|error| fail(&error.to_string()))?;
    let header_size = pfs0::header_size(&prefix).map_err(|error| fail(&error.to_string()))?;
    let header =
        read_at(&mut file, 0, header_size as u64).map_err(|error| fail(&error.to_string()))?;
    let files = pfs0::files(&header).map_err(|error| fail(&error.to_string()))?;
    let entry = files
        .iter()
        .find(|entry| entry.name == RECORD_FILE)
        .ok_or_else(|| fail("not built by Carafe: it has no carafe.json"))?;
    if entry.size > MAX_RECORD_BYTES || entry.offset.saturating_add(entry.size) > size_bytes {
        return Err(fail("its carafe.json is damaged"));
    }
    let json =
        read_at(&mut file, entry.offset, entry.size).map_err(|error| fail(&error.to_string()))?;
    let record: BuildRecord =
        serde_json::from_slice(&json).map_err(|error| fail(&error.to_string()))?;
    record
        .check_format()
        .map_err(|error| fail(&error.to_string()))?;
    Ok(ReadyNsp {
        path: path.to_path_buf(),
        record,
        size_bytes,
    })
}

/// Puts `nsp` into `nsp_dir/ready` under its Title ID, as the emulator names games by the file: a hard link when
/// the drives match, a copy otherwise. A file of the same size already there is kept.
///
/// # Returns
///
/// The path the emulator boots.
///
/// # Errors
///
/// The folder cannot be made or the NSP cannot be linked or copied.
pub fn stage(nsp: &ReadyNsp, nsp_dir: &Path) -> io::Result<PathBuf> {
    let dir = nsp_dir.join(READY_DIR);
    fs::create_dir_all(&dir)?;
    let staged = dir.join(format!("{}.nsp", nsp.record.title_id));
    if fs::metadata(&staged).is_ok_and(|kept| kept.len() == nsp.size_bytes)
        && same_file(&staged, &nsp.path)
    {
        return Ok(staged);
    }
    let _ = fs::remove_file(&staged);
    if fs::hard_link(&nsp.path, &staged).is_err() {
        let partial = staged.with_extension("part");
        fs::copy(&nsp.path, &partial)?;
        fs::rename(&partial, &staged)?;
    }
    Ok(staged)
}

/// Returns a request to build the game of `nsp` again from its folder with its own settings.
///
/// # Errors
///
/// A message when the game folder is gone.
pub fn rebuild_request(nsp: &ReadyNsp) -> Result<BuildRequest, String> {
    let record = &nsp.record;
    if !Path::new(&record.source.folder).is_dir() {
        return Err(format!(
            "the game folder {} is gone, so the NSP cannot be built again",
            record.source.folder
        ));
    }
    Ok(BuildRequest {
        title_id: Some(record.title_id),
        source: record.source.clone(),
        metadata: record.metadata.clone(),
        settings: record.settings.clone(),
        icon: None,
    })
}

fn same_file(staged: &Path, original: &Path) -> bool {
    let modified = |path: &Path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    };
    modified(staged).is_some() && modified(staged) == modified(original)
}

fn read_at(file: &mut File, offset: u64, len: u64) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; usize::try_from(len).map_err(io::Error::other)?];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use carafe_core::pfs0::Pfs0Entry;
    use carafe_core::wizard;

    use super::*;

    fn record(folder: &Path) -> BuildRecord {
        let request = BuildRequest {
            title_id: None,
            source: carafe_core::record::GameSource {
                folder: folder.to_string_lossy().into_owned(),
                executable: "game.exe".to_owned(),
                arch: carafe_core::record::Arch::X86,
                arch_override: None,
                arguments: Vec::new(),
            },
            metadata: carafe_core::metadata::Metadata::new("Game".to_owned()),
            settings: carafe_core::settings::AutorunSettings::default(),
            icon: None,
        };
        wizard::plan(request, None, 7, "0.9.0").unwrap()
    }

    fn nsp(path: &Path, files: &[(&str, &[u8])]) {
        let entries: Vec<Pfs0Entry> = files
            .iter()
            .map(|(name, data)| Pfs0Entry {
                name: (*name).to_owned(),
                size: data.len() as u64,
            })
            .collect();
        let mut bytes = pfs0::header(&entries).unwrap();
        for (_, data) in files {
            bytes.extend_from_slice(data);
        }
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn a_ready_nsp_tells_its_runtime_and_settings_and_is_staged_under_its_title_id() {
        let dir = std::env::temp_dir().join(format!("lab-ready-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let built = record(&dir);
        let json = serde_json::to_vec(&built).unwrap();
        let path = dir.join("Game [build 1].nsp");
        nsp(&path, &[("0123.nca", b"nca"), (RECORD_FILE, &json)]);
        let ready = read(&path).unwrap();
        assert_eq!(ready.record, built);
        assert_eq!(ready.record.runtime_version, "0.9.0");
        let staged = stage(&ready, &dir.join("nsp")).unwrap();
        assert_eq!(
            staged.file_name().unwrap().to_string_lossy(),
            format!("{}.nsp", built.title_id)
        );
        assert_eq!(fs::read(&staged).unwrap(), fs::read(&path).unwrap());
        let request = rebuild_request(&ready).unwrap();
        assert_eq!(request.title_id, Some(built.title_id));
        let plain = dir.join("plain.nsp");
        nsp(&plain, &[("0123.nca", b"nca")]);
        assert!(read(&plain).unwrap_err().contains("not built by Carafe"));
        let _ = fs::remove_dir_all(&dir);
    }
}
