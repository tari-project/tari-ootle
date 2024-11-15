use crate::cli::commands::create::CreateHandlerError;
use crate::cli::config::Config;
use crate::cli::util;
use crate::git::repository::GitRepository;
use crate::loading;
use crate::template::collector::TemplateCollector;
use anyhow::anyhow;
use cargo_generate::{GenerateArgs, TemplatePath};
use cargo_toml::{Manifest, Workspace};
use convert_case::{Case, Casing};
use std::path::PathBuf;
use tokio::fs;

/// Handle `new` command.
/// It creates a new Tari WASM template development project.
pub async fn handle(
    config: Config,
    wasm_template_repo: GitRepository,
    name: Option<&String>,
    wasm_template: Option<&String>,
    target: PathBuf,
) -> anyhow::Result<()> {
    // selecting wasm template
    let templates = loading!("Collecting available WASM templates", TemplateCollector::new(
        wasm_template_repo.local_folder().join(config.wasm_template_repository.folder)
    ).collect().await)?;

    let template = match wasm_template {
        Some(template_id) => {
            templates.iter()
                .filter(|template| template.name().to_lowercase() == template_id.to_lowercase())
                .last()
                .ok_or(CreateHandlerError::TemplateNotFound(
                    template_id.to_string(),
                    templates.iter().map(|template| template.id().to_string()).collect(),
                ))?
        }
        None => {
            &util::cli_select("🔎 Select WASM template", templates.clone())?
        }
    };

    let project_name = match name {
        None => {
            template.id().to_string()
        }
        Some(name) => name.to_case(Case::Kebab) // TODO: move to this as CLI parser fn
    };

    let template_path = template.path().to_str()
        .ok_or(anyhow!("Invalid template path!"))?
        .to_string();

    // generate new project
    let generate_args = GenerateArgs {
        name: Some(project_name.clone()),
        destination: Some(target.clone()),
        template_path: TemplatePath {
            path: Some(template_path),
            ..TemplatePath::default()
        },
        ..GenerateArgs::default()
    };
    loading!("Generate new project", cargo_generate::generate(generate_args))?;

    // check if target is a cargo project and update Cargo.toml if exists
    let cargo_toml_file = target.join("Cargo.toml");
    if util::file_exists(&cargo_toml_file).await? {
        loading!("Update Cargo.toml", update_cargo_toml(&cargo_toml_file, project_name).await)?;
    }

    Ok(())
}

async fn update_cargo_toml(cargo_toml_file: &PathBuf, project_name: String) -> anyhow::Result<()> {
    let mut cargo_toml = Manifest::from_path(cargo_toml_file)?;
    cargo_toml.workspace = match cargo_toml.workspace {
        Some(mut workspace) => {
            if workspace.members.contains(&project_name) {
                return Err(anyhow!("New project generated, but Cargo.toml already contains a workspace member with the same name: {}", project_name));
            } else {
                workspace.members.push(project_name);
            }
            Some(workspace)
        }
        None => {
            Some(
                Workspace {
                    members: vec![project_name],
                    default_members: vec![],
                    package: None,
                    exclude: vec![],
                    metadata: None,
                    resolver: None,
                    dependencies: Default::default(),
                    lints: None,
                }
            )
        }
    };
    fs::write(&cargo_toml_file, toml::to_string(&cargo_toml)?).await?;
    Ok(())
}