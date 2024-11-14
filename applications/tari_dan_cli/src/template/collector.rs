use convert_case::{Case, Casing};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::path::PathBuf;
use termimad::crossterm::style::Color;
use termimad::MadSkin;
use thiserror::Error;
use tokio::{fs, io};

const TEMPLATE_DESCRIPTOR_FILE_NAME: &str = "template.toml";

#[derive(Clone, Debug)]
pub struct Template {
    path: PathBuf,
    id: String,
    name: String,
    description: String,
}

impl Display for Template {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut skin = MadSkin::default();
        skin.bold.set_fg(Color::Magenta);
        let formatted_name = skin.inline(format!("**{}**", self.name).as_str()).to_string();
        let formatted_description = skin.inline(self.description.as_str()).to_string();
        write!(f, "{} - {}", formatted_name, formatted_description)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateFile {
    name: String,
    description: String,
}

impl Template {
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("Git2 error: {0}")]
    IO(#[from] io::Error),
    #[error("Failed to deserialize TOML: {0}")]
    TomlDeserialize(#[from] toml::de::Error),
}

pub type TemplateCollectorResult<T> = Result<T, Error>;

pub struct TemplateCollector {
    local_folder: PathBuf,
}

impl TemplateCollector {
    pub fn new(local_folder: PathBuf) -> Self {
        Self { local_folder }
    }

    pub async fn collect(&self) -> TemplateCollectorResult<Vec<Template>> {
        let mut result = vec![];
        Self::collect_templates(&self.local_folder, &mut result).await?;

        Ok(result)
    }

    async fn collect_templates(dir: &PathBuf, result: &mut Vec<Template>) -> TemplateCollectorResult<()>
    {
        if dir.is_dir() {
            let mut entries_stream = fs::read_dir(dir).await?;
            while let Some(entry) = entries_stream.next_entry().await? {
                if entry.path().is_dir() {
                    Box::pin(Self::collect_templates(&entry.path(), result)).await?;
                } else if let Some(file_name) = entry.file_name().to_str() {
                    if file_name == TEMPLATE_DESCRIPTOR_FILE_NAME {
                        let toml_content = fs::read_to_string(&entry.path()).await?;
                        let template_file: TemplateFile = toml::from_str(toml_content.as_str()).map_err(Error::TomlDeserialize)?;
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
                            }
                            None => {
                                template_file.name.to_case(Case::Snake)
                            }
                        };
                        let path = match entry.path().parent() {
                            Some(curr_path) => {
                                curr_path.to_path_buf()
                            }
                            None => entry.path(),
                        };
                        result.push(Template {
                            path,
                            id: template_id,
                            name: template_file.name,
                            description: template_file.description,
                        });
                    }
                }
            }
        }
        Ok(())
    }
}