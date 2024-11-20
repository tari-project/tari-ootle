// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::path::PathBuf;

use convert_case::{Case, Casing};
use thiserror::Error;
use tokio::{fs, io};

use crate::templates::{Template, TemplateFile};

const TEMPLATE_DESCRIPTOR_FILE_NAME: &str = "template.toml";

#[derive(Error, Debug)]
pub enum Error {
    #[error("Git2 error: {0}")]
    IO(#[from] io::Error),
    #[error("Failed to deserialize TOML: {0}")]
    TomlDeserialize(#[from] toml::de::Error),
}

pub type CollectorResult<T> = Result<T, Error>;

pub struct Collector {
    local_folder: PathBuf,
}

impl Collector {
    pub fn new(local_folder: PathBuf) -> Self {
        Self { local_folder }
    }

    /// Collect and return all templates from [`Collector::local_folder`].
    pub async fn collect(&self) -> CollectorResult<Vec<Template>> {
        let mut result = vec![];
        Self::collect_templates(&self.local_folder, &mut result).await?;

        Ok(result)
    }

    /// Collecting recursively all the templates from a starting folder `dir`.
    /// All the results will be pushed into `result`.
    async fn collect_templates(dir: &PathBuf, result: &mut Vec<Template>) -> CollectorResult<()> {
        if dir.is_dir() {
            let mut entries_stream = fs::read_dir(dir).await?;
            while let Some(entry) = entries_stream.next_entry().await? {
                if entry.path().is_dir() {
                    Box::pin(Self::collect_templates(&entry.path(), result)).await?;
                } else if let Some(file_name) = entry.file_name().to_str() {
                    if file_name == TEMPLATE_DESCRIPTOR_FILE_NAME {
                        let toml_content = fs::read_to_string(&entry.path()).await?;
                        let template_file: TemplateFile =
                            toml::from_str(toml_content.as_str()).map_err(Error::TomlDeserialize)?;
                        let template_id = match entry.path().parent() {
                            Some(dir) => {
                                if dir.is_dir() {
                                    if let Some(dir_name) = dir.file_name() {
                                        if let Some(dir_name) = dir_name.to_str() {
                                            dir_name.to_case(Case::Snake)
                                        } else {
                                            template_file.name.to_case(Case::Snake)
                                        }
                                    } else {
                                        template_file.name.to_case(Case::Snake)
                                    }
                                } else {
                                    template_file.name.to_case(Case::Snake)
                                }
                            },
                            None => template_file.name.to_case(Case::Snake),
                        };
                        let path = match entry.path().parent() {
                            Some(curr_path) => curr_path.to_path_buf(),
                            None => entry.path(),
                        };
                        result.push(Template::new(
                            path,
                            template_id,
                            template_file.name,
                            template_file.description,
                            template_file.extra.unwrap_or_default(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}
