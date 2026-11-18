//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::BTreeMap, path::Path};

use cargo_toml::Manifest;
use tari_template_lib_types::TemplateAddress;
use url::Url;

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
    let description = match package.description.as_ref() {
        Some(d) => d
            .get()
            .map_err(|_| CargoTomlError::InheritedField("description"))?
            .clone(),
        None => String::new(),
    };
    let license = match package.license.as_ref() {
        Some(l) => Some(l.get().map_err(|_| CargoTomlError::InheritedField("license"))?.clone()),
        None => None,
    };
    let repository = match package.repository.as_ref() {
        Some(r) => {
            let repo = r.get().map_err(|_| CargoTomlError::InheritedField("repository"))?;
            Some(Url::parse(repo).map_err(|e| CargoTomlError::InvalidUrl("repository", e))?)
        },
        None => None,
    };

    // Read [package.metadata.tari-template]
    let tari_template = match package.metadata.as_ref().and_then(|m| m.get("tari-template")) {
        Some(v) => Some(
            v.as_table()
                .ok_or(CargoTomlError::InvalidFieldType("tari-template", "table"))?,
        ),
        None => None,
    };

    let get_str = |field: &'static str| -> Result<Option<&str>, CargoTomlError> {
        match tari_template.and_then(|t| t.get(field)) {
            Some(v) => v
                .as_str()
                .map(Some)
                .ok_or(CargoTomlError::InvalidFieldType(field, "string")),
            None => Ok(None),
        }
    };

    let tags = parse_tags(tari_template.and_then(|t| t.get("tags")))?;

    let category = get_str("category")?.map(String::from);

    let commit_hash = get_str("commit_hash")?
        .map(|s| gix_hash::ObjectId::from_hex(s.as_bytes()))
        .transpose()
        .map_err(CargoTomlError::InvalidCommitHash)?;

    let documentation = get_str("documentation")?
        .map(Url::parse)
        .transpose()
        .map_err(|e| CargoTomlError::InvalidUrl("documentation", e))?;

    let homepage = get_str("homepage")?
        .map(Url::parse)
        .transpose()
        .map_err(|e| CargoTomlError::InvalidUrl("homepage", e))?;

    let logo_url = get_str("logo_url")?
        .map(Url::parse)
        .transpose()
        .map_err(|e| CargoTomlError::InvalidUrl("logo_url", e))?;

    let supersedes = get_str("supersedes")?
        .map(|s| TemplateAddress::from_hex(s.strip_prefix("template_").unwrap_or(s)))
        .transpose()
        .map_err(|_| CargoTomlError::InvalidTemplateAddress("supersedes"))?;

    let extra = parse_extra(tari_template.and_then(|t| t.get("extra")))?;

    Ok(TemplateMetadata {
        schema_version: SCHEMA_VERSION,
        name,
        version,
        description,
        tags,
        category,
        repository,
        commit_hash,
        documentation,
        homepage,
        license,
        logo_url,
        supersedes,
        functions: Vec::new(),
        extra,
    })
}

fn parse_tags(value: Option<&cargo_toml::Value>) -> Result<Vec<String>, CargoTomlError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let invalid = || CargoTomlError::InvalidFieldType("tags", "array of strings");
    value
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .map(|item| item.as_str().map(String::from).ok_or_else(invalid))
        .collect()
}

