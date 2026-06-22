//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Lowers a [`PublicTransferIntent`] into an [`UnsignedTransaction`].
//!
//! Emits a fixed instruction sequence for a public transfer and attaches the caller-supplied input set
//! explicitly (no auto-fill / want-list resolution):
//!
//! ```text
//! pay_fee_from_component(from_component, fee)
//!   .call_method(from_component, "withdraw", args![resource, amount])
//!   .put_last_instruction_output_on_workspace(bucket_name)  // unique per transfer
//!   .create_account_with_bucket(recipient_pk, bucket_name); // idempotent create-or-deposit
//! ```
//!
//! ## Bucket-label convention (load-bearing)
//!
//! `put_last_instruction_output_on_workspace` maps the label *string* to a numeric workspace id; the
//! encoded instruction carries the numeric `key`, not the string. The encoded bytes therefore depend
//! on the id sequence, not the literal label. The label
//! `format!("__AccountInvokeBuilder_{}", builder.next_workspace_id())` is used so the encoding stays
//! stable across releases.

use tari_ootle_common_types::Epoch;
use tari_ootle_transaction::{TransactionBuilder, UnsignedTransaction, args};
use tari_template_lib_types::{ComponentAddress, ResourceAddress, crypto::RistrettoPublicKeyBytes};

use crate::types::{
    error::OotleSdkError,
    intent::{PublicTransferIntent, TransferRecipient},
    network::Network,
};

/// The per-transfer workspace-bucket label prefix. Fixed so the encoded transaction bytes stay stable
/// across releases.
const BUCKET_LABEL_PREFIX: &str = "__AccountInvokeBuilder_";

/// Builds the per-transfer workspace-bucket label for this transaction.
pub(crate) fn bucket_label(builder: &TransactionBuilder) -> String {
    format!("{}{}", BUCKET_LABEL_PREFIX, builder.next_workspace_id())
}

/// The internal values a public transfer is built from, resolved from the boundary
/// [`PublicTransferIntent`].
///
/// Used by both the explicit-input and want-list resolution paths.
pub(crate) struct TransferRecipe {
    pub from_component: ComponentAddress,
    pub resource: ResourceAddress,
    pub recipient_pk: RistrettoPublicKeyBytes,
    pub amount: tari_template_lib_types::Amount,
    pub fee: tari_template_lib_types::Amount,
}

/// Resolves the boundary intent into the internal recipe values (fail fast, no panics).
///
/// Rejects the `Account` recipient variant (see [`build_public_transfer_unsigned`]).
pub(crate) fn resolve_transfer_recipe(intent: &PublicTransferIntent) -> Result<TransferRecipe, OotleSdkError> {
    let from_component = intent.from_account.to_internal()?;
    let resource = intent.resource_address.to_internal()?;
    let amount = intent.amount.to_internal();
    let fee = intent.fee.to_internal();

    let recipient_pk = match &intent.recipient {
        TransferRecipient::PublicKey(pk) => pk.to_internal(),
        // Deposit into an existing account component is not supported; the supported path is
        // create-or-deposit keyed on the recipient's public key. Reject rather than emit an
        // unsupported instruction sequence.
        TransferRecipient::Account(_) => {
            return Err(OotleSdkError::Invalid(
                "recipient-by-account-component is not supported; supply the recipient public key".to_string(),
            ));
        },
    };

    Ok(TransferRecipe {
        from_component,
        resource,
        recipient_pk,
        amount,
        fee,
    })
}

/// Emits the public-transfer instruction sequence (fee → withdraw → put-on-workspace →
/// create-account-with-bucket) onto a fresh builder, without inputs or epoch/dry-run metadata.
///
/// Both the explicit-input and want-list resolution paths build on this single recipe so the
/// instruction sequence and bucket label stay identical between them.
pub(crate) fn build_transfer_recipe_builder(network: Network, recipe: &TransferRecipe) -> TransactionBuilder {
    let mut builder = TransactionBuilder::new(network.as_byte());

    // Fee first.
    builder = builder.pay_fee_from_component(recipe.from_component, recipe.fee);

    // Transfer: withdraw → put-on-workspace → create-account-with-bucket.
    let bucket = bucket_label(&builder);
    builder
        .call_method(recipe.from_component, "withdraw", args![recipe.resource, recipe.amount])
        .put_last_instruction_output_on_workspace(&bucket)
        .create_account_with_bucket(recipe.recipient_pk, bucket)
}

/// Threads the intent's epoch window and dry-run flag onto a built recipe.
pub(crate) fn apply_epoch_and_dry_run(
    builder: TransactionBuilder,
    intent: &PublicTransferIntent,
) -> TransactionBuilder {
    builder
        .with_min_epoch(intent.min_epoch.map(Epoch))
        .with_max_epoch(intent.max_epoch.map(Epoch))
        .with_dry_run(intent.dry_run)
}

/// Translates a public-transfer [`PublicTransferIntent`] (plus the boundary [`Network`]) into an
/// [`UnsignedTransaction`], emitting the public-transfer instruction sequence and adding the
/// caller-supplied explicit inputs.
///
/// Errors map to [`OotleSdkError`]; nothing panics on bad input.
pub fn build_public_transfer_unsigned(
    network: Network,
    intent: &PublicTransferIntent,
) -> Result<UnsignedTransaction, OotleSdkError> {
    let recipe = resolve_transfer_recipe(intent)?;
    let mut builder = build_transfer_recipe_builder(network, &recipe);

    // Attach the caller-supplied explicit inputs.
    let inputs = intent.inputs_to_internal()?;
    builder = builder.with_inputs(inputs);

    builder = apply_epoch_and_dry_run(builder, intent);

    Ok(builder.build_unsigned())
}

#[cfg(test)]
mod tests {
    use tari_template_lib_types::{ComponentAddress, ObjectKey, ResourceAddress};

    use super::*;
    use crate::types::{
        address::{ComponentAddressStr, ResourceAddressStr},
        bytes::PublicKeyBytes,
        intent::InputRef,
        numeric::BoundaryAmount,
    };

    fn component_str() -> String {
        ComponentAddress::new(ObjectKey::from_array([0xaa; ObjectKey::LENGTH])).to_string()
    }

    fn resource_str() -> String {
        ResourceAddress::new(ObjectKey::from_array([0xbb; ObjectKey::LENGTH])).to_string()
    }

    fn sample_intent() -> PublicTransferIntent {
        PublicTransferIntent {
            from_account: ComponentAddressStr::parse(component_str()).unwrap(),
            recipient: TransferRecipient::PublicKey(PublicKeyBytes::from_array([2u8; 32])),
            resource_address: ResourceAddressStr::parse(resource_str()).unwrap(),
            amount: BoundaryAmount::new(1_000_000),
            fee: BoundaryAmount::new(2000),
            inputs: vec![InputRef::versioned(component_str(), 0)],
            min_epoch: Some(5),
            max_epoch: Some(99),
            dry_run: false,
        }
    }

    #[test]
    fn builds_the_verified_recipe() {
        let unsigned = build_public_transfer_unsigned(Network::Esmeralda, &sample_intent()).unwrap();

        // Fee instruction: pay_fee on the from-component.
        assert_eq!(unsigned.fee_instructions().len(), 1);

        // Main instructions: withdraw, put-on-workspace, create-account-with-bucket.
        assert_eq!(unsigned.instructions().len(), 3);

        // Epochs + dry-run threaded through.
        assert_eq!(unsigned.min_epoch(), Some(Epoch(5)));
        assert_eq!(unsigned.max_epoch(), Some(Epoch(99)));

        // The explicit input was added.
        assert!(unsigned.inputs().iter().any(|i| i.version() == Some(0)));
    }

    #[test]
    fn is_deterministic_across_calls() {
        let a = build_public_transfer_unsigned(Network::Esmeralda, &sample_intent()).unwrap();
        let b = build_public_transfer_unsigned(Network::Esmeralda, &sample_intent()).unwrap();
        // The unsigned form carries no signatures/nonces, so it is already byte-stable.
        let ea = tari_bor::encode(&a).unwrap();
        let eb = tari_bor::encode(&b).unwrap();
        assert_eq!(ea, eb);
    }

    #[test]
    fn account_recipient_is_rejected_for_now() {
        let mut intent = sample_intent();
        intent.recipient = TransferRecipient::Account(ComponentAddressStr::parse(component_str()).unwrap());
        let err = build_public_transfer_unsigned(Network::Esmeralda, &intent).unwrap_err();
        assert_eq!(err.code(), "INVALID");
    }

    #[test]
    fn bad_address_is_a_parse_error() {
        let mut intent = sample_intent();
        // Inject a malformed from-account by constructing the boundary type directly.
        intent.from_account = ComponentAddressStr("not-a-component".to_string());
        let err = build_public_transfer_unsigned(Network::Esmeralda, &intent).unwrap_err();
        assert_eq!(err.code(), "PARSE");
    }
}
