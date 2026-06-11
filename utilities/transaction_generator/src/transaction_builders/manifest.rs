//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, fs, path::Path};

use tari_crypto::ristretto::RistrettoSecretKey;
use tari_ootle_common_types::SubstateRequirement;
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
    extra_inputs: Vec<SubstateRequirement>,
) -> anyhow::Result<BoxedTransactionBuilder> {
    let contents = fs::read_to_string(manifest).unwrap();
    // Every substate referenced by the transaction must be declared as an input, otherwise the
    // executor never loads it and the instructions fail with SubstateNotFound. Substate-typed
    // globals (e.g. the fee account) are auto-declared; `extra_inputs` (from `--input`) cover
    // substates the manifest debits but never names, such as the fee vault. Versions are resolved
    // by the validator at execution time. Duplicates are de-duplicated by the input set.
    let inputs = globals
        .values()
        .filter_map(|value| value.as_address().cloned())
        .map(SubstateRequirement::unversioned)
        .chain(extra_inputs)
        .collect::<Vec<_>>();
    let instructions = tari_transaction_manifest::parse_manifest(&contents, globals, templates, Default::default())?;
    Ok(Box::new(move |_| {
        Transaction::builder(network.as_byte())
            .with_fee_instructions(instructions.fee_instructions.clone())
            .with_instructions(instructions.instructions.clone())
            .with_inputs(inputs.clone())
            .build_and_seal(&signer_secret_key)
    }))
}
