//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::{IntoSigned, Signable};

use crate::{
    key_managers::{KeyManagerBackend, SignatureOutput},
    models::{KeyBranch, KeyId},
};

#[derive(Debug, Clone)]
pub struct SignerApi<TKm> {
    backend: TKm,
}

impl<TKm> SignerApi<TKm> {
    pub fn new(backend: TKm) -> Self {
        Self { backend }
    }

    pub fn get_signature<T, CTX>(
        &mut self,
        branch: KeyBranch,
        key_id: KeyId,
        context: CTX,
        item: &T,
    ) -> Result<SignatureOutput, TKm::Error>
    where
        T: Signable<CTX>,
        TKm: KeyManagerBackend<T::MessageOutput>,
    {
        let message = item.as_signing_message(context);
        let signature = self.backend.try_sign(branch.as_str(), key_id, message)?;
        Ok(signature)
    }

    pub fn sign_with_context<T, Ctx>(
        &mut self,
        branch: KeyBranch,
        key_id: KeyId,
        context: Ctx,
        item: T,
    ) -> Result<T::SignedOutput, TKm::Error>
    where
        T: IntoSigned<Ctx>,
        TKm: KeyManagerBackend<T::MessageOutput>,
    {
        let output = self.get_signature(branch, key_id, context, &item)?;
        let output = item.into_signed(output.public_key, output.signature);
        Ok(output)
    }

    pub fn sign<T>(&mut self, branch: KeyBranch, key_id: KeyId, item: T) -> Result<T::SignedOutput, TKm::Error>
    where
        T: IntoSigned<()>,
        TKm: KeyManagerBackend<T::MessageOutput>,
    {
        let output = self.get_signature(branch, key_id, (), &item)?;
        let output = item.into_signed(output.public_key, output.signature);
        Ok(output)
    }
}
