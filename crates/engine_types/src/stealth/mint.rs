//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::collections::BTreeMap;

use tari_crypto::ristretto::RistrettoPublicKey;
use tari_template_lib::{models::StealthOutputsStatement, prelude::PedersenCommitmentBytes};

use crate::{
    crypto::PrivateOutput,
    resource_container::ResourceError,
    stealth::outputs::validate_stealth_outputs_statement,
    ToByteType,
};

pub fn mint_stealth_outputs(
    stmt: &StealthOutputsStatement,
    view_key: Option<&RistrettoPublicKey>,
) -> Result<StealthResourceContainer, ResourceError> {
    let validated_proof = validate_stealth_outputs_statement(stmt, view_key)?;
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
