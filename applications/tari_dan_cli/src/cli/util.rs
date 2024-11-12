use std::io;
use std::path::PathBuf;
use tokio::fs;

pub async fn create_dir(dir: &PathBuf) -> io::Result<()> {
    fs::create_dir_all(dir).await
}

pub async fn file_exists(file: &PathBuf) -> io::Result<bool> {
    fs::try_exists(file).await
}