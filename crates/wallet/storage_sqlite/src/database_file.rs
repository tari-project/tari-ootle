// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{ffi::OsString, fs, io, path::Path};

/// The files SQLite keeps beside a database, named by appending these suffixes to the database's path: the
/// write-ahead log and its shared-memory index in WAL mode, and the rollback journal a database last opened in another
/// journal mode may have left.
const SIDE_FILE_SUFFIXES: [&str; 3] = ["-wal", "-shm", "-journal"];

/// Moves the wallet database at `from` to `to`, with the files SQLite keeps beside it. Returns `false` when there is no
/// database at `from`.
///
/// The backup needs each of them: the write-ahead log holds commits not yet copied into the database, and a hot
/// rollback journal holds the undo of a transaction the database file may hold in part.
pub fn move_database<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<bool> {
    let (from, to) = (from.as_ref(), to.as_ref());
    let moved = rename_if_exists(from.as_os_str().to_owned(), to.as_os_str().to_owned())?;
    for suffix in SIDE_FILE_SUFFIXES {
        rename_if_exists(with_suffix(from, suffix), with_suffix(to, suffix))?;
    }
    Ok(moved)
}

fn with_suffix(path: &Path, suffix: &str) -> OsString {
    let mut path = path.as_os_str().to_owned();
    path.push(suffix);
    path
}

fn rename_if_exists(from: OsString, to: OsString) -> io::Result<bool> {
    match fs::rename(&from, &to) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_side_files_move_with_the_database() {
        let temp = tempfile::tempdir().unwrap();
        let from = temp.path().join("wallet.sqlite");
        let to = temp.path().join("wallet.sqlite.bak");
        for file in [
            "wallet.sqlite",
            "wallet.sqlite-wal",
            "wallet.sqlite-shm",
            "wallet.sqlite-journal",
        ] {
            fs::write(temp.path().join(file), file).unwrap();
        }

        assert!(move_database(&from, &to).unwrap());

        for (moved, original) in [
            ("wallet.sqlite.bak", "wallet.sqlite"),
            ("wallet.sqlite.bak-wal", "wallet.sqlite-wal"),
            ("wallet.sqlite.bak-shm", "wallet.sqlite-shm"),
            ("wallet.sqlite.bak-journal", "wallet.sqlite-journal"),
        ] {
            assert!(!temp.path().join(original).exists());
            assert_eq!(fs::read_to_string(temp.path().join(moved)).unwrap(), original);
        }
    }

    #[test]
    fn a_missing_database_is_reported_and_a_stale_log_still_moves() {
        let temp = tempfile::tempdir().unwrap();
        let from = temp.path().join("wallet.sqlite");
        fs::write(temp.path().join("wallet.sqlite-wal"), "stale").unwrap();

        assert!(!move_database(&from, temp.path().join("wallet.sqlite.bak")).unwrap());
        assert!(!temp.path().join("wallet.sqlite-wal").exists());
        assert_eq!(
            fs::read_to_string(temp.path().join("wallet.sqlite.bak-wal")).unwrap(),
            "stale"
        );
    }
}
