//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use borsh::{BorshSerialize, io};
use tari_template_lib::types::{Hash32, TemplateMetadataHash, crypto::RistrettoPublicKeyBytes};

use crate::{
    Epoch,
    ProtocolVersion,
    component::{Component, ComponentBody, ComponentHeader},
    confidential::ClaimedOutputTombstone,
    confidential_output::ConfidentialOutput,
    events::Event,
    fees::FeeReceipt,
    hashing::{EngineHashDomainLabel, hasher32},
    non_fungible::NonFungibleContainer,
    published_template::{PublishedTemplate, PublishedTemplateMetadata},
    resource::Resource,
    substate::SubstateValue,
    transaction_receipt::{DiffSummary, FinalizeOutcome, TransactionReceipt},
    utxo::Utxo,
    validator_fee::{ValidatorFeePool, ValidatorFeeWithdrawal},
    vault::Vault,
};

/// The substate value preimage, one variant per protocol version. The variant tag is part of the preimage, so a
/// value hashes differently under each version.
#[derive(Debug, Clone, Copy, borsh::BorshSerialize)]
pub enum SubstateHashMessage<'a> {
    V0(SubstateValueHashMessage<'a>),
    V1(SubstateValueCborHashMessage<'a>),
}

impl<'a> SubstateHashMessage<'a> {
    pub fn new(protocol_version: ProtocolVersion, value: &'a SubstateValue) -> Self {
        match protocol_version {
            ProtocolVersion::V0 => Self::V0(value.into()),
            ProtocolVersion::V1 => Self::V1(value.into()),
        }
    }

    /// The preimage of the template these metadata describe, identical to [`Self::new`] for the full template.
    pub fn template_metadata(protocol_version: ProtocolVersion, metadata: &'a PublishedTemplateMetadata) -> Self {
        match protocol_version {
            ProtocolVersion::V0 => Self::V0(SubstateValueHashMessage::Template(metadata.into())),
            ProtocolVersion::V1 => Self::V1(SubstateValueCborHashMessage::Template(Cbor(metadata.into()))),
        }
    }
}

/// The per-type preimage. Variant order is consensus-bound: a new variant goes at the end.
#[derive(Debug, Clone, Copy, borsh::BorshSerialize)]
pub enum SubstateValueHashMessage<'a> {
    Component(ComponentHashMessage<'a>),
    Resource(ResourceHashMessage<'a>),
    Vault(VaultHashMessage<'a>),
    NonFungible(NonFungibleContainerHashMessage<'a>),
    ClaimedOutputTombstone(ClaimedOutputTombstoneHashMessage<'a>),
    TransactionReceipt(TransactionReceiptHashMessage<'a>),
    Template(PublishedTemplateHashMessage<'a>),
    ValidatorFeePool(ValidatorFeePoolHashMessage<'a>),
    Utxo(UtxoHashMessage<'a>),
    ConfidentialOutput(ConfidentialOutputHashMessage<'a>),
}

impl<'a> From<&'a SubstateValue> for SubstateValueHashMessage<'a> {
    fn from(value: &'a SubstateValue) -> Self {
        match value {
            SubstateValue::Component(component) => Self::Component(component.into()),
            SubstateValue::Resource(resource) => Self::Resource(resource.as_ref().into()),
            SubstateValue::Vault(vault) => Self::Vault(vault.into()),
            SubstateValue::NonFungible(nf) => Self::NonFungible(nf.into()),
            SubstateValue::ClaimedOutputTombstone(tombstone) => Self::ClaimedOutputTombstone(tombstone.into()),
            SubstateValue::TransactionReceipt(receipt) => Self::TransactionReceipt(receipt.into()),
            SubstateValue::Template(template) => Self::Template(template.into()),
            SubstateValue::ValidatorFeePool(pool) => Self::ValidatorFeePool(pool.into()),
            SubstateValue::Utxo(utxo) => Self::Utxo(utxo.into()),
            SubstateValue::ConfidentialOutput(output) => Self::ConfidentialOutput(output.into()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ComponentHashMessage<'a>(pub &'a Component);

impl<'a> From<&'a Component> for ComponentHashMessage<'a> {
    fn from(component: &'a Component) -> Self {
        Self(component)
    }
}

impl borsh::BorshSerialize for ComponentHashMessage<'_> {
    fn serialize<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        BorshSerialize::serialize(&self.0.header, writer)?;
        // Split the body hash so that the body could be pruned
        let body_hash = hash(&self.0.body);
        BorshSerialize::serialize(&body_hash, writer)?;

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, borsh::BorshSerialize)]
pub struct ResourceHashMessage<'a>(pub &'a Resource);

impl<'a> From<&'a Resource> for ResourceHashMessage<'a> {
    fn from(resource: &'a Resource) -> Self {
        Self(resource)
    }
}

#[derive(Debug, Clone, Copy, borsh::BorshSerialize)]
pub struct VaultHashMessage<'a>(&'a Vault);

impl<'a> From<&'a Vault> for VaultHashMessage<'a> {
    fn from(vault: &'a Vault) -> Self {
        Self(vault)
    }
}

#[derive(Debug, Clone, Copy, borsh::BorshSerialize)]
pub struct NonFungibleContainerHashMessage<'a>(&'a NonFungibleContainer);

impl<'a> From<&'a NonFungibleContainer> for NonFungibleContainerHashMessage<'a> {
    fn from(non_fungible: &'a NonFungibleContainer) -> Self {
        Self(non_fungible)
    }
}

#[derive(Debug, Clone, Copy, borsh::BorshSerialize)]
pub struct ClaimedOutputTombstoneHashMessage<'a>(&'a ClaimedOutputTombstone);

impl<'a> From<&'a ClaimedOutputTombstone> for ClaimedOutputTombstoneHashMessage<'a> {
    fn from(tombstone: &'a ClaimedOutputTombstone) -> Self {
        Self(tombstone)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TransactionReceiptHashMessage<'a> {
    pub receipt: &'a TransactionReceipt,
}

impl<'a> From<&'a TransactionReceipt> for TransactionReceiptHashMessage<'a> {
    fn from(receipt: &'a TransactionReceipt) -> Self {
        Self { receipt }
    }
}

impl borsh::BorshSerialize for TransactionReceiptHashMessage<'_> {
    fn serialize<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        let receipt = self.receipt;
        BorshSerialize::serialize(&receipt.outcome, writer)?;
        BorshSerialize::serialize(&receipt.diff_summary, writer)?;
        BorshSerialize::serialize(&receipt.fee_withdrawals, writer)?;
        let events = hash(&receipt.events);
        BorshSerialize::serialize(&events, writer)?;
        BorshSerialize::serialize(&receipt.fee_receipt, writer)?;
        BorshSerialize::serialize(&receipt.epoch, writer)?;
        // Serialized in full: the commitment is already 32 bytes, so part-hashing it saves nothing.
        BorshSerialize::serialize(&receipt.intent_commitment, writer)?;
        Ok(())
    }
}

/// Commits to the binary by its `binary_hash`, so a template's value hash can be derived from its
/// [`PublishedTemplateMetadata`] alone.
#[derive(Debug, Clone, Copy, borsh::BorshSerialize)]
pub struct PublishedTemplateHashMessage<'a> {
    template_name: &'a str,
    at_epoch: u64,
    author: &'a RistrettoPublicKeyBytes,
    metadata_hash: &'a Option<TemplateMetadataHash>,
    binary_hash: Hash32,
}

impl<'a> From<&'a PublishedTemplate> for PublishedTemplateHashMessage<'a> {
    fn from(template: &'a PublishedTemplate) -> Self {
        Self {
            template_name: &template.template_name,
            at_epoch: template.at_epoch,
            author: &template.author,
            metadata_hash: &template.metadata_hash,
            binary_hash: template.to_binary_hash(),
        }
    }
}

impl<'a> From<&'a PublishedTemplateMetadata> for PublishedTemplateHashMessage<'a> {
    fn from(metadata: &'a PublishedTemplateMetadata) -> Self {
        Self {
            template_name: &metadata.template_name,
            at_epoch: metadata.at_epoch,
            author: &metadata.author_public_key,
            metadata_hash: &metadata.metadata_hash,
            binary_hash: metadata.binary_hash,
        }
    }
}

#[derive(Debug, Clone, Copy, borsh::BorshSerialize)]
pub struct ValidatorFeePoolHashMessage<'a>(&'a ValidatorFeePool);

impl<'a> From<&'a ValidatorFeePool> for ValidatorFeePoolHashMessage<'a> {
    fn from(pool: &'a ValidatorFeePool) -> Self {
        Self(pool)
    }
}