fn parse_extra(value: Option<&cargo_toml::Value>) -> Result<BTreeMap<String, String>, CargoTomlError> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    let invalid = || CargoTomlError::InvalidFieldType("extra", "table of strings");
    value
        .as_table()
        .ok_or_else(invalid)?
        .iter()
        .map(|(key, val)| val.as_str().map(|s| (key.clone(), s.to_string())).ok_or_else(invalid))
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum CargoTomlError {
    #[error("Failed to parse Cargo.toml: {0}")]
    Parse(#[from] cargo_toml::Error),
    #[error("Missing [package] section")]
    MissingPackageSection,
    #[error("Field '{0}' uses workspace inheritance which is not supported in this context")]
    InheritedField(&'static str),
    #[error("Field '{0}' must be a {1}")]
    InvalidFieldType(&'static str, &'static str),
    #[error("Invalid template address in field '{0}'")]
    InvalidTemplateAddress(&'static str),
    #[error("Invalid URL in field '{0}': {1}")]
    InvalidUrl(&'static str, url::ParseError),
    #[error("Invalid commit hash: {0}")]
    InvalidCommitHash(gix_hash::decode::Error),
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
            metadata.repository.as_ref().map(|u| u.as_str()),
            Some("https://github.com/example/fungible-token")
        );
        assert_eq!(metadata.tags, vec!["token", "fungible", "defi"]);
        assert_eq!(metadata.category.as_deref(), Some("token"));
        assert_eq!(
            metadata.documentation.as_ref().map(|u| u.as_str()),
            Some("https://docs.example.com/fungible-token")
        );
        assert_eq!(
            metadata.homepage.as_ref().map(|u| u.as_str()),
            Some("https://example.com/")
        );
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

    #[test]
    fn accepts_prefixed_supersedes_and_rejects_invalid_field_types() {
        let toml = r#"
[package]
name = "upgraded-template"
version = "2.0.0"

[package.metadata.tari-template]
supersedes = "template_0000000000000000000000000000000000000000000000000000000000000001"
"#;
        let metadata = from_cargo_toml_str(toml).unwrap();
        assert_eq!(
            metadata.supersedes,
            Some(
                TemplateAddress::from_hex("0000000000000000000000000000000000000000000000000000000000000001").unwrap()
            )
        );

        let bad_tags = r#"
[package]
name = "bad-tags"
version = "1.0.0"

[package.metadata.tari-template]
tags = "not-an-array"
"#;
        assert!(matches!(
            from_cargo_toml_str(bad_tags),
            Err(CargoTomlError::InvalidFieldType("tags", _))
        ));

        let inherited_license = r#"
[package]
name = "inherited-license"
version = "1.0.0"
license.workspace = true
"#;
        assert!(matches!(
            from_cargo_toml_str(inherited_license),
            Err(CargoTomlError::InheritedField("license"))
        ));
    }

    fn parse_with_package_tail(tail: &str) -> Result<TemplateMetadata, CargoTomlError> {
        from_cargo_toml_str(&format!("[package]\nname = \"t\"\nversion = \"1.0.0\"\n{tail}"))
    }

    #[test]
    fn rejects_non_string_tag_entry() {
        let result = parse_with_package_tail("[package.metadata.tari-template]\ntags = [\"a\", 1]\n");
        assert!(matches!(result, Err(CargoTomlError::InvalidFieldType("tags", _))));
    }

    #[test]
    fn rejects_non_string_extra_value() {
        let result = parse_with_package_tail("[package.metadata.tari-template.extra]\naudit = 1\n");
        assert!(matches!(result, Err(CargoTomlError::InvalidFieldType("extra", _))));
    }

    #[test]
    fn rejects_non_table_extra() {
        let result = parse_with_package_tail("[package.metadata.tari-template]\nextra = \"x\"\n");
        assert!(matches!(result, Err(CargoTomlError::InvalidFieldType("extra", _))));
    }

    #[test]
    fn rejects_non_string_category() {
        let result = parse_with_package_tail("[package.metadata.tari-template]\ncategory = 1\n");
        assert!(matches!(
            result,
            Err(CargoTomlError::InvalidFieldType("category", "string"))
        ));
    }

    #[test]
    fn rejects_non_string_homepage() {
        let result = parse_with_package_tail("[package.metadata.tari-template]\nhomepage = true\n");
        assert!(matches!(
            result,
            Err(CargoTomlError::InvalidFieldType("homepage", "string"))
        ));
    }

    #[test]
    fn rejects_non_table_tari_template() {
        let result = parse_with_package_tail("[package.metadata]\ntari-template = \"x\"\n");
        assert!(matches!(
            result,
            Err(CargoTomlError::InvalidFieldType("tari-template", "table"))
        ));
    }

    #[test]
    fn rejects_inherited_description() {
        let result = parse_with_package_tail("description.workspace = true\n");
        assert!(matches!(result, Err(CargoTomlError::InheritedField("description"))));
    }

    #[test]
    fn rejects_inherited_repository() {
        let result = parse_with_package_tail("repository.workspace = true\n");
        assert!(matches!(result, Err(CargoTomlError::InheritedField("repository"))));
    }
}
