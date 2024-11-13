use std::fs::Metadata;
use std::io;
use std::path::PathBuf;
use tokio::fs;

pub async fn create_dir(dir: &PathBuf) -> io::Result<()> {
    fs::create_dir_all(dir).await
}

pub async fn file_exists(file: &PathBuf) -> io::Result<bool> {
    Ok(
        fs::try_exists(file).await? && path_metadata(file).await?.is_file()
    )
}

pub async fn dir_exists(dir: &PathBuf) -> io::Result<bool> {
    Ok(
        fs::try_exists(dir).await? && path_metadata(dir).await?.is_dir()
    )
}

pub async fn path_metadata(path: &PathBuf) -> io::Result<Metadata> {
    fs::metadata(path).await
}