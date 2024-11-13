use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::string::ToString;
use tokio::fs;
use tokio::io::AsyncWriteExt;

pub const VALID_OVERRIDE_KEYS: &[&str] = &[
    "project_template_repository.url",
    "project_template_repository.branch",
    "project_template_repository.folder",
    "wasm_template_repository.url",
    "wasm_template_repository.branch",
    "wasm_template_repository.folder",
];

/// CLI configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub project_template_repository: TemplateRepository,
    pub wasm_template_repository: TemplateRepository,
}

/// Repository that holds templates to generate project and Tari templates.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateRepository {
    pub url: String,
    pub branch: String,
    pub folder: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            project_template_repository: TemplateRepository {
                url: "https://github.com/tari-project/wasm-template".to_string(),
                branch: "main".to_string(),
                folder: "project_templates".to_string(),
            },
            wasm_template_repository: TemplateRepository {
                url: "https://github.com/tari-project/wasm-template".to_string(),
                branch: "main".to_string(),
                folder: "wasm_templates".to_string(),
            },
        }
    }
}

impl Config {
    pub async fn open(path: &PathBuf) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path).await?;
        Ok(
            toml::from_str::<Config>(content.as_str())?
        )
    }
    pub async fn write_to_file(&self, path: &PathBuf) -> anyhow::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(path).await?;
        let content = toml::to_string(self)?;
        let _ = file.write(content.as_bytes()).await?;
        file.flush().await?;
        Ok(())
    }
    pub fn is_override_key_valid(key: &str) -> bool {
        VALID_OVERRIDE_KEYS.contains(&key)
    }

    pub fn override_data(&mut self, key: &str, value: &str) -> anyhow::Result<&mut Self> {
        if !Self::is_override_key_valid(key) {
            return Err(anyhow!("Invalid key: {}", key));
        }

        match key {
            "project_template_repository.url" => {
                self.project_template_repository.url = value.to_string();
            }
            "project_template_repository.branch" => {
                self.project_template_repository.branch = value.to_string();
            }
            "project_template_repository.folder" => {
                self.project_template_repository.folder = value.to_string();
            }
            "wasm_template_repository.url" => {
                self.wasm_template_repository.url = value.to_string();
            }
            "wasm_template_repository.branch" => {
                self.wasm_template_repository.branch = value.to_string();
            }
            "wasm_template_repository.folder" => {
                self.wasm_template_repository.folder = value.to_string();
            }
            _ => {}
        }

        Ok(self)
    }
}