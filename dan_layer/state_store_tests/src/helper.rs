//   Copyright 2025. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{io::Write, ops::Deref};

use rand::{rngs::OsRng, Rng, RngCore};
use tari_bor::cbor;
use tari_common_types::types::FixedHash;
use tari_crypto::{keys::SecretKey, ristretto::RistrettoSecretKey};
use tari_dan_common_types::{shard::Shard, Epoch, ExtraData, NodeHeight, NumPreshards, ShardGroup};
use tari_dan_storage::{
    consensus_models::{
        Block,
        BlockId,
        Command,
        Decision,
        LeafBlock,
        QcId,
        QuorumCertificate,
        QuorumDecision,
        SubstateRecord,
        TransactionAtom,
        ValidatorSignature,
    },
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};
use tari_engine_types::{
    component::{ComponentBody, ComponentHeader},
    substate::{hash_substate, Substate, SubstateId, SubstateValue},
};
use tari_state_store_rocksdb::RocksDbStateStore;
use tari_state_store_sqlite::SqliteStateStore;
use tari_template_lib::{
    auth::OwnerRule,
    models::ComponentAddress,
    prelude::{ComponentAccessRules, TemplateAddress},
    types::{ComponentKey, EntityId, ObjectKey},
};
use tari_transaction::TransactionId;
use tari_utilities::epoch_time::EpochTime;
use tempfile::TempDir;

pub const fn num_preshards() -> NumPreshards {
    NumPreshards::P256
}

/// Create a RocksDbStateStore and a temporary directory
/// NOTE: this takes around 1.5s on my machine (AMD Ryzen 9 5950X, SSD)
pub fn create_rocksdb() -> (RocksDbStateStore<String>, TempDir) {
    let temp_dir = tempfile::Builder::new().keep(false).tempdir().unwrap();
    let db_file = temp_dir.path().join("rocksdb");
    (RocksDbStateStore::connect(db_file).unwrap(), temp_dir)
}

pub fn create_sqlite() -> SqliteStateStore<String> {
    let db = SqliteStateStore::connect(":memory:").unwrap();

    // Need FK=off because otherwise we'd have to create transactions for each in the pool
    db.foreign_keys_off().unwrap();
    db
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

pub fn build_substate_record(substate_id: &SubstateId, version: u32) -> SubstateRecord {
    let entity_id = substate_id.to_object_key().as_entity_id();
    let value = build_substate_value(Some(entity_id));
    SubstateRecord {
        substate_id: substate_id.clone(),
        version,
        state_hash: hash_substate(&value, version),
        substate_value: Some(value),
        created_justify: QcId::zero(),
        created_block: BlockId::zero(),
        created_by_shard: Shard::first(),
        created_at_epoch: Epoch::zero(),
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

pub fn substate_id_seed(seed: u32) -> SubstateId {
    let mut buf = [0u8; EntityId::LENGTH];
    buf[..size_of::<u32>()].copy_from_slice(&seed.to_be_bytes());
    let entity_id = EntityId::from_array(buf);
    let mut buf = [0u8; ComponentKey::LENGTH];
    buf[..size_of::<u32>()].copy_from_slice(&seed.to_be_bytes());
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
    range: impl IntoIterator<Item = u32>,
    version: u32,
) -> impl Iterator<Item = (SubstateId, Substate)> {
    range.into_iter().map(move |i| {
        let substate_id = substate_id_seed(i);
        let value = substate_value_for_entity(substate_id.to_object_key().as_entity_id());
        (substate_id, Substate::new(version, value))
    })
}

pub fn assert_eq_debug<T>(a: &T, b: &T)
where T: std::fmt::Debug {
    assert_eq!(format!("{:?}", a), format!("{:?}", b),);
}

pub fn create_random_block_id() -> BlockId {
    BlockId::new(create_random_hash())
}

pub fn create_random_hash() -> FixedHash {
    let rand_bytes = OsRng.gen::<[u8; FixedHash::byte_size()]>();
    FixedHash::new(rand_bytes)
}

pub fn create_random_vn_signature() -> ValidatorSignature {
    let message = OsRng.gen::<[u8; FixedHash::byte_size()]>();
    let secret_key = RistrettoSecretKey::random(&mut OsRng);
    ValidatorSignature::sign(&secret_key, message)
}

pub fn create_block(parent: Option<&Block>) -> Block {
    let network = Default::default();

    let Some(parent) = parent else {
        return Block::zero_block(network, num_preshards());
    };

    let atom1 = create_tx_atom();

    // This prevents all blocks to have the same hash/id
    let random_merkle_root = create_random_hash();

    Block::create(
        network,
        *parent.id(),
        parent.justify().clone(),
        NodeHeight(1),
        Epoch(0),
        ShardGroup::all_shards(num_preshards()),
        Default::default(),
        // Need to have a command in, otherwise this block will not be included internally in the query because it
        // cannot cause a state change without any commands
        [Command::LocalPrepare(atom1.clone())].into_iter().collect(),
        random_merkle_root,
        Default::default(),
        Default::default(),
        None,
        EpochTime::now().as_u64(),
        0,
        FixedHash::zero(),
        ExtraData::default(),
    )
    .unwrap()
}

pub fn create_block_with_qc(parent: &LeafBlock) -> Block {
    let network = Default::default();

    let atom1 = create_tx_atom();

    // This prevents all blocks to have the same hash/id
    let random_merkle_root = create_random_hash();

    let qc = create_qc(parent);

    Block::create(
        network,
        *parent.block_id(),
        qc,
        parent.height() + NodeHeight(1),
        parent.epoch(),
        ShardGroup::all_shards(num_preshards()),
        Default::default(),
        // Need to have a command in, otherwise this block will not be included internally in the query because it
        // cannot cause a state change without any commands
        [Command::LocalPrepare(atom1.clone())].into_iter().collect(),
        random_merkle_root,
        Default::default(),
        Default::default(),
        None,
        EpochTime::now().as_u64(),
        0,
        FixedHash::zero(),
        ExtraData::default(),
    )
    .unwrap()
}
pub fn create_qc(block: &LeafBlock) -> QuorumCertificate {
    QuorumCertificate::new(
        *block.block_id().hash(),
        *block.block_id(),
        block.height(),
        block.epoch(),
        ShardGroup::all_shards(num_preshards()),
        vec![],
        vec![],
        QuorumDecision::Accept,
    )
}

pub fn create_chain(num_blocks: usize) -> Vec<Block> {
    let mut blocks = Vec::with_capacity(num_blocks);
    let block = create_block(None);
    let mut parent = block.as_leaf_block();
    blocks.push(block);
    for _ in 0..num_blocks {
        let block = create_block_with_qc(&parent);
        parent = block.as_leaf_block();
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
        block.justify().save(tx).unwrap();
    }
    let len = chain.len();
    if len < 4 {
        return;
    }

    chain[len - 4].as_locked_block().set(tx).unwrap();

    for block in &chain[..len - 4] {
        tx.blocks_set_flags(block.id(), Some(true), Some(true)).unwrap();
    }

    chain.last().unwrap().as_leaf_block().set(tx).unwrap();
}
