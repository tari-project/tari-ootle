//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Build-time metadata generation for Tari Ootle templates.
//!
//! Add this crate as a build-dependency and call it from your `build.rs`:
//!
//! ```toml
//! [build-dependencies]
//! tari_ootle_template_build = "0.1"
//! ```
//!
//! ```rust,no_run
//! // build.rs
//! tari_ootle_template_build::TemplateMetadataBuilder::new()
//!     .description("My awesome template")
//!     .tags(vec!["defi", "token"])
//!     .enable_json_output()
//!     .build()
//!     .expect("Failed to generate template metadata");
//! ```

use std::{collections::BTreeMap, path::PathBuf};

pub use tari_ootle_template_metadata;
use tari_ootle_template_metadata::{MetadataHash, TemplateMetadata, from_cargo_toml};

/// Result of a successful metadata build.
pub struct TemplateBuildOutput {
    /// The computed metadata hash.
    pub hash: MetadataHash,
    /// Path to the generated CBOR file.
    pub cbor_path: PathBuf,
    /// Path to the generated JSON file, if JSON output was enabled.
    pub json_path: Option<PathBuf>,
    /// The resolved metadata.
    pub metadata: TemplateMetadata,
}

/// Builder for generating template metadata files at build time.
///
/// Reads metadata from the crate's `Cargo.toml` and generates CBOR (and optionally JSON)
/// metadata files in the `OUT_DIR`. Any field set on the builder overrides the corresponding
/// value from `Cargo.toml`.
pub struct TemplateMetadataBuilder {
    json_output: bool,
    description: Option<String>,
    tags: Option<Vec<String>>,
    category: Option<String>,
    repository: Option<String>,
    documentation: Option<String>,
    homepage: Option<String>,
    license: Option<String>,
    extra: Option<BTreeMap<String, String>>,
}

