use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

const MARKER: &str = ".lab-log-capture";

pub(crate) struct Capture {
    current: PathBuf,
    limit: u64,
    used: u64,
    files: Vec<(SystemTime, PathBuf, u64)>,
}

impl Capture {
    pub(crate) fn open(root: &Path, destination: &Path, limit: u64) -> io::Result<Self> {
        fs::create_dir_all(destination)?;
        let root = root.canonicalize()?;
        let current = destination.canonicalize()?;
        if current
            .strip_prefix(&root)
            .ok()
            .is_none_or(|relative| relative.components().count() != 3)
        {
            return Err(io::Error::other("capture is outside the runs layout"));
        }
        let manifest = current.join(MARKER);
        match File::options()
            .append(true)
            .create_new(true)
            .open(&manifest)
        {
            Ok(_) => {}
            Err(error)
                if error.kind() == io::ErrorKind::AlreadyExists
                    && fs::symlink_metadata(&manifest).is_ok_and(|metadata| metadata.is_file()) => {
            }
            Err(error) => return Err(error),
        }
        let mut files = Vec::new();
        for run in child_dirs(&root)? {
            for game in child_dirs(&run)? {
                for launch in child_dirs(&game)? {
                    collect(&launch, &mut files)?;
                }
            }
        }
        files.sort_unstable_by(|a, b| b.cmp(a));
        let used = files.iter().map(|file| file.2).sum();
        let mut capture = Self {
            current,
            limit,
            used,
            files,
        };
        capture.evict(0)?;
        Ok(capture)
    }

    pub(crate) fn copy(
        &mut self,
        source: &Path,
        destination: &Path,
        offset: u64,
    ) -> io::Result<()> {
        let mut source = File::open(source)?;
        let end = source.metadata()?.len();
        let start = offset.min(end);
        let (mut output, bytes) = self.output(destination, end - start)?;
        source.seek(SeekFrom::Start(end - bytes))?;
        io::copy(&mut source.take(bytes), &mut output)?;
        Ok(())
    }

    pub(crate) fn write(&mut self, destination: &Path, data: &[u8]) -> io::Result<()> {
        let (mut output, bytes) = self.output(destination, data.len() as u64)?;
        output.write_all(&data[data.len() - bytes as usize..])
    }

    fn output(&mut self, path: &Path, wanted: u64) -> io::Result<(File, u64)> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("missing log parent"))?;
        let parent = parent.canonicalize()?;
        if !parent.starts_with(&self.current) {
            return Err(io::Error::other("log is outside the capture"));
        }
        let path = parent.join(
            path.file_name()
                .ok_or_else(|| io::Error::other("missing log name"))?,
        );
        if let Some(index) = self.files.iter().position(|item| item.1 == path) {
            self.remove(index)?;
        }
        let wanted = wanted.min(self.limit / 4);
        self.evict(wanted)?;
        let bytes = wanted.min(self.limit.saturating_sub(self.used));
        let relative = path.strip_prefix(&self.current).map_err(io::Error::other)?;
        let mut manifest = File::options()
            .append(true)
            .open(self.current.join(MARKER))?;
        let entry = serde_json::to_string(relative).map_err(io::Error::other)?;
        let output = File::options().write(true).create_new(true).open(&path)?;
        if let Err(error) = writeln!(manifest, "{entry}") {
            drop(output);
            let _ = fs::remove_file(&path);
            return Err(error);
        }
        self.used += bytes;
        self.files.insert(0, (SystemTime::now(), path, bytes));
        Ok((output, bytes))
    }

    fn evict(&mut self, wanted: u64) -> io::Result<()> {
        while self.used.saturating_add(wanted) > self.limit {
            let Some(index) = self
                .files
                .iter()
                .rposition(|item| !item.1.starts_with(&self.current))
                .or_else(|| {
                    (wanted == 0)
                        .then(|| self.files.len().checked_sub(1))
                        .flatten()
                })
            else {
                break;
            };
            self.remove(index)?;
        }
        Ok(())
    }

    fn remove(&mut self, index: usize) -> io::Result<()> {
        let (_, path, size) = &self.files[index];
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.used = self.used.saturating_sub(*size);
        self.files.remove(index);
        Ok(())
    }
}

pub(crate) fn managed_log(path: &Path) -> bool {
    let raw = |path: &Path| {
        path.extension()
            .is_some_and(|ext| matches!(ext.to_str(), Some("log" | "txt" | "csv")))
    };
    raw(path)
        || (path.extension().is_some_and(|ext| ext == "gz")
            && path.file_stem().is_some_and(|stem| raw(Path::new(stem))))
}

fn child_dirs(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let path = entry.path().canonicalize()?;
            if path.parent() == Some(dir) {
                result.push(path);
            }
        }
    }
    Ok(result)
}

fn collect(dir: &Path, files: &mut Vec<(SystemTime, PathBuf, u64)>) -> io::Result<()> {
    let manifest = dir.join(MARKER);
    if !fs::symlink_metadata(&manifest).is_ok_and(|metadata| metadata.is_file()) {
        return Ok(());
    }
    for item in BufReader::new(File::open(manifest)?).lines() {
        let Ok(relative) = serde_json::from_str::<PathBuf>(&item?) else {
            continue;
        };
        if relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            continue;
        }
        let path = dir.join(relative);
        let mut zipped = path.as_os_str().to_owned();
        zipped.push(".gz");
        for candidate in [path, PathBuf::from(zipped)] {
            if !managed_log(&candidate)
                || !candidate
                    .parent()
                    .and_then(|path| path.canonicalize().ok())
                    .is_some_and(|parent| parent.starts_with(dir))
                || files.iter().any(|(_, path, _)| *path == candidate)
            {
                continue;
            }
            if let Ok(metadata) = fs::symlink_metadata(&candidate)
                && metadata.is_file()
            {
                files.push((metadata.modified()?, candidate, metadata.len()));
            }
        }
    }
    Ok(())
}
