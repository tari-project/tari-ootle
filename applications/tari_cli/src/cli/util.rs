// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{fs::Metadata, io, path::PathBuf};

use dialoguer::FuzzySelect;
use tokio::fs;

pub async fn create_dir(dir: &PathBuf) -> io::Result<()> {
    fs::create_dir_all(dir).await
}

pub async fn file_exists(file: &PathBuf) -> io::Result<bool> {
    Ok(fs::try_exists(file).await? && path_metadata(file).await?.is_file())
}

pub async fn dir_exists(dir: &PathBuf) -> io::Result<bool> {
    Ok(fs::try_exists(dir).await? && path_metadata(dir).await?.is_dir())
}

pub async fn path_metadata(path: &PathBuf) -> io::Result<Metadata> {
    fs::metadata(path).await
}

pub fn cli_select<T: ToString + Clone>(prompt: &str, items: &[T]) -> anyhow::Result<T> {
    let selection = FuzzySelect::new()
        .with_prompt(prompt)
        .highlight_matches(true)
        .items(items)
        .interact()?;

    Ok(items[selection].clone())
}
