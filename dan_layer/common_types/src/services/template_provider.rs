//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::types::PublicKey;
use tari_engine_types::TemplateAddress;

pub trait TemplateProvider: Send + Sync + Clone + 'static {
    type Template;
    type Error: std::error::Error + Sync + Send + 'static;

    fn get_template_module(&self, id: &TemplateAddress) -> Result<Option<Self::Template>, Self::Error>;

    fn add_wasm_template(
        &self,
        author_public_key: PublicKey,
        template_address: TemplateAddress,
        template: &[u8],
    ) -> Result<(), Self::Error>;
}
