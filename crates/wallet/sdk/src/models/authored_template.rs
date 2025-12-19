// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_template_abi::{FunctionDef, TemplateDef};
use tari_template_lib::types::{crypto::RistrettoPublicKeyBytes, TemplateAddress};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthoredTemplateModel {
    pub author_public_key: RistrettoPublicKeyBytes,
    pub address: TemplateAddress,
    pub name: String,
    pub tari_version: String,
    pub functions: Vec<FunctionDef>,
}

impl AuthoredTemplateModel {
    pub fn new(
        author_public_key: RistrettoPublicKeyBytes,
        template_address: TemplateAddress,
        template_def: TemplateDef,
    ) -> Self {
        Self {
            author_public_key,
            address: template_address,
            name: template_def.template_name().to_string(),
            tari_version: template_def.tari_version().to_string(),
            functions: template_def.functions().to_vec(),
        }
    }
}
