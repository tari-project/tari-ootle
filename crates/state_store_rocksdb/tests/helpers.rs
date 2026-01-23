//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{io::Write, ops::Deref};

use rand::{rngs::OsRng, Rng, RngCore};
use tari_bor::cbor;
use tari_common_types::types::FixedHash;
use tari_consensus_types::{BlockId, Decision, LeafBlock, PcId, ProposalCertificate, ShardGroupAccumulatedData};
use tari_engine_types::{
    component::{ComponentBody, ComponentHeader},
    substate::{hash_substate, SubstateId, SubstateValue},
};
use tari_ootle_common_types::{
    Epoch,
    ExtraData,
    Network,
    NodeHeight,
    NumPreshards,
    ShardGroup,
    VersionedSubstateId,
    VersionedSubstateIdRef,
};
use tari_ootle_storage::{
    consensus_models::{
        Block,
        BlockPledge,
        BookkeepingModel,
        Command,
        CommandsCommitProof,
        ForeignProposal,
        ForeignProposalRecord,
        SubstateCreated,
        SubstateRecord,
        SubstateUpdateBatch,
        TransactionAtom,
    },
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};
use tari_ootle_transaction::TransactionId;
use tari_sidechain::{CommitProofElement, QuorumDecision, SidechainBlockCommitProof, SidechainBlockHeader};
use tari_state_store_rocksdb::{DatabaseOptions, RocksDbStateStore};
use tari_state_tree::Version;
use tari_template_lib::types::{
    access_rules::ComponentAccessRules,
    crypto::SchnorrSignatureBytes,
    ComponentAddress,
    ComponentKey,
    EntityId,
    ObjectKey,
    OwnerRule,
    TemplateAddress,
};
use tari_utilities::epoch_time::EpochTime;
use tempfile::TempDir;

pub const fn num_preshards() -> NumPreshards {
    NumPreshards::P256
}

/// Create a RocksDbStateStore and a temporary directory
/// NOTE: this takes around 1.5 s on my machine (AMD Ryzen 9 5950X, SSD)
pub fn create_rocksdb() -> (RocksDbStateStore<String>, TempDir) {
    let temp_dir = tempfile::Builder::new().disable_cleanup(false).tempdir().unwrap();
    let db_file = temp_dir.path().join("rocksdb");
    (
        RocksDbStateStore::open(db_file, DatabaseOptions::default()).unwrap(),
        temp_dir,
    )
}

pub fn create_tx_atom() -> TransactionAtom {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    TransactionAtom {
        id: TransactionId::new(bytes),
        decision: Decision::Commit,
        evidence: Default::default(),
        transaction_fee: 0,
        leader_fee: None,
    }
}

pub fn create_random_substate_id() -> SubstateId {
    let entity_id = EntityId::from_array(random_fixed());
    let component_key = ComponentKey::new(random_fixed());
    SubstateId::Component(ComponentAddress::new(ObjectKey::new(entity_id, component_key)))
}

