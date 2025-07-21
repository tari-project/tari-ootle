//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::collections::BTreeMap;

use tari_crypto::ristretto::RistrettoPublicKey;
use tari_template_lib::{models::StealthOutputStatement, prelude::PedersenCommitmentBytes};

use crate::{
    crypto::PrivateOutput,
    resource_container::ResourceError,
    stealth::validation::validate_stealth_statement,
    ToByteType,
};

pub fn mint_stealth_outputs(
    stmt: &StealthOutputStatement,
    view_key: Option<&RistrettoPublicKey>,
) -> Result<StealthResourceContainer, ResourceError> {
    let validated_proof = validate_stealth_statement(stmt, view_key)?;
    Ok(StealthResourceContainer {
        outputs: validated_proof
            .outputs
            .into_iter()
            .map(|o| (o.commitment.to_byte_type(), o.into()))
            .collect(),
    })
}

pub struct StealthResourceContainer {
    pub outputs: BTreeMap<PedersenCommitmentBytes, PrivateOutput>,
}
