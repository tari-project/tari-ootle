//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::borrow::Cow;

use tari_engine::{abi::TemplateDef, template::LoadedTemplate};
use tari_ootle_common_types::Epoch;
use tari_ootle_storage::{StorageError, global::DbTemplate};
use tari_template_lib_types::{Hash32, TemplateAddress, crypto::RistrettoPublicKeyBytes};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TemplateMetadata {
    pub name: String,
    pub address: TemplateAddress,
    /// SHA hash of binary
    pub binary_sha: Hash32,
    pub author_public_key: RistrettoPublicKeyBytes,
    pub code_size: usize,
    pub epoch: Epoch,
}

// TODO: Allow fetching of just the template metadata without the compiled code
impl From<DbTemplate> for TemplateMetadata {
    fn from(record: DbTemplate) -> Self {
        TemplateMetadata {
            name: record.template_name,
            address: record.template_address,
            binary_sha: record.binary_hash,
            author_public_key: record.author_public_key,
            code_size: record.code.as_ref().map(|code| code.len()).unwrap_or_default(),
            epoch: record.epoch,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TemplateCode {
    StaticWasm(&'static [u8]),
    CompiledWasm(Box<[u8]>),
}

impl TemplateCode {
    pub fn as_wasm_code(&self) -> Option<&[u8]> {
        match self {
            TemplateCode::StaticWasm(code) => Some(code),
            TemplateCode::CompiledWasm(code) => Some(code),
        }
    }

    pub fn as_raw_bytes(&self) -> &[u8] {
        match self {
            TemplateCode::StaticWasm(code) => code,
            TemplateCode::CompiledWasm(code) => code,
        }
    }

    pub fn into_raw_bytes(self) -> Cow<'static, [u8]> {
        match self {
            TemplateCode::StaticWasm(code) => Cow::Borrowed(code),
            TemplateCode::CompiledWasm(code) => Cow::Owned(code.into_vec()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedTemplateWithMetadata {
    pub metadata: TemplateMetadata,
    pub loaded: LoadedTemplate,
}

impl LoadedTemplateWithMetadata {
    pub fn template_def(&self) -> &TemplateDef {
        self.loaded.template_def()
    }

    pub fn code_size(&self) -> usize {
        self.metadata.code_size
    }

    pub fn template_name(&self) -> &str {
        &self.metadata.name
    }
}

#[derive(Debug, Clone)]
pub struct Template {
    pub metadata: TemplateMetadata,
    pub code: TemplateCode,
}

// we encapsulate the db row format to not expose it to the caller
impl TryFrom<DbTemplate> for Template {
    type Error = StorageError;

    fn try_from(record: DbTemplate) -> Result<Self, Self::Error> {
        Ok(Template {
            metadata: TemplateMetadata {
                name: record.template_name,
                address: record.template_address,
                binary_sha: record.binary_hash,
                author_public_key: record.author_public_key,
                code_size: record.code.as_ref().map(|code| code.len()).unwrap_or_default(),
                epoch: record.epoch,
            },
            code: {
                let code = record.code.ok_or_else(|| StorageError::DataInconsistency {
                    details: format!(
                        "Template type {} does not have the required binary data",
                        record.template_type
                    ),
                })?;
                TemplateCode::CompiledWasm(code.into_boxed_slice())
            },
        })
    }
}