#[derive(Debug, Clone, Copy, borsh::BorshSerialize)]
pub struct UtxoHashMessage<'a>(&'a Utxo);

impl<'a> From<&'a Utxo> for UtxoHashMessage<'a> {
    fn from(utxo: &'a Utxo) -> Self {
        Self(utxo)
    }
}

#[derive(Debug, Clone, Copy, borsh::BorshSerialize)]
pub struct ConfidentialOutputHashMessage<'a>(&'a ConfidentialOutput);

impl<'a> From<&'a ConfidentialOutput> for ConfidentialOutputHashMessage<'a> {
    fn from(output: &'a ConfidentialOutput) -> Self {
        Self(output)
    }
}

/// The version 1 preimage. Each substate type commits to its CBOR encoding, the encoding it is stored and sent in,
/// with a part that may be pruned (a component's body, a receipt's events, a template's binary) replaced by the hash
/// of that part's own encoding.
///
/// Every node encodes the decoded value to build the preimage, so the hash is a function of the value and never of
/// the bytes a peer sent. A field appended to a hashed type as an `Option` encodes nothing while it is `None`, so a
/// value written before the field existed keeps its hash. Any other change to a hashed type's encoding re-hashes
/// committed state and needs a new protocol version.
///
/// Variant order is consensus-bound: a new variant goes at the end.
#[derive(Debug, Clone, Copy, borsh::BorshSerialize)]
pub enum SubstateValueCborHashMessage<'a> {
    Component {
        header: Cbor<&'a ComponentHeader>,
        body: CborHash<&'a ComponentBody>,
    },
    Resource(Cbor<&'a Resource>),
    Vault(Cbor<&'a Vault>),
    NonFungible(Cbor<&'a NonFungibleContainer>),
    ClaimedOutputTombstone(Cbor<&'a ClaimedOutputTombstone>),
    TransactionReceipt(Cbor<TransactionReceiptHashView<'a>>),
    Template(Cbor<PublishedTemplateHashView<'a>>),
    ValidatorFeePool(Cbor<&'a ValidatorFeePool>),
    Utxo(Cbor<&'a Utxo>),
    ConfidentialOutput(Cbor<&'a ConfidentialOutput>),
}

impl<'a> From<&'a SubstateValue> for SubstateValueCborHashMessage<'a> {
    fn from(value: &'a SubstateValue) -> Self {
        match value {
            SubstateValue::Component(Component { header, body }) => Self::Component {
                header: Cbor(header),
                body: CborHash(body),
            },
            SubstateValue::Resource(resource) => Self::Resource(Cbor(resource.as_ref())),
            SubstateValue::Vault(vault) => Self::Vault(Cbor(vault)),
            SubstateValue::NonFungible(nf) => Self::NonFungible(Cbor(nf)),
            SubstateValue::ClaimedOutputTombstone(tombstone) => Self::ClaimedOutputTombstone(Cbor(tombstone)),
            SubstateValue::TransactionReceipt(receipt) => Self::TransactionReceipt(Cbor(receipt.into())),
            SubstateValue::Template(template) => Self::Template(Cbor(template.into())),
            SubstateValue::ValidatorFeePool(pool) => Self::ValidatorFeePool(Cbor(pool)),
            SubstateValue::Utxo(utxo) => Self::Utxo(Cbor(utxo)),
            SubstateValue::ConfidentialOutput(output) => Self::ConfidentialOutput(Cbor(output)),
        }
    }
}

/// Borsh-serializes the CBOR encoding of `T` as a length-prefixed byte string.
#[derive(Debug, Clone, Copy)]
pub struct Cbor<T>(T);

impl<T: minicbor::Encode<()>> BorshSerialize for Cbor<T> {
    fn serialize<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        let bytes = tari_bor::encode(&self.0).map_err(io::Error::other)?;
        BorshSerialize::serialize(&bytes, writer)
    }
}

/// Borsh-serializes the hash of the CBOR encoding of `T`, for a part of a substate that may be pruned.
#[derive(Debug, Clone, Copy)]
pub struct CborHash<T>(T);

impl<T: minicbor::Encode<()>> CborHash<T> {
    fn hash(&self) -> io::Result<Hash32> {
        let bytes = tari_bor::encode(&self.0).map_err(io::Error::other)?;
        Ok(hasher32(EngineHashDomainLabel::SubstateValuePart)
            .chain(&bytes)
            .result())
    }
}

impl<T: minicbor::Encode<()>> BorshSerialize for CborHash<T> {
    fn serialize<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        BorshSerialize::serialize(&self.hash()?, writer)
    }
}

/// A [`TransactionReceipt`] encoded field for field, with its events replaced by their hash.
#[derive(Debug, Clone, Copy, minicbor::Encode)]
pub struct TransactionReceiptHashView<'a> {
    #[n(0)]
    outcome: &'a FinalizeOutcome,
    #[n(1)]
    diff_summary: &'a DiffSummary,
    #[n(2)]
    fee_withdrawals: &'a [ValidatorFeeWithdrawal],
    #[n(3)]
    events: CborHashField<'a, [Event]>,
    #[n(4)]
    fee_receipt: &'a FeeReceipt,
    #[n(5)]
    epoch: Epoch,
    #[n(6)]
    intent_commitment: &'a Hash32,
}

impl<'a> From<&'a TransactionReceipt> for TransactionReceiptHashView<'a> {
    fn from(receipt: &'a TransactionReceipt) -> Self {
        // Destructured in full so that a field added to the receipt does not compile until it is committed here.
        let TransactionReceipt {
            outcome,
            diff_summary,
            fee_withdrawals,
            events,
            fee_receipt,
            epoch,
            intent_commitment,
        } = receipt;
        Self {
            outcome,
            diff_summary,
            fee_withdrawals,
            events: CborHashField(events),
            fee_receipt,
            epoch: *epoch,
            intent_commitment,
        }
    }
}

/// Encodes as the hash of the CBOR encoding of the referenced value.
#[derive(Debug)]
pub struct CborHashField<'a, T: ?Sized>(&'a T);

impl<T: ?Sized> Clone for CborHashField<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: ?Sized> Copy for CborHashField<'_, T> {}

impl<C, T: minicbor::Encode<()> + ?Sized> minicbor::Encode<C> for CborHashField<'_, T> {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        let hash = CborHash(self.0)
            .hash()
            .map_err(|err| minicbor::encode::Error::message(err.to_string()))?;
        e.encode(hash)?;
        Ok(())
    }
}

