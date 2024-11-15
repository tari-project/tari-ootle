use crate::cli::config::Config;
use crate::cli::util;
use crate::git::repository::GitRepository;
use crate::loading;
use crate::template::collector::TemplateCollector;
use anyhow::anyhow;
use cargo_generate::{GenerateArgs, TemplatePath};
use convert_case::{Case, Casing};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CreateHandlerError {
    #[error("Template not found by name: {0}. Possible values: {1:?}")]
    TemplateNotFound(String, Vec<String>),
}

/// Handle `create` command.
/// It creates a new Tari template development project.
pub async fn handle(
    config: Config,
    project_template_repo: GitRepository,
    name: &str,
    project_template: Option<&String>,
    target: PathBuf,
) -> anyhow::Result<()> {
    let name = name.to_case(Case::Kebab); // TODO: move to this as CLI parser fn

    // selecting project template
    let templates = loading!("Collecting available project templates", TemplateCollector::new(
        project_template_repo.local_folder().join(config.project_template_repository.folder)
    ).collect().await)?;

    let template = match project_template {
        Some(template_id) => {
            templates.iter()
                .filter(|template| template.id().to_lowercase() == template_id.to_lowercase())
                .last()
                .ok_or(CreateHandlerError::TemplateNotFound(
                    template_id.to_string(),
                    templates.iter().map(|template| template.id().to_string()).collect(),
                ))?
        }
        None => {
            &util::cli_select("🔎 Select project template", templates.clone())?
        }
    };

    let template_path = template.path().to_str()
        .ok_or(anyhow!("Invalid template path!"))?
        .to_string();

    // generate new project
    let generate_args = GenerateArgs {
        name: Some(name.to_string()),
        destination: Some(target.clone()),
        template_path: TemplatePath {
            path: Some(template_path),
            ..TemplatePath::default()
        },
        ..GenerateArgs::default()
    };
    loading!("Generate new project", cargo_generate::generate(generate_args))?;

    // git init
    let mut new_repo = GitRepository::new(target.join(name));
    new_repo.init()?;

    Ok(())
}