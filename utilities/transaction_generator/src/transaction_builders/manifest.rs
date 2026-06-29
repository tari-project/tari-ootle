//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, fs, path::Path};

use ootle_byte_type::ToByteType;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_ootle_common_types::SubstateRequirement;
use tari_ootle_transaction::{Blob, Network, Transaction};
use tari_template_lib_types::TemplateAddress;
use tari_transaction_manifest::ManifestValue;

use crate::BoxedTransactionBuilder;

#[allow(clippy::too_many_arguments)]
pub fn builder<P: AsRef<Path>>(
    signer_secret_key: RistrettoSecretKey,
    network: Network,
    manifest: P,
    globals: HashMap<String, ManifestValue>,
    templates: HashMap<String, TemplateAddress>,
    extra_inputs: Vec<SubstateRequirement>,
    blob_inputs: HashMap<String, Blob>,
    random_signer: bool,
) -> anyhow::Result<BoxedTransactionBuilder> {
    let contents = fs::read_to_string(manifest)?;
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
    let instructions = tari_transaction_manifest::parse_manifest(&contents, globals, templates, blob_inputs)?;
    let fee_instructions = instructions.fee_instructions;
    let main_instructions = instructions.instructions;
    // Blobs referenced by the manifest (e.g. the WASM binary for `publish_template!`), in the order
    // the generator assigned their `BlobIndex`es. They must be attached to every transaction in the
    // same order so the indices in `Instruction::PublishTemplate` resolve correctly.
    let blobs = instructions.blobs;

    Ok(Box::new(move |_| {
        let mut builder = Transaction::builder(network.as_byte())
            .with_fee_instructions(fee_instructions.clone())
            .with_instructions(main_instructions.clone());
        for (i, blob) in blobs.iter().enumerate() {
            builder = builder.add_blob(i.to_string(), blob.clone());
        }
        let builder = builder.with_inputs(inputs.clone());

        if random_signer {
            // Seal with a fresh random keypair per transaction and add `signer_secret_key` as an
            // additional signer, so its badge stays in the auth scope (e.g. to authorise pay_fee on
            // its account). For publish_template this also makes each publish unique: the engine
            // records the template's author from the transaction's main signer — the authorised seal
            // signer — so a fresh seal key gives a unique template address (H(author, binary_hash))
            // instead of colliding on a duplicate substate.
            let (random_secret, random_public) = RistrettoPublicKey::random_keypair(&mut rand::rng());
            builder
                .add_signer(&random_public.to_byte_type(), &signer_secret_key)
                .seal(&random_secret)
        } else {
            builder.build_and_seal(&signer_secret_key)
        }
    }))
}
