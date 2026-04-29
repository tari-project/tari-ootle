//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, fs, path::Path};

use tari_crypto::ristretto::RistrettoSecretKey;
use tari_ootle_transaction::{Network, Transaction};
use tari_template_lib_types::TemplateAddress;
use tari_transaction_manifest::ManifestValue;

use crate::BoxedTransactionBuilder;

pub fn builder<P: AsRef<Path>>(
    signer_secret_key: RistrettoSecretKey,
    network: Network,
    manifest: P,
    globals: HashMap<String, ManifestValue>,
    templates: HashMap<String, TemplateAddress>,
) -> anyhow::Result<BoxedTransactionBuilder> {
    let contents = fs::read_to_string(manifest).unwrap();
    let instructions = tari_transaction_manifest::parse_manifest(&contents, globals, templates, Default::default())?;
    Ok(Box::new(move |_| {
        Transaction::builder(network.as_byte())
            .with_fee_instructions(instructions.fee_instructions.clone())
            .with_instructions(instructions.instructions.clone())
            .build_and_seal(&signer_secret_key)
    }))
}
