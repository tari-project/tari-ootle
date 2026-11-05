//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
};

/// A log file rotated by size into a fixed window: once `<name>.log` would grow past `max_bytes`, it becomes
/// `<name>.1.log`, each `<name>.N.log` becomes `<name>.N+1.log`, and whatever would pass `<name>.<max_rotated>.log`
/// is deleted.
///
/// Writes are buffered. The non-blocking worker that owns this writer flushes it whenever its queue drains.
pub struct RollingFile {
    directory: PathBuf,
    name: String,
    max_bytes: u64,
    max_rotated: usize,
    file: BufWriter<File>,
    written: u64,
}

impl RollingFile {
    pub fn open(directory: &Path, name: &str, max_bytes: u64, max_rotated: usize) -> io::Result<Self> {
        let path = file_path(directory, name, 0);
        let file = open_append(&path)?;
        let written = file.metadata()?.len();
        Ok(Self {
            directory: directory.to_path_buf(),
            name: name.to_string(),
            max_bytes,
            max_rotated,
            file: BufWriter::new(file),
            written,
        })
    }

    fn path(&self, index: usize) -> PathBuf {
        file_path(&self.directory, &self.name, index)
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file.flush()?;
        if self.max_rotated == 0 {
            self.file = BufWriter::new(File::create(self.path(0))?);
            return Ok(());
        }
        ignore_not_found(fs::remove_file(self.path(self.max_rotated)))?;
        for index in (1..self.max_rotated).rev() {
            ignore_not_found(fs::rename(self.path(index), self.path(index + 1)))?;
        }
        fs::rename(self.path(0), self.path(1))?;
        self.file = BufWriter::new(open_append(&self.path(0))?);
        Ok(())
    }
}

impl Write for RollingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.written > 0 && self.written.saturating_add(buf.len() as u64) > self.max_bytes {
            // A rotation that fails keeps appending to the current file, and is next attempted another `max_bytes`
            // later, so a persistent failure neither loses lines nor repeats on every write.
            if let Err(err) = self.rotate() {
                eprintln!("Failed to rotate log file {}: {err}", self.path(0).display());
            }
            self.written = 0;
        }
        let written = self.file.write(buf)?;
        self.written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn file_path(directory: &Path, name: &str, index: usize) -> PathBuf {
    if index == 0 {
        directory.join(format!("{name}.log"))
    } else {
        directory.join(format!("{name}.{index}.log"))
    }
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn ignore_not_found(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(directory: &Path, file: &str) -> Option<String> {
        fs::read_to_string(directory.join(file)).ok()
    }

    #[test]
    fn rotates_into_a_fixed_window_of_numbered_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = RollingFile::open(dir.path(), "engine", 10, 2).unwrap();
        for line in ["aaaaaaaa\n", "bbbbbbbb\n", "cccccccc\n", "dddddddd\n"] {
            file.write_all(line.as_bytes()).unwrap();
        }
        file.flush().unwrap();

        assert_eq!(read(dir.path(), "engine.log").as_deref(), Some("dddddddd\n"));
        assert_eq!(read(dir.path(), "engine.1.log").as_deref(), Some("cccccccc\n"));
        assert_eq!(read(dir.path(), "engine.2.log").as_deref(), Some("bbbbbbbb\n"));
        assert_eq!(read(dir.path(), "engine.3.log"), None);
    }

    #[test]
    fn counts_an_existing_file_towards_its_size() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("engine.log"), "previous\n").unwrap();
        let mut file = RollingFile::open(dir.path(), "engine", 10, 1).unwrap();
        file.write_all(b"next\n").unwrap();
        file.flush().unwrap();

        assert_eq!(read(dir.path(), "engine.log").as_deref(), Some("next\n"));
        assert_eq!(read(dir.path(), "engine.1.log").as_deref(), Some("previous\n"));
    }

    #[test]
    fn truncates_in_place_when_no_rotated_files_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = RollingFile::open(dir.path(), "engine", 10, 0).unwrap();
        file.write_all(b"aaaaaaaa\n").unwrap();
        file.write_all(b"bbbbbbbb\n").unwrap();
        file.flush().unwrap();

        assert_eq!(read(dir.path(), "engine.log").as_deref(), Some("bbbbbbbb\n"));
        assert_eq!(read(dir.path(), "engine.1.log"), None);
    }
}
