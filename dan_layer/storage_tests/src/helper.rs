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

use rand::{rngs::OsRng, Rng, RngCore};
use tari_common_types::types::{FixedHash, PublicKey};
use tari_crypto::{keys::PublicKey as _, signatures::SchnorrSignature};
use tari_dan_common_types::{shard::Shard, Epoch, ExtraData, NodeHeight, NumPreshards, ShardGroup};
use tari_dan_storage::consensus_models::{
    Block,
    BlockId,
    Command,
    Decision,
    QcId,
    SubstateRecord,
    TransactionAtom,
    ValidatorSignature,
};
use tari_engine_types::{
    component::{ComponentBody, ComponentHeader},
    substate::{SubstateId, SubstateValue},
};
use tari_state_store_rocksdb::RocksDbStateStore;
use tari_state_store_sqlite::SqliteStateStore;
use tari_template_lib::{
    auth::OwnerRule,
    models::{ComponentAddress, ComponentKey, EntityId, ObjectKey, TemplateAddress},
    prelude::ComponentAccessRules,
};
use tari_transaction::TransactionId;
use tari_utilities::epoch_time::EpochTime;
use tempfile::tempdir;

pub fn create_rocksdb() -> RocksDbStateStore<String> {
    let temp_dir = tempdir().unwrap();
    let db_file = temp_dir.path().join("rocksdb");
    let db_file = db_file.as_os_str().to_str().unwrap();

    RocksDbStateStore::connect(db_file).unwrap()
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
    let entity_id = EntityId::default();
    let rand_bytes = OsRng.gen::<[u8; ComponentKey::LENGTH]>();
    let component_key = ComponentKey::new(copy_fixed(&rand_bytes));
    SubstateId::Component(ComponentAddress::new(ObjectKey::new(entity_id, component_key)))
}

pub fn build_substate_record(substate_id: &SubstateId, version: u32) -> SubstateRecord {
    let entity_id = substate_id.to_object_key().as_entity_id();
    SubstateRecord {
        substate_id: substate_id.clone(),
        version,
        substate_value: Some(build_substate_value(Some(entity_id))),
        state_hash: FixedHash::default(),
        created_by_transaction: TransactionId::default(),
        created_justify: QcId::zero(),
        created_block: BlockId::genesis(),
        created_height: NodeHeight::zero(),
        created_by_shard: Shard::first(),
        created_at_epoch: Epoch::zero(),
        destroyed: None,
    }
}

pub fn build_substate_value(entity_id: Option<EntityId>) -> SubstateValue {
    SubstateValue::Component(ComponentHeader {
        template_address: TemplateAddress::default(),
        module_name: "foo".to_string(),
        owner_key: None,
        owner_rule: OwnerRule::None,
        access_rules: ComponentAccessRules::allow_all(),
        entity_id: entity_id.unwrap_or_default(),
        body: ComponentBody {
            state: tari_bor::Value::Null,
        },
    })
}

pub fn copy_fixed<const SZ: usize>(bytes: &[u8]) -> [u8; SZ] {
    let mut out = [0u8; SZ];
    out.copy_from_slice(bytes);
    out
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
    let (secret_key, public_key) = PublicKey::random_keypair(&mut OsRng);
    let signature = SchnorrSignature::sign(&secret_key, message, &mut OsRng).unwrap();
    ValidatorSignature { public_key, signature }
}

pub fn create_block(parent: Option<&Block>) -> Block {
    let network = Default::default();
    let num_preshards = NumPreshards::P64;

    let Some(parent) = parent else {
        return Block::zero_block(network, NumPreshards::P64);
    };

    let atom1 = create_tx_atom();

    // This prevents all blocks to have the same hash/id
    let random_merkle_root = create_random_hash();
    eprintln!("*** random_merkle: {}", random_merkle_root);

    Block::create(
        network,
        *parent.id(),
        parent.justify().clone(),
        NodeHeight(1),
        Epoch(0),
        ShardGroup::all_shards(num_preshards),
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