/// A published template encoded as its [`PublishedTemplateMetadata`], which commits to the binary by its
/// `binary_hash`. The two encode identically, so a template's value hash can be derived from its metadata alone.
#[derive(Debug, Clone, Copy, minicbor::Encode)]
pub struct PublishedTemplateHashView<'a> {
    #[n(0)]
    template_name: &'a str,
    #[n(1)]
    author_public_key: &'a RistrettoPublicKeyBytes,
    #[n(2)]
    binary_hash: Hash32,
    #[n(3)]
    at_epoch: u64,
    #[n(4)]
    metadata_hash: Option<&'a TemplateMetadataHash>,
}

impl<'a> From<&'a PublishedTemplate> for PublishedTemplateHashView<'a> {
    fn from(template: &'a PublishedTemplate) -> Self {
        // Destructured in full so that a field added to the template does not compile until it is committed here.
        let PublishedTemplate {
            template_name,
            author,
            binary: _,
            at_epoch,
            metadata_hash,
        } = template;
        Self {
            template_name,
            author_public_key: author,
            binary_hash: template.to_binary_hash(),
            at_epoch: *at_epoch,
            metadata_hash: metadata_hash.as_ref(),
        }
    }
}

impl<'a> From<&'a PublishedTemplateMetadata> for PublishedTemplateHashView<'a> {
    fn from(metadata: &'a PublishedTemplateMetadata) -> Self {
        let PublishedTemplateMetadata {
            template_name,
            author_public_key,
            binary_hash,
            at_epoch,
            metadata_hash,
        } = metadata;
        Self {
            template_name,
            author_public_key,
            binary_hash: *binary_hash,
            at_epoch: *at_epoch,
            metadata_hash: metadata_hash.as_ref(),
        }
    }
}