pub fn random_fixed<const SIZE: usize>() -> [u8; SIZE] {
    let mut bytes = [0u8; SIZE];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

pub fn random_bytes(size: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; size];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

pub fn transaction_id_from_seed(seed: u32) -> TransactionId {
    let mut buf = [0u8; TransactionId::byte_size()];
    let mut writer = &mut buf[..];
    let be_bytes = seed.to_be_bytes();
    (0..32 / 4).for_each(|_| writer.write_all(&be_bytes).unwrap());
    TransactionId::new(buf)
}

pub fn build_substate_record(substate_id: &SubstateId, version: u32, state_version: Version) -> SubstateRecord {
    let entity_id = substate_id.to_object_key().as_entity_id();
    let value = build_substate_value(Some(entity_id));
    SubstateRecord {
        substate_id: substate_id.clone(),
        version,
        state_hash: hash_substate(&value, version),
        substate_value: Some(value),
        created: SubstateCreated {
            at_epoch: Epoch::zero(),
            in_shard: VersionedSubstateIdRef::new(substate_id, version).to_shard(num_preshards()),
            at_state_version: state_version,
        },
        destroyed: None,
    }
}

pub fn build_substate_value(entity_id: Option<EntityId>) -> SubstateValue {
    let bytes = random_bytes(100);
    let entity_id = entity_id.unwrap_or_else(|| EntityId::from_array(random_fixed()));
    SubstateValue::Component(ComponentHeader {
        template_address: TemplateAddress::default(),
        module_name: "foo".to_string(),
        owner_key: None,
        owner_rule: OwnerRule::None,
        access_rules: ComponentAccessRules::allow_all(),
        entity_id,
        body: ComponentBody {
            state: cbor!({
                "foo" => "bar",
                "bytes" => bytes,
            })
            .unwrap(),
        },
    })
}

pub fn create_substate_update_batch<'a, I: IntoIterator<Item = &'a SubstateRecord>>(
    epoch: Epoch,
    substates: I,
) -> SubstateUpdateBatch {
    let mut batch = SubstateUpdateBatch::new(epoch);
    for substate in substates {
        if let Some(destroyed) = &substate.destroyed {
            batch
                .with_transition(
                    substate.to_versioned_substate_id().to_shard(num_preshards()),
                    destroyed.at_state_version,
                )
                .push(tari_ootle_storage::consensus_models::SubstateTransition::Down {
                    id: VersionedSubstateId::new(substate.substate_id.clone(), substate.version),
                });
        } else {
            batch
                .with_transition(
                    substate.to_versioned_substate_id().to_shard(num_preshards()),
                    substate.created().at_state_version,
                )
                .push(tari_ootle_storage::consensus_models::SubstateTransition::Up {
                    id: substate.substate_id.clone(),
                    version: substate.version,
                    substate_or_hash: substate.clone().into_substate_value_or_hash(),
                });
        }
    }
    batch
}

pub fn substate_id_tx_seed(transaction_id: TransactionId, seed: u32) -> SubstateId {
    let mut buf = [0u8; EntityId::LENGTH];
    buf[..].copy_from_slice(&transaction_id.as_hash().as_slice()[..EntityId::LENGTH]);
    let entity_id = EntityId::from_array(buf);
    let mut buf = [0u8; ComponentKey::LENGTH];
    buf[..].copy_from_slice(&transaction_id.as_hash().as_slice()[..ComponentKey::LENGTH]);
    let len = buf.len();
    buf[len - size_of::<u32>()..].copy_from_slice(&seed.to_be_bytes());
    let component_key = ComponentKey::new(buf);
    SubstateId::Component(ComponentAddress::new(ObjectKey::new(entity_id, component_key)))
}

pub fn substate_id_seed(seed: u32) -> SubstateId {
    let mut buf = [0u8; EntityId::LENGTH];
    let end = size_of::<u32>().min(EntityId::LENGTH);
    buf[..end].copy_from_slice(&seed.to_be_bytes()[..end]);
    let entity_id = EntityId::from_array(buf);
    let mut buf = [0u8; ComponentKey::LENGTH];
    let end = size_of::<u32>().min(ComponentKey::LENGTH);
    buf[..end].copy_from_slice(&seed.to_be_bytes()[..end]);
    let component_key = ComponentKey::new(buf);
    SubstateId::Component(ComponentAddress::new(ObjectKey::new(entity_id, component_key)))
}

pub fn substate_value_for_entity(entity_id: EntityId) -> SubstateValue {
    SubstateValue::Component(ComponentHeader {
        template_address: TemplateAddress::default(),
        module_name: "foo".to_string(),
        owner_key: None,
        owner_rule: OwnerRule::None,
        access_rules: ComponentAccessRules::allow_all(),
        entity_id,
        body: ComponentBody {
            state: cbor!({
                "baz" => "bar",
                "bytes" => entity_id.as_bytes(),
            })
            .unwrap(),
        },
    })
}

pub fn gen_substates(
    epoch: Epoch,
    state_version: Version,
    range: impl IntoIterator<Item = u32>,
    version: u32,
) -> impl Iterator<Item = SubstateRecord> {
    range.into_iter().map(move |i| {
        let substate_id = substate_id_seed(i);
        let value = substate_value_for_entity(substate_id.to_object_key().as_entity_id());
        let shard = VersionedSubstateIdRef::new(&substate_id, version).to_shard(num_preshards());
        SubstateRecord::new(substate_id, version, value, SubstateCreated {
            at_epoch: epoch,
            in_shard: shard,
            at_state_version: state_version,
        })
    })
}

// track_caller allows a panic to include the caller's location in the error message
#[track_caller]
pub fn assert_eq_debug<T>(a: &T, b: &T)
where T: std::fmt::Debug {
    assert_eq!(format!("{:?}", a), format!("{:?}", b));
}

pub fn create_random_block_id() -> BlockId {
    BlockId::new(create_random_hash())
}

pub fn create_random_hash() -> FixedHash {
    let rand_bytes = OsRng.gen::<[u8; FixedHash::byte_size()]>();
    FixedHash::new(rand_bytes)
}

pub fn create_block(parent: Option<&Block>) -> Block {
    let network = Network::LocalNet;

    let Some(parent) = parent else {
        return Block::zero_block(network, num_preshards());
    };

    let atom1 = create_tx_atom();

    // This prevents all blocks to have the same hash/id
    let random_merkle_root = create_random_hash();
    let shard_group = ShardGroup::all_shards(num_preshards());

    Block::create(
        network,
        *parent.id(),
        parent.justify().clone(),
        None,
        NodeHeight(1),
        Epoch(0),
        shard_group,
        Default::default(),
        // Need to have a command in, otherwise this block will not be included internally in the query because it
        // cannot cause a state change without any commands
        [Command::LocalPrepare(atom1.clone())].into_iter().collect(),
        random_merkle_root,
        Default::default(),
        SchnorrSignatureBytes::zero(),
        EpochTime::now().as_u64(),
        FixedHash::zero(),
        ShardGroupAccumulatedData::default(),
        ExtraData::default(),
    )
    .unwrap()
}

pub fn create_block_with_qc(parent: &LeafBlock) -> Block {
    let network = Network::LocalNet;

    let atom1 = create_tx_atom();

    // This prevents all blocks to have the same hash/id
    let random_merkle_root = create_random_hash();

    let qc = create_qc(parent);
    let shard_group = parent.shard_group();

    Block::create(
        network,
        *parent.block_id(),
        qc,
        None,
        parent.height() + NodeHeight(1),
        parent.epoch(),
        shard_group,
        Default::default(),
        // Need to have a command in, otherwise this block will not be included internally in the query because it
        // cannot cause a state change without any commands
        [Command::LocalPrepare(atom1.clone())].into_iter().collect(),
        random_merkle_root,
        Default::default(),
        SchnorrSignatureBytes::zero(),
        EpochTime::now().as_u64(),
        FixedHash::zero(),
        ShardGroupAccumulatedData::default(),
        ExtraData::default(),
    )
    .unwrap()
}
pub fn create_qc(block: &LeafBlock) -> ProposalCertificate {
    ProposalCertificate::new(
        *block.block_id().hash(),
        *block.block_id(),
        block.height(),
        block.epoch(),
        ShardGroup::all_shards(num_preshards()),
        vec![],
        QuorumDecision::Accept,
    )
}

pub fn create_chain(num_blocks: usize) -> Vec<Block> {
    let mut blocks = Vec::with_capacity(num_blocks);
    let block = create_block(None);
    let mut parent = block.as_leaf();
    blocks.push(block);
    for _ in 0..num_blocks {
        let block = create_block_with_qc(&parent);
        parent = block.as_leaf();
        blocks.push(block);
    }
    blocks
}

pub fn commit_chain<TTx>(tx: &mut TTx, chain: &[Block])
where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
{
    for block in chain {
        block.insert(tx).unwrap();
        tx.proposal_certificates_save(block.justify()).unwrap();
    }
    let len = chain.len();
    if len < 3 {
        return;
    }

    chain[len - 3].as_locked().set(tx).unwrap();

    for block in &chain[..len - 3] {
        tx.blocks_set_qcs(block.id(), Some(&PcId::zero()), Some(&PcId::zero()))
            .unwrap();
    }

    chain.last().unwrap().as_leaf().set(tx).unwrap();
}

pub fn create_foreign_proposal(parent_id: BlockId, epoch: Epoch) -> ForeignProposalRecord {
    let shard_group = ShardGroup::all_shards(num_preshards());
    let qc1 = ProposalCertificate::new(
        *parent_id.hash(),
        parent_id,
        NodeHeight(1),
        epoch,
        shard_group,
        vec![],
        QuorumDecision::Accept,
    );

    let foreign_block = Block::create(
        Network::LocalNet,
        parent_id,
        qc1.clone(),
        None,
        NodeHeight(2),
        epoch,
        shard_group,
        Default::default(),
        Default::default(),
        Default::default(),
        1,
        SchnorrSignatureBytes::zero(),
        EpochTime::now().as_u64(),
        FixedHash::zero(),
        ShardGroupAccumulatedData::default(),
        ExtraData::new(),
    )
    .unwrap();
    let commit_proof = CommandsCommitProof::new_latest(vec![], SidechainBlockCommitProof {
        header: SidechainBlockHeader {
            network: foreign_block.network().as_byte(),
            parent_id: *parent_id.hash(),
            justify_id: *qc1.calculate_id().hash(),
            height: foreign_block.height().as_u64(),
            epoch: epoch.as_u64(),
            shard_group: tari_sidechain::ShardGroup {
                start: shard_group.start().as_u32(),
                end_inclusive: shard_group.end().as_u32(),
            },
            proposed_by: Default::default(),
            state_merkle_root: Default::default(),
            command_merkle_root: Default::default(),
            signature: Default::default(),
            accumulated_data: Default::default(),
            metadata_hash: Default::default(),
        },
        proof_elements: vec![CommitProofElement::QuorumCertificate(
            tari_sidechain::QuorumCertificate {
                header_hash: foreign_block.header().calculate_hash(),
                parent_id: *parent_id.hash(),
                signatures: vec![],
                decision: QuorumDecision::Accept,
            },
        )],
    });

    ForeignProposalRecord::new(ForeignProposal::new(commit_proof, BlockPledge::default()))
}
