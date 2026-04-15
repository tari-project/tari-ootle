//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::BTreeMap, path::Path};

use cargo_toml::Manifest;
use tari_template_lib_types::TemplateAddress;

use crate::{TemplateMetadata, metadata::SCHEMA_VERSION};

/// Parse template metadata from a Cargo.toml file.
///
/// Reads fields from `[package]` (name, version, description, license, repository)
/// and `[package.metadata.tari-template]` (tags, category, documentation, homepage, extra).
pub fn from_cargo_toml(path: &Path) -> Result<TemplateMetadata, CargoTomlError> {
    let manifest = Manifest::from_path(path)?;
    from_manifest(&manifest)
}

/// Parse template metadata from a Cargo.toml string.
pub fn from_cargo_toml_str(content: &str) -> Result<TemplateMetadata, CargoTomlError> {
    let manifest = Manifest::from_str(content)?;
    from_manifest(&manifest)
}

fn from_manifest(manifest: &Manifest) -> Result<TemplateMetadata, CargoTomlError> {
    let package = manifest.package.as_ref().ok_or(CargoTomlError::MissingPackageSection)?;

    let name = package.name.clone();
    let version = package
        .version
        .get()
        .map_err(|_| CargoTomlError::InheritedField("version"))?
        .to_string();
    let description = package
        .description
        .as_ref()
        .and_then(|d| d.get().ok())
        .cloned()
        .unwrap_or_default();
    let license = package.license.as_ref().and_then(|l| l.get().ok()).cloned();
    let repository = package.repository.as_ref().and_then(|r| r.get().ok()).cloned();

    // Read [package.metadata.tari-template]
    let tari_template = package
        .metadata
        .as_ref()
        .and_then(|m| m.get("tari-template"))
        .and_then(|v| v.as_table());

    let tags = tari_template
        .and_then(|t| t.get("tags"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let category = tari_template
        .and_then(|t| t.get("category"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let documentation = tari_template
        .and_then(|t| t.get("documentation"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let homepage = tari_template
        .and_then(|t| t.get("homepage"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let logo_url = tari_template
        .and_then(|t| t.get("logo_url"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let supersedes = tari_template
        .and_then(|t| t.get("supersedes"))
        .and_then(|v| v.as_str())
        .map(TemplateAddress::from_hex)
        .transpose()
        .map_err(|_| CargoTomlError::InvalidTemplateAddress("supersedes"))?;

    let extra = tari_template
        .and_then(|t| t.get("extra"))
        .and_then(|v| v.as_table())
        .map(|t| {
            t.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect::<BTreeMap<String, String>>()
        })
        .unwrap_or_default();

    Ok(TemplateMetadata {
        schema_version: SCHEMA_VERSION,
        name,
        version,
        description,
        tags,
        category,
        repository,
        documentation,
        homepage,
        license,
        logo_url,
        supersedes,
        extra,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum CargoTomlError {
    #[error("Failed to parse Cargo.toml: {0}")]
    Parse(#[from] cargo_toml::Error),
    #[error("Missing [package] section")]
    MissingPackageSection,
    #[error("Field '{0}' uses workspace inheritance which is not supported in this context")]
    InheritedField(&'static str),
    #[error("Invalid template address in field '{0}'")]
    InvalidTemplateAddress(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_cargo_toml() {
        let toml = r#"
[package]
name = "fungible-token"
version = "1.2.0"
description = "A standard fungible token with mint/burn/transfer"
license = "BSD-3-Clause"
repository = "https://github.com/example/fungible-token"

[package.metadata.tari-template]
tags = ["token", "fungible", "defi"]
category = "token"
documentation = "https://docs.example.com/fungible-token"
homepage = "https://example.com"

[package.metadata.tari-template.extra]
audit = "https://example.com/audit-report"
"#;
        let metadata = from_cargo_toml_str(toml).unwrap();
        assert_eq!(metadata.schema_version, SCHEMA_VERSION);
        assert_eq!(metadata.name, "fungible-token");
        assert_eq!(metadata.version, "1.2.0");
        assert_eq!(
            metadata.description,
            "A standard fungible token with mint/burn/transfer"
        );
        assert_eq!(metadata.license.as_deref(), Some("BSD-3-Clause"));
        assert_eq!(
            metadata.repository.as_deref(),
            Some("https://github.com/example/fungible-token")
        );
        assert_eq!(metadata.tags, vec!["token", "fungible", "defi"]);
        assert_eq!(metadata.category.as_deref(), Some("token"));
        assert_eq!(
            metadata.documentation.as_deref(),
            Some("https://docs.example.com/fungible-token")
        );
        assert_eq!(metadata.homepage.as_deref(), Some("https://example.com"));
        assert_eq!(
            metadata.extra.get("audit").map(|s| s.as_str()),
            Some("https://example.com/audit-report")
        );
    }

    #[test]
    fn parse_minimal_cargo_toml() {
        let toml = r#"
[package]
name = "my-template"
version = "0.1.0"
edition = "2024"
"#;
        let metadata = from_cargo_toml_str(toml).unwrap();
        assert_eq!(metadata.name, "my-template");
        assert_eq!(metadata.version, "0.1.0");
        assert_eq!(metadata.description, "");
        assert!(metadata.tags.is_empty());
        assert!(metadata.category.is_none());
        assert!(metadata.license.is_none());
    }

    #[test]
    fn missing_package_section_errors() {
        let toml = r#"
[dependencies]
foo = "1.0"
"#;
        assert!(from_cargo_toml_str(toml).is_err());
    }
}