fn hash<T: borsh::BorshSerialize>(value: &T) -> Hash32 {
    hasher32(EngineHashDomainLabel::SubstateValuePart)
        .chain(&value)
        .result()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ootle_network::Network;
    use tari_template_lib::types::{
        AccessRule,
        EncryptedData,
        EntityId,
        Metadata,
        ObjectKey,
        ResourceAddress,
        ResourceType,
        SubstateOwnerRule,
        TemplateAddress,
        access_rules::{ComponentAccessRules, ResourceAccessRules, UpdateRule},
        crypto::{RistrettoPublicKeyBytes, UtxoTag},
        stealth::SpendAuthorization,
    };

    use super::*;
    use crate::{
        Epoch,
        SubstateVersion,
        crypto::OutputBody,
        fees::{FeeBreakdown, FeeReceipt, FeeSource},
        published_template::{TemplateBlob, TemplateName},
        resource_container::ResourceContainer,
        substate::{hash_published_template_metadata, hash_substate},
        transaction_receipt::{DiffSummary, FinalizeOutcome},
        utxo::UtxoOutput,
    };

    /// The non-fungible data rule is spelled out so that the pinned hash moves only with the preimage format.
    fn resource(auth_hook_updater: UpdateRule) -> SubstateValue {
        SubstateValue::Resource(Box::new(Resource::new(
            ResourceType::Fungible,
            SubstateOwnerRule::None,
            ResourceAccessRules::new()
                .update_non_fungible_data(AccessRule::AllowAll, UpdateRule::Owner)
                .set_auth_hook_updater(auth_hook_updater),
            Metadata::from_iter([("name", "baseline")]),
            None,
            None,
            6,
            true,
        )))
    }

    fn receipt(exhaust_burn: u64) -> SubstateValue {
        let mut breakdown = FeeBreakdown::default();
        breakdown.add(FeeSource::Initial, 10);
        breakdown.add(FeeSource::Storage, 30);
        breakdown.add(FeeSource::WasmExecution, 20);
        let fee_receipt = FeeReceipt::builder()
            .with_total_fee_payment(1000)
            .with_total_fees_paid(60)
            .with_total_fee_overcharge(0)
            .with_cost_breakdown(breakdown)
            .with_exhaust_burn(exhaust_burn)
            .build();
        SubstateValue::TransactionReceipt(TransactionReceipt {
            outcome: FinalizeOutcome::Commit,
            diff_summary: DiffSummary {
                upped: Box::new([]),
                downed: Box::new([]),
            },
            fee_withdrawals: Box::new([]),
            events: Box::new([]),
            fee_receipt,
            epoch: Epoch(3),
            intent_commitment: Hash32::from_array([7u8; 32]),
        })
    }

    fn receipt_with_event(topic: &str) -> SubstateValue {
        let SubstateValue::TransactionReceipt(mut receipt) = receipt(123) else {
            unreachable!()
        };
        receipt.events = Box::new([Event::new(
            None,
            TemplateAddress::default(),
            topic.to_string(),
            Metadata::new(),
        )]);
        SubstateValue::TransactionReceipt(receipt)
    }

    fn component(upgrade_rule: UpdateRule, state: tari_bor::Value) -> SubstateValue {
        SubstateValue::Component(Component {
            header: ComponentHeader {
                template_address: TemplateAddress::default(),
                owner_rule: SubstateOwnerRule::None,
                access_rules: ComponentAccessRules::new(),
                entity_id: EntityId::default(),
                upgrade_rule,
            },
            body: ComponentBody { state },
        })
    }

    fn template(binary: &[u8]) -> PublishedTemplate {
        PublishedTemplate {
            template_name: TemplateName::try_from("baseline").unwrap(),
            author: RistrettoPublicKeyBytes::from_bytes(&[5u8; 32]).unwrap(),
            binary: TemplateBlob::new_checked(binary.to_vec()).unwrap(),
            at_epoch: 3,
            metadata_hash: Some(TemplateMetadataHash::try_from(vec![9u8; 32]).unwrap()),
        }
    }

    fn hash_at(version: ProtocolVersion, value: &SubstateValue) -> Hash32 {
        hasher32(EngineHashDomainLabel::SubstateValue)
            .chain(&SubstateHashMessage::new(version, value))
            .chain(&0u64)
            .chain(&Epoch(3))
            .result()
    }

    fn hex(hash: Hash32) -> String {
        hex::encode(hash.as_ref() as &[u8])
    }

    /// Pins the version 0 preimage for a receipt. Every substate value hash, and so every state root,
    /// is derived from it: a change to this hash is a change to all of them, and nodes carrying state
    /// hashed under the old preimage cannot re-derive their roots.
    #[test]
    fn version_0_receipt_hash_is_pinned() {
        assert_eq!(
            hex(hash_at(ProtocolVersion::V0, &receipt(123))),
            "3ed616d2ebdc09d0bf480368e2a0812b358f8c16bfad772c69baf565c77bbd89"
        );
        assert_eq!(
            hash_substate(Network::Esmeralda, &receipt(123), SubstateVersion::ZERO, Epoch(3)),
            hash_at(ProtocolVersion::V0, &receipt(123))
        );
    }

    /// Pins the version 0 preimage for a resource, as [`version_0_receipt_hash_is_pinned`] does for a
    /// receipt.
    #[test]
    fn version_0_resource_hash_is_pinned() {
        assert_eq!(
            hex(hash_at(ProtocolVersion::V0, &resource(UpdateRule::Locked))),
            "30d55dd7f25674254559dbcb8af6130df44501f1c3b76f790e72d23aa0a9e840"
        );
    }

    /// Pins the version 0 preimage for a template, as [`version_0_receipt_hash_is_pinned`] does for a
    /// receipt.
    #[test]
    fn version_0_template_hash_is_pinned() {
        assert_eq!(
            hex(hash_at(
                ProtocolVersion::V0,
                &SubstateValue::Template(template(b"wasm"))
            )),
            "fcfa3804d28d15458bbb90fc39ce19ff090ebc8941fb30c9b626a7588191a4a9"
        );
    }

    /// The indexer derives template value hashes from streamed metadata. LocalNet is on V0 at epoch 3 and on V1 at
    /// epoch 11, so both versions are covered.
    #[test]
    fn template_metadata_hashes_like_the_template() {
        let template = template(b"wasm");
        let metadata = template.clone().into_template_metadata();
        for (version, epoch) in [(SubstateVersion::ZERO, Epoch(3)), (SubstateVersion::new(7), Epoch(11))] {
            assert_eq!(
                hash_published_template_metadata(Network::LocalNet, &metadata, version, epoch),
                hash_substate(
                    Network::LocalNet,
                    &SubstateValue::Template(template.clone()),
                    version,
                    epoch
                )
            );
        }
        assert_ne!(
            ProtocolVersion::at(Network::LocalNet, Epoch(3)),
            ProtocolVersion::at(Network::LocalNet, Epoch(11))
        );
    }

    /// Pins the version 1 preimage for a receipt, as [`version_0_receipt_hash_is_pinned`] does for version 0.
    #[test]
    fn version_1_receipt_hash_is_pinned() {
        assert_eq!(
            hex(hash_at(ProtocolVersion::V1, &receipt(123))),
            "bf90e85dc11a478f88f0bfd49371222c39b41f74d64bdd394cf297461049669a"
        );
        assert_eq!(
            hash_substate(Network::MainNet, &receipt(123), SubstateVersion::ZERO, Epoch(3)),
            hash_at(ProtocolVersion::V1, &receipt(123))
        );
    }

    #[test]
    fn version_1_resource_hash_is_pinned() {
        assert_eq!(
            hex(hash_at(ProtocolVersion::V1, &resource(UpdateRule::Locked))),
            "ce45f48e2955eaa58f0c59a85e0cd14c278c203ff7dbf1a0db4e484e1636f4c7"
        );
    }

    #[test]
    fn version_1_template_hash_is_pinned() {
        assert_eq!(
            hex(hash_at(
                ProtocolVersion::V1,
                &SubstateValue::Template(template(b"wasm"))
            )),
            "6977ceed9250317fa84901b0ec9b8a4424de6b3760fcc62b2e621b9648693d54"
        );
    }

    #[test]
    fn version_1_component_hash_is_pinned() {
        assert_eq!(
            hex(hash_at(
                ProtocolVersion::V1,
                &component(UpdateRule::Locked, tari_bor::Value::Integer(1.into()))
            )),
            "4dd94a716a3beff436fdae64e1cb5d41707c0e4391c67f7d00182a2b8bef515e"
        );
    }

    #[test]
    fn each_version_hashes_a_value_differently() {
        for value in [
            receipt(123),
            resource(UpdateRule::Locked),
            SubstateValue::Template(template(b"wasm")),
            component(UpdateRule::Locked, tari_bor::Value::Null),
        ] {
            assert_ne!(
                hash_at(ProtocolVersion::V0, &value),
                hash_at(ProtocolVersion::V1, &value),
                "{value:?}"
            );
        }
    }

    /// The template preimage is the encoding of its metadata, which is what lets the indexer hash a template it holds
    /// only the metadata of.
    #[test]
    fn version_1_template_view_encodes_as_its_metadata() {
        for metadata_hash in [None, Some(TemplateMetadataHash::try_from(vec![9u8; 32]).unwrap())] {
            let template = PublishedTemplate {
                metadata_hash,
                ..template(b"wasm")
            };
            let metadata = template.clone().into_template_metadata();
            assert_eq!(
                tari_bor::encode(&PublishedTemplateHashView::from(&template)).unwrap(),
                tari_bor::encode(&metadata).unwrap()
            );
        }
    }

    #[test]
    fn version_1_covers_template_binary() {
        assert_ne!(
            hash_at(ProtocolVersion::V1, &SubstateValue::Template(template(b"wasm"))),
            hash_at(ProtocolVersion::V1, &SubstateValue::Template(template(b"wasn")))
        );
    }

    #[test]
    fn version_1_covers_auth_hook_updater() {
        assert_ne!(
            hash_at(ProtocolVersion::V1, &resource(UpdateRule::Locked)),
            hash_at(ProtocolVersion::V1, &resource(UpdateRule::Owner))
        );
    }

    #[test]
    fn version_1_covers_exhaust_burn() {
        assert_ne!(
            hash_at(ProtocolVersion::V1, &receipt(0)),
            hash_at(ProtocolVersion::V1, &receipt(123))
        );
    }

    #[test]
    fn version_1_covers_receipt_events() {
        assert_ne!(
            hash_at(ProtocolVersion::V1, &receipt(123)),
            hash_at(ProtocolVersion::V1, &receipt_with_event("a"))
        );
        assert_ne!(
            hash_at(ProtocolVersion::V1, &receipt_with_event("a")),
            hash_at(ProtocolVersion::V1, &receipt_with_event("b"))
        );
    }

    #[test]
    fn version_1_covers_component_header_and_body() {
        let base = hash_at(
            ProtocolVersion::V1,
            &component(UpdateRule::Locked, tari_bor::Value::Null),
        );
        assert_ne!(
            base,
            hash_at(
                ProtocolVersion::V1,
                &component(UpdateRule::Owner, tari_bor::Value::Null)
            )
        );
        assert_ne!(
            base,
            hash_at(
                ProtocolVersion::V1,
                &component(UpdateRule::Locked, tari_bor::Value::Bool(true))
            )
        );
    }

    /// The property [`SubstateValueCborHashMessage`] relies on to let a substate type gain a field without a new
    /// protocol version: an `Option` field appended at the next index encodes nothing while it is `None`.
    #[test]
    fn an_appended_none_field_leaves_the_preimage_unchanged() {
        #[derive(minicbor::Encode)]
        struct Before {
            #[n(0)]
            amount: u64,
            #[n(1)]
            data: Vec<u8>,
        }
        #[derive(minicbor::Encode)]
        struct After {
            #[n(0)]
            amount: u64,
            #[n(1)]
            data: Vec<u8>,
            #[n(2)]
            added: Option<u64>,
        }

        let before = borsh::to_vec(&Cbor(&Before {
            amount: 1,
            data: vec![2],
        }))
        .unwrap();
        let after = |added| {
            borsh::to_vec(&Cbor(&After {
                amount: 1,
                data: vec![2],
                added,
            }))
            .unwrap()
        };
        assert_eq!(before, after(None));
        assert_ne!(before, after(Some(3)));
    }

    #[test]
    fn version_0_covers_template_binary() {
        assert_ne!(
            hash_at(ProtocolVersion::V0, &SubstateValue::Template(template(b"wasm"))),
            hash_at(ProtocolVersion::V0, &SubstateValue::Template(template(b"wasn")))
        );
    }

    #[test]
    fn version_0_covers_auth_hook_updater() {
        assert_ne!(
            hash_at(ProtocolVersion::V0, &resource(UpdateRule::Locked)),
            hash_at(ProtocolVersion::V0, &resource(UpdateRule::Owner))
        );
    }

    #[test]
    fn version_0_covers_exhaust_burn() {
        assert_ne!(
            hash_at(ProtocolVersion::V0, &receipt(0)),
            hash_at(ProtocolVersion::V0, &receipt(123))
        );
    }

    /// Pins the borsh tag of every cheaply constructed variant, which pins the rest transitively: a
    /// variant inserted anywhere but after `Utxo` shifts one of these tags. `ConfidentialOutput` is
    /// last, where an append is the only move and is safe. The two leading bytes are the
    /// `SubstateHashMessage` version tag and the value tag.
    /// One value per type that the other version 1 pins leave out, so that a change to any type's encoding fails here.
    #[test]
    fn version_1_hash_of_every_other_type_is_pinned() {
        let output = OutputBody {
            public_nonce: RistrettoPublicKeyBytes::from_bytes(&[2u8; 32]).unwrap(),
            encrypted_data: EncryptedData::empty(),
            minimum_value_promise: 11,
            viewable_balance: None,
        };
        let values = [
            (
                SubstateValue::Vault(Vault::new(ResourceContainer::non_fungible(
                    ResourceAddress::new(ObjectKey::from_array([1u8; ObjectKey::LENGTH])),
                    BTreeSet::new(),
                ))),
                "6c971b3745518e4f168b1713d13b543a518fea0dba826fb4986890ffbed3aa27",
            ),
            (
                SubstateValue::NonFungible(NonFungibleContainer::new(
                    tari_bor::Value::Integer(1.into()),
                    tari_bor::Value::Null,
                )),
                "73d230102f65ad78d3c9ef94cda1ce74f1dfe1ff1d3f71e3ad4993daafc226ca",
            ),
            (
                SubstateValue::ClaimedOutputTombstone(ClaimedOutputTombstone { value: 1 }),
                "3f936da5906c8894dde118ba72ce1ce6fac41192a4f03f491aa6962e8bfcdd3d",
            ),
            (
                SubstateValue::ValidatorFeePool(ValidatorFeePool::new(RistrettoPublicKeyBytes::default(), 5)),
                "abf9f17fec650232238efef74b8a6c2416b62f1cd89999e32c658d9212d26701",
            ),
            (
                SubstateValue::Utxo(Utxo::new(UtxoOutput {
                    output: output.clone(),
                    auth: SpendAuthorization::Key(RistrettoPublicKeyBytes::from_bytes(&[3u8; 32]).unwrap()),
                    tag: UtxoTag::new(4),
                })),
                "fbfd477a67bf6983ee391e98b7b15584b594090628ef34ebd79d18ebefc1aec0",
            ),
            (
                SubstateValue::ConfidentialOutput(ConfidentialOutput::new(output)),
                "21334cfbe386757cbd370e05b1431831e204dba322017a4fcc75e15aef245a06",
            ),
        ];
        for (value, expected) in &values {
            assert_eq!(hex(hash_at(ProtocolVersion::V1, value)), *expected, "{value:?}");
        }
    }

    #[test]
    fn value_tags_are_stable() {
        let values: Vec<(u8, SubstateValue)> = vec![
            (
                2,
                SubstateValue::Vault(Vault::new(ResourceContainer::non_fungible(
                    ResourceAddress::new(ObjectKey::from_array([1u8; ObjectKey::LENGTH])),
                    BTreeSet::new(),
                ))),
            ),
            (
                3,
                SubstateValue::NonFungible(NonFungibleContainer::new(tari_bor::Value::Null, tari_bor::Value::Null)),
            ),
            (
                4,
                SubstateValue::ClaimedOutputTombstone(ClaimedOutputTombstone { value: 1 }),
            ),
            (1, resource(UpdateRule::Locked)),
            (5, receipt(0)),
            (
                7,
                SubstateValue::ValidatorFeePool(ValidatorFeePool::new(RistrettoPublicKeyBytes::default(), 5)),
            ),
            (
                8,
                SubstateValue::Utxo(Utxo {
                    output: None,
                    is_frozen: false,
                }),
            ),
        ];
        for (tag, value) in &values {
            let v0 = borsh::to_vec(&SubstateHashMessage::new(ProtocolVersion::V0, value)).unwrap();
            assert_eq!(&v0[..2], &[0, *tag], "V0 tag for {value:?}");
            let v1 = borsh::to_vec(&SubstateHashMessage::new(ProtocolVersion::V1, value)).unwrap();
            assert_eq!(&v1[..2], &[1, *tag], "V1 tag for {value:?}");
        }
    }
}