impl Default for TemplateMetadataBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateMetadataBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> Self {
        Self {
            json_output: false,
            description: None,
            tags: None,
            category: None,
            repository: None,
            documentation: None,
            homepage: None,
            license: None,
            extra: None,
        }
    }

    /// Enable additional JSON output alongside CBOR.
    pub fn enable_json_output(mut self) -> Self {
        self.json_output = true;
        self
    }

    /// Override the description from Cargo.toml.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Override the tags from Cargo.toml.
    pub fn tags(mut self, tags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.tags = Some(tags.into_iter().map(Into::into).collect());
        self
    }

    /// Override the category from Cargo.toml.
    pub fn category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }

    /// Override the repository URL from Cargo.toml.
    pub fn repository(mut self, repository: impl Into<String>) -> Self {
        self.repository = Some(repository.into());
        self
    }

    /// Override the documentation URL from Cargo.toml.
    pub fn documentation(mut self, documentation: impl Into<String>) -> Self {
        self.documentation = Some(documentation.into());
        self
    }

    /// Override the homepage URL from Cargo.toml.
    pub fn homepage(mut self, homepage: impl Into<String>) -> Self {
        self.homepage = Some(homepage.into());
        self
    }

    /// Override the license from Cargo.toml.
    pub fn license(mut self, license: impl Into<String>) -> Self {
        self.license = Some(license.into());
        self
    }

    /// Override the extra metadata map from Cargo.toml.
    pub fn extra(mut self, extra: BTreeMap<String, String>) -> Self {
        self.extra = Some(extra);
        self
    }

    /// Add a single extra metadata key-value pair, merging with existing extra entries.
    pub fn extra_entry(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra
            .get_or_insert_with(BTreeMap::new)
            .insert(key.into(), value.into());
        self
    }

    /// Apply builder overrides to the given metadata.
    fn apply_overrides(&self, metadata: &mut TemplateMetadata) {
        if let Some(ref description) = self.description {
            metadata.description = description.clone();
        }
        if let Some(ref tags) = self.tags {
            metadata.tags = tags.clone();
        }
        if let Some(ref category) = self.category {
            metadata.category = Some(category.clone());
        }
        if let Some(ref repository) = self.repository {
            metadata.repository = Some(repository.clone());
        }
        if let Some(ref documentation) = self.documentation {
            metadata.documentation = Some(documentation.clone());
        }
        if let Some(ref homepage) = self.homepage {
            metadata.homepage = Some(homepage.clone());
        }
        if let Some(ref license) = self.license {
            metadata.license = Some(license.clone());
        }
        if let Some(ref extra) = self.extra {
            metadata.extra = extra.clone();
        }
    }

    /// Generate metadata files from `Cargo.toml` with any builder overrides applied.
    ///
    /// Reads `CARGO_MANIFEST_DIR/Cargo.toml`, applies overrides, writes CBOR (and optionally JSON)
    /// to `OUT_DIR`, and emits `cargo::metadata=TEMPLATE_METADATA_HASH=<hex>`.
    pub fn build(self) -> Result<TemplateBuildOutput, TemplateBuildError> {
        let manifest_dir =
            std::env::var("CARGO_MANIFEST_DIR").map_err(|_| TemplateBuildError::MissingEnvVar("CARGO_MANIFEST_DIR"))?;
        let out_dir = std::env::var("OUT_DIR").map_err(|_| TemplateBuildError::MissingEnvVar("OUT_DIR"))?;

        self.build_inner(&manifest_dir, &out_dir)
    }

    fn build_inner(self, manifest_dir: &str, out_dir: &str) -> Result<TemplateBuildOutput, TemplateBuildError> {
        let cargo_toml_path = PathBuf::from(manifest_dir).join("Cargo.toml");
        let mut metadata = from_cargo_toml(&cargo_toml_path)?;

        self.apply_overrides(&mut metadata);

        // Write CBOR
        let cbor = metadata.to_cbor()?;
        let cbor_path = PathBuf::from(out_dir).join("template_metadata.cbor");
        std::fs::write(&cbor_path, &cbor)?;

        // Optionally write JSON
        let json_path = if self.json_output {
            let json = metadata.to_json()?;
            let path = PathBuf::from(out_dir).join("template_metadata.json");
            std::fs::write(&path, json)?;
            Some(path)
        } else {
            None
        };

        // Compute hash
        let hash = metadata.hash()?;

        // Emit cargo metadata
        println!("cargo::metadata=TEMPLATE_METADATA_HASH={}", hash);
        println!("cargo:rerun-if-changed=Cargo.toml");

        Ok(TemplateBuildOutput {
            hash,
            cbor_path,
            json_path,
            metadata,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TemplateBuildError {
    #[error("Environment variable {0} not set — must be run from build.rs")]
    MissingEnvVar(&'static str),
    #[error("Failed to parse Cargo.toml metadata: {0}")]
    CargoToml(#[from] tari_ootle_template_metadata::CargoTomlError),
    #[error("Metadata error: {0}")]
    Metadata(#[from] tari_ootle_template_metadata::TemplateMetadataError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use tari_ootle_template_metadata::TemplateMetadata;

    use super::*;

    fn create_test_cargo_toml(dir: &std::path::Path) {
        let toml = r#"
[package]
name = "test-template"
version = "1.0.0"
description = "Original description"
license = "MIT"
repository = "https://github.com/example/original"

[package.metadata.tari-template]
tags = ["original"]
category = "utility"
"#;
        std::fs::write(dir.join("Cargo.toml"), toml).unwrap();
    }

    #[test]
    fn build_generates_cbor_file() {
        let dir = tempfile::tempdir().unwrap();
        create_test_cargo_toml(dir.path());
        let out_dir = dir.path().join("out");
        std::fs::create_dir_all(&out_dir).unwrap();

        let output = TemplateMetadataBuilder::new()
            .build_inner(dir.path().to_str().unwrap(), out_dir.to_str().unwrap())
            .unwrap();

        assert!(output.cbor_path.exists());
        assert!(output.json_path.is_none());

        let cbor_bytes = std::fs::read(&output.cbor_path).unwrap();
        let decoded = TemplateMetadata::from_cbor(&cbor_bytes).unwrap();
        assert_eq!(decoded.name, "test-template");
        assert_eq!(decoded.version, "1.0.0");
        assert_eq!(decoded.description, "Original description");
    }

    #[test]
    fn build_generates_json_when_enabled() {
        let dir = tempfile::tempdir().unwrap();
        create_test_cargo_toml(dir.path());
        let out_dir = dir.path().join("out");
        std::fs::create_dir_all(&out_dir).unwrap();

        let output = TemplateMetadataBuilder::new()
            .enable_json_output()
            .build_inner(dir.path().to_str().unwrap(), out_dir.to_str().unwrap())
            .unwrap();

        let json_path = output.json_path.unwrap();
        assert!(json_path.exists());
        let json_str = std::fs::read_to_string(&json_path).unwrap();
        let decoded = TemplateMetadata::from_json(&json_str).unwrap();
        assert_eq!(decoded.name, "test-template");
    }

    #[test]
    fn overrides_replace_cargo_toml_values() {
        let dir = tempfile::tempdir().unwrap();
        create_test_cargo_toml(dir.path());
        let out_dir = dir.path().join("out");
        std::fs::create_dir_all(&out_dir).unwrap();

        let output = TemplateMetadataBuilder::new()
            .description("Overridden description")
            .tags(vec!["new-tag-1", "new-tag-2"])
            .category("defi")
            .repository("https://github.com/example/overridden")
            .documentation("https://docs.example.com")
            .homepage("https://example.com")
            .license("BSD-3-Clause")
            .build_inner(dir.path().to_str().unwrap(), out_dir.to_str().unwrap())
            .unwrap();

        let m = &output.metadata;
        assert_eq!(m.name, "test-template");
        assert_eq!(m.description, "Overridden description");
        assert_eq!(m.tags, vec!["new-tag-1", "new-tag-2"]);
        assert_eq!(m.category.as_deref(), Some("defi"));
        assert_eq!(m.repository.as_deref(), Some("https://github.com/example/overridden"));
        assert_eq!(m.documentation.as_deref(), Some("https://docs.example.com"));
        assert_eq!(m.homepage.as_deref(), Some("https://example.com"));
        assert_eq!(m.license.as_deref(), Some("BSD-3-Clause"));

        // CBOR file should contain overridden values
        let cbor_bytes = std::fs::read(&output.cbor_path).unwrap();
        let decoded = TemplateMetadata::from_cbor(&cbor_bytes).unwrap();
        assert_eq!(decoded.description, "Overridden description");
        assert_eq!(decoded.tags, vec!["new-tag-1", "new-tag-2"]);
    }

    #[test]
    fn extra_entries_are_written() {
        let dir = tempfile::tempdir().unwrap();
        create_test_cargo_toml(dir.path());
        let out_dir = dir.path().join("out");
        std::fs::create_dir_all(&out_dir).unwrap();

        let output = TemplateMetadataBuilder::new()
            .extra_entry("audit", "https://example.com/audit")
            .extra_entry("contact", "dev@example.com")
            .build_inner(dir.path().to_str().unwrap(), out_dir.to_str().unwrap())
            .unwrap();

        assert_eq!(
            output.metadata.extra.get("audit").map(String::as_str),
            Some("https://example.com/audit")
        );
        assert_eq!(
            output.metadata.extra.get("contact").map(String::as_str),
            Some("dev@example.com")
        );
    }

    #[test]
    fn hash_is_deterministic_across_builds() {
        let dir = tempfile::tempdir().unwrap();
        create_test_cargo_toml(dir.path());
        let out_dir = dir.path().join("out");
        std::fs::create_dir_all(&out_dir).unwrap();

        let output1 = TemplateMetadataBuilder::new()
            .build_inner(dir.path().to_str().unwrap(), out_dir.to_str().unwrap())
            .unwrap();
        let output2 = TemplateMetadataBuilder::new()
            .build_inner(dir.path().to_str().unwrap(), out_dir.to_str().unwrap())
            .unwrap();

        assert_eq!(output1.hash, output2.hash);
    }

    #[test]
    fn hash_changes_with_overrides() {
        let dir = tempfile::tempdir().unwrap();
        create_test_cargo_toml(dir.path());
        let out_dir = dir.path().join("out");
        std::fs::create_dir_all(&out_dir).unwrap();

        let output_original = TemplateMetadataBuilder::new()
            .build_inner(dir.path().to_str().unwrap(), out_dir.to_str().unwrap())
            .unwrap();
        let output_overridden = TemplateMetadataBuilder::new()
            .description("Different description")
            .build_inner(dir.path().to_str().unwrap(), out_dir.to_str().unwrap())
            .unwrap();

        assert_ne!(output_original.hash, output_overridden.hash);
    }

    #[test]
    fn build_errors_on_missing_cargo_toml() {
        let dir = tempfile::tempdir().unwrap();
        let out_dir = dir.path().join("out");
        std::fs::create_dir_all(&out_dir).unwrap();

        let result =
            TemplateMetadataBuilder::new().build_inner(dir.path().to_str().unwrap(), out_dir.to_str().unwrap());
        assert!(result.is_err());
    }
}
