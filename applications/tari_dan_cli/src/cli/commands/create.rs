use crate::cli::config::Config;
use crate::cli::util;
use crate::git::repository::GitRepository;
use crate::loading;
use crate::template::collector::TemplateCollector;
use cargo_generate::{GenerateArgs, TemplatePath};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CreateHandlerError {
    #[error("Template not found by name: {0}")]
    TemplateNotFound(String),
}

/// Handle create command.
/// It creates a new Tari template development project.
pub async fn handle(
    config: Config,
    project_template_repo: GitRepository,
    name: &str,
    project_template: Option<&String>,
    target: &PathBuf,
) -> anyhow::Result<()> {
    // selecting project template
    let templates = loading!("Collecting available project templates", TemplateCollector::new(
        project_template_repo.local_folder().join(config.project_template_repository.folder)
    ).collect().await)?;

    let template = match project_template {
        Some(template_name) => {
            templates.iter()
                .filter(|template| template.name().to_lowercase() == template_name.to_lowercase())
                .last()
                .ok_or(CreateHandlerError::TemplateNotFound(template_name.to_string()))?
        }
        None => {
            &util::cli_select("🔎 Select project template", templates.clone())?
        }
    };

    let template_path = template.path().to_str().unwrap().to_string(); // TODO: handle error
    let destination = target.clone();

    // generate new project
    let generate_args = GenerateArgs {
        name: Some(name.to_string()),
        destination: Some(destination.clone()),
        template_path: TemplatePath {
            path: Some(template_path),
            ..TemplatePath::default()
        },
        ..GenerateArgs::default()
    };
    loading!("Generate new project", cargo_generate::generate(generate_args))?;

    // git init
    let mut new_repo = GitRepository::new(destination);
    new_repo.init()?;

    Ok(())
}