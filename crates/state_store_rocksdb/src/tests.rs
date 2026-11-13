//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rocksdb::{SingleThreaded, Transaction, TransactionDB};
use tari_consensus_types::BlockId;
use tari_ootle_common_types::{Epoch, NodeHeight};
use tari_ootle_storage::{Ordering, StorageError};
use tari_ootle_transaction::TransactionId;
use tari_sidechain::FixedHash;
use tempfile::TempDir;

use crate::{
    cf_api::DbContext,
    codecs::{BlockIdCodec, BytesCodec, EpochCodec, NumberCodec, Prefixed, UnitCodec},
    traits::{Cf, QueryCf},
};

pub fn create_rocksdb(cf_names: impl IntoIterator<Item = &'static str>) -> (TransactionDB<SingleThreaded>, TempDir) {
    let temp_dir = tempfile::Builder::new().disable_cleanup(false).tempdir().unwrap();
    let path = temp_dir.path().join("rocksdb");
    // Use the production options so that tests exercise the production filters and read options
    let (db_opts, _budget) = crate::store::build_default_store_opts(&crate::options::DatabaseOptions::default());
    let tx_opts = rocksdb::TransactionDBOptions::default();
    let cf_descriptors = cf_names
        .into_iter()
        .map(|name| rocksdb::ColumnFamilyDescriptor::new(name, db_opts.clone()));
    let db = TransactionDB::open_cf_descriptors(&db_opts, &tx_opts, path, cf_descriptors)
        .map_err(|e| StorageError::ConnectionError {
            reason: e.into_string(),
        })
        .unwrap();

    (db, temp_dir)
}

pub fn ctx<'a>(
    db: &'a TransactionDB<SingleThreaded>,
    tx: &'a Transaction<TransactionDB>,
) -> DbContext<'a, Transaction<'a, TransactionDB<SingleThreaded>>> {
    DbContext::new(db, tx)
}

fn block_id_from_seed(seed: u64) -> BlockId {
    let mut bytes = [0u8; 32];
    bytes[0..8].copy_from_slice(&seed.to_be_bytes());
    BlockId::new(FixedHash::new(bytes))
}

fn transaction_id_from_seed(seed: u64) -> TransactionId {
    let mut bytes = [0u8; 32];
    bytes[0..8].copy_from_slice(&seed.to_be_bytes());
    TransactionId::new(bytes)
}

pub struct TwoBlocksCf;

impl Cf for TwoBlocksCf {
    type Key = (BlockId, TransactionId);
    type KeyCodec = (BlockIdCodec, BytesCodec);
    type Prefix = ();
    type Value = u64;
    type ValueCodec = NumberCodec<Self::Value>;

    fn name() -> &'static str {
        "two_blocks"
    }
}

pub struct TwoBlocksPrefixQuery;

impl QueryCf for TwoBlocksPrefixQuery {
    type Cf = TwoBlocksCf;
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
}

pub struct EpochHeightBlock;

impl Cf for EpochHeightBlock {
    type Key = (Epoch, NodeHeight, BlockId);
    type KeyCodec = (EpochCodec, NumberCodec<NodeHeight>, BlockIdCodec);
    type Prefix = ();
    type Value = ();
    type ValueCodec = UnitCodec;

    fn name() -> &'static str {
        "epoch_height_block"
    }
}

pub struct ByEpochQuery;

impl QueryCf for ByEpochQuery {
    type Cf = EpochHeightBlock;
    type Key = Epoch;
    type KeyCodec = EpochCodec;
}

/// Two logical tables that share one physical column family, separated only by their key prefix byte.
const SHARED_CF: &str = "shared_prefixed";

pub struct LowerPrefix;

impl Prefixed for LowerPrefix {
    fn prefix() -> Option<u8> {
        Some(1)
    }
}

pub struct UpperPrefix;

impl Prefixed for UpperPrefix {
    fn prefix() -> Option<u8> {
        Some(2)
    }
}

pub struct LowerEpochBlockCf;

impl Cf for LowerEpochBlockCf {
    type Key = (Epoch, BlockId);
    type KeyCodec = (EpochCodec, BlockIdCodec);
    type Prefix = LowerPrefix;
    type Value = ();
    type ValueCodec = UnitCodec;

    fn name() -> &'static str {
        SHARED_CF
    }
}

pub struct LowerByEpochQuery;

impl QueryCf for LowerByEpochQuery {
    type Cf = LowerEpochBlockCf;
    type Key = Epoch;
    type KeyCodec = EpochCodec;
}

pub struct UpperEpochBlockCf;

impl Cf for UpperEpochBlockCf {
    type Key = (Epoch, BlockId);
    type KeyCodec = (EpochCodec, BlockIdCodec);
    type Prefix = UpperPrefix;
    type Value = ();
    type ValueCodec = UnitCodec;

    fn name() -> &'static str {
        SHARED_CF
    }
}

pub struct UpperByEpochQuery;

impl QueryCf for UpperByEpochQuery {
    type Cf = UpperEpochBlockCf;
    type Key = Epoch;
    type KeyCodec = EpochCodec;
}

#[test]
fn it_puts_and_retrieves() {
    const OP: &str = "it_puts_and_retrieves_in_single_transaction";
    let (db, _tmp) = create_rocksdb([TwoBlocksCf::name()]);
    let tx = db.transaction();
    let ctx = ctx(&db, &tx);
    let cf = ctx.cf(TwoBlocksCf).unwrap();
    let key = (block_id_from_seed(1), transaction_id_from_seed(2));
    cf.put(&key, &123, OP).unwrap();
    let v = cf.get(&key, OP).unwrap();
    assert_eq!(v, 123);
    assert!(cf.exists(&key, OP).unwrap());
}

#[test]
fn it_iterates_using_the_prefix() {
    const OP: &str = "it_iterates_using_the_prefix";
    let (db, _tmp) = create_rocksdb([TwoBlocksCf::name()]);
    let tx = db.transaction();
    let ctx = ctx(&db, &tx);
    let cf = ctx.cf(TwoBlocksCf).unwrap();
    let key = (block_id_from_seed(1), transaction_id_from_seed(2));
    cf.put(&key, &10, OP).unwrap();
    let key = (block_id_from_seed(1), transaction_id_from_seed(3));
    cf.put(&key, &11, OP).unwrap();
    let key = (block_id_from_seed(1), transaction_id_from_seed(4));
    cf.put(&key, &12, OP).unwrap();
    let key = (block_id_from_seed(2), transaction_id_from_seed(3));
    cf.put(&key, &2, OP).unwrap();
    let key = (block_id_from_seed(3), transaction_id_from_seed(4));
    cf.put(&key, &3, OP).unwrap();

    let query = ctx.cf(TwoBlocksPrefixQuery).unwrap();

    let iter = query.query_prefix_range_iterator(Ordering::default(), &block_id_from_seed(1));

    let mut count = 0;

    for (i, result) in iter.enumerate() {
        let ((b1, b2), v) = result.unwrap();
        assert_eq!(b1, block_id_from_seed(1));
        assert_eq!(b2, transaction_id_from_seed(2 + i as u64));
        assert_eq!(v, 10 + i as u64);
        count += 1;
    }

    assert_eq!(count, 3);
}

#[test]
fn it_iterates_over_epoch_heights_ordering() {
    const OP: &str = "it_iterates_over_epoch_heights_ordering";
    let (db, _tmp) = create_rocksdb([EpochHeightBlock::name()]);
    let tx = db.transaction();
    let ctx = ctx(&db, &tx);
    let cf = ctx.cf(EpochHeightBlock).unwrap();
    for key in (0..100).map(|i| (Epoch(0), NodeHeight(i), block_id_from_seed(i))) {
        cf.put(&key, &(), OP).unwrap();
    }
    for key in (0..100).map(|i| (Epoch(1), NodeHeight(i), block_id_from_seed(i))) {
        cf.put(&key, &(), OP).unwrap();
    }

    let query = ctx.cf(ByEpochQuery).unwrap();

    let iter = query.query_prefix_range_key_iterator(Ordering::Ascending, &Epoch(0));
    for (i, result) in iter.enumerate() {
        let (epoch, height, _) = result.unwrap();
        assert_eq!(epoch, Epoch(0));
        assert_eq!(height, NodeHeight(i as u64));
    }

    let iter = query.query_prefix_range_key_iterator(Ordering::Descending, &Epoch(0));
    for (i, result) in iter.enumerate() {
        let i = 100 - 1 - i as u64;
        let (epoch, height, _) = result.unwrap();
        assert_eq!(epoch, Epoch(0));
        assert_eq!(height, NodeHeight(i));
    }
}

fn seed_shared_prefixed_cf(ctx: &DbContext<'_, Transaction<'_, TransactionDB<SingleThreaded>>>) {
    const OP: &str = "seed_shared_prefixed_cf";
    let lower = ctx.cf(LowerEpochBlockCf).unwrap();
    let upper = ctx.cf(UpperEpochBlockCf).unwrap();
    for i in 0..5u64 {
        lower.put(&(Epoch(i), block_id_from_seed(i)), &(), OP).unwrap();
        upper.put(&(Epoch(i), block_id_from_seed(i)), &(), OP).unwrap();
    }
}

#[test]
fn end_range_iterator_does_not_read_the_preceding_prefix() {
    let (db, _tmp) = create_rocksdb([SHARED_CF]);
    let tx = db.transaction();
    let ctx = ctx(&db, &tx);
    seed_shared_prefixed_cf(&ctx);

    let query = ctx.cf(UpperByEpochQuery).unwrap();

    let epochs = query
        .query_end_range_iterator(Ordering::Ascending, &Epoch(3))
        .map(|res| res.map(|((epoch, _), ())| epoch))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(epochs, vec![Epoch(0), Epoch(1), Epoch(2)]);

    let epochs = query
        .query_end_range_key_iterator(Ordering::Descending, &Epoch(3))
        .map(|res| res.map(|(epoch, _)| epoch))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(epochs, vec![Epoch(2), Epoch(1), Epoch(0)]);
}

#[test]
fn start_range_iterator_does_not_read_the_following_prefix() {
    let (db, _tmp) = create_rocksdb([SHARED_CF]);
    let tx = db.transaction();
    let ctx = ctx(&db, &tx);
    seed_shared_prefixed_cf(&ctx);

    let query = ctx.cf(LowerByEpochQuery).unwrap();

    let epochs = query
        .query_start_range_iterator(Ordering::Ascending, &Epoch(3))
        .map(|res| res.map(|((epoch, _), ())| epoch))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(epochs, vec![Epoch(3), Epoch(4)]);

    let epochs = query
        .query_start_range_key_iterator(Ordering::Descending, &Epoch(3))
        .map(|res| res.map(|(epoch, _)| epoch))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(epochs, vec![Epoch(4), Epoch(3)]);
}

#[test]
fn query_last_does_not_read_the_following_prefix() {
    const OP: &str = "query_last_does_not_read_the_following_prefix";
    let (db, _tmp) = create_rocksdb([SHARED_CF]);
    let tx = db.transaction();
    let ctx = ctx(&db, &tx);
    seed_shared_prefixed_cf(&ctx);

    let ((epoch, block_id), ()) = ctx.cf(LowerByEpochQuery).unwrap().query_last(OP).unwrap();
    assert_eq!(epoch, Epoch(4));
    assert_eq!(block_id, block_id_from_seed(4));
}

#[test]
fn multi_get_exact_returns_values_in_key_order_and_rejects_a_missing_key() {
    const OP: &str = "multi_get_exact";
    let (db, _tmp) = create_rocksdb([TwoBlocksCf::name()]);
    let tx = db.transaction();
    let ctx = ctx(&db, &tx);
    let cf = ctx.cf(TwoBlocksCf).unwrap();
    let keys = (1..=3)
        .map(|seed| (block_id_from_seed(seed), transaction_id_from_seed(seed)))
        .collect::<Vec<_>>();
    for (value, key) in keys.iter().enumerate() {
        cf.put(key, &(value as u64), OP).unwrap();
    }

    let values = cf.multi_get_exact(keys.iter().rev(), OP).unwrap();
    assert_eq!(values, [2, 1, 0]);

    let missing = (block_id_from_seed(9), transaction_id_from_seed(9));
    let err = cf
        .multi_get_exact([&keys[0], &missing, &keys[2]], OP)
        .expect_err("a missing key must not be skipped");
    assert!(
        matches!(err, crate::error::RocksDbStorageError::NotFound { .. }),
        "{err}"
    );
}

#[test]
#[expect(clippy::too_many_lines)]
fn foreign_proposals_clear_proposed_in_does_not_reset_confirmed_or_invalid_status() {
    use tari_common_types::types::CompressedPublicKey;
    use tari_consensus_types::{PcId, ProposalCertificate, ShardGroupAccumulatedData};
    use tari_ootle_common_types::{ExtraData, NumPreshards, ProtocolVersion, ShardGroup};
    use tari_ootle_storage::{
        StateStore,
        StateStoreReadTransaction,
        StateStoreWriteTransaction,
        consensus_models::{
            Block,
            BlockCommands,
            BlockHeader,
            BlockHeaderMetadata,
            BlockPledge,
            CommandsCommitProof,
            ForeignProposal,
            ForeignProposalRecord,
            ForeignProposalStatus,
        },
    };
    use tari_ootle_transaction::Network;
    use tari_sidechain::{QuorumDecision, SidechainBlockCommitProof, SidechainBlockHeader};
    use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

    use crate::{DatabaseOptions, RocksDbStateStore};

    fn make_fp(height: u64) -> ForeignProposalRecord {
        let metadata = BlockHeaderMetadata {
            total_leader_fee: 0,
            timestamp: 0,
            extra_data: Default::default(),
            timeout_certificate_id: None,
            is_dummy: false,
        };
        let proof = CommandsCommitProof::new_latest(vec![], SidechainBlockCommitProof {
            header: SidechainBlockHeader {
                network: 0,
                protocol_version: 0,
                parent_id: Default::default(),
                justify_id: Default::default(),
                height,
                epoch: 0,
                epoch_hash: Default::default(),
                shard_group: tari_sidechain::ShardGroup {
                    start: 0,
                    end_inclusive: 127,
                },
                proposed_by: CompressedPublicKey::default(),
                state_merkle_root: Default::default(),
                command_merkle_root: Default::default(),
                transaction_merkle_root: Default::default(),
                signature: Default::default(),
                accumulated_data: Default::default(),
                metadata_hash: metadata.calculate_hash(),
            },
            proof_elements: vec![],
        });
        ForeignProposalRecord::new(ForeignProposal::new(proof, BlockPledge::default(), metadata))
    }

    fn make_fork_block(parent: &Block, timestamp: u64) -> Block {
        let shard_group = ShardGroup::all_shards(NumPreshards::P256);
        let justify = ProposalCertificate::new(
            *parent.id().hash(),
            *parent.id(),
            parent.height(),
            parent.epoch(),
            shard_group,
            ProposalCertificate::genesis_signatures(),
            QuorumDecision::Accept,
        );
        let commands = BlockCommands::empty();
        let header = BlockHeader::create_unsigned(
            Network::LocalNet,
            ProtocolVersion::V0,
            *parent.id(),
            justify.calculate_id(),
            None,
            parent.height() + NodeHeight(1),
            parent.epoch(),
            shard_group,
            RistrettoPublicKeyBytes::default(),
            FixedHash::zero(),
            &commands,
            0,
            timestamp,
            FixedHash::zero(),
            ShardGroupAccumulatedData::default(),
            ExtraData::new(),
        )
        .unwrap();
        Block::new(header, justify, commands, None)
    }

    let tmp = tempfile::tempdir().unwrap();
    let store = RocksDbStateStore::<String>::open(tmp.path().join("db"), DatabaseOptions::default()).unwrap();

    let zero = Block::zero_block(Network::LocalNet, NumPreshards::P256);
    let block_a = make_fork_block(&zero, 1);
    let block_b = make_fork_block(&zero, 2);
    assert_ne!(block_a.id(), block_b.id());

    let fp_confirmed = make_fp(1);
    let fp_invalid = make_fp(2);
    let fp_proposed = make_fp(3);

    store
        .with_write_tx(|tx| {
            zero.insert(tx)?;
            tx.blocks_set_qcs(zero.id(), Some(&PcId::zero()), Some(&PcId::zero()))?;
            tx.locked_block_set(&zero.as_locked())?;
            block_a.insert(tx)?;
            block_b.insert(tx)?;

            tx.foreign_proposals_save(&fp_confirmed)?;
            tx.foreign_proposals_set_status(
                fp_confirmed.block_id(),
                ForeignProposalStatus::Confirmed,
                Some(block_a.id()),
            )?;

            tx.foreign_proposals_save(&fp_invalid)?;
            tx.foreign_proposals_set_status(
                fp_invalid.block_id(),
                ForeignProposalStatus::Invalid,
                Some(block_a.id()),
            )?;

            tx.foreign_proposals_save(&fp_proposed)?;
            tx.foreign_proposals_set_status(
                fp_proposed.block_id(),
                ForeignProposalStatus::Proposed,
                Some(block_a.id()),
            )?;
            // Propose in a competing fork block_b; index for block_a must remain until block_a is cleared
            tx.foreign_proposals_set_status(
                fp_proposed.block_id(),
                ForeignProposalStatus::Proposed,
                Some(block_b.id()),
            )?;
            Ok::<_, StorageError>(())
        })
        .unwrap();

    // Before clearing either fork block, `fp_proposed` must not be returned as new on `block_a`'s chain
    // even though `proposed_in_block` on the record was overwritten with `block_b`.
    store
        .with_read_tx(|tx| {
            let new_on_a = tx.foreign_proposals_get_all_new(block_a.id(), usize::MAX)?;
            assert!(
                new_on_a.is_empty(),
                "expected no new FPs on block_a chain, got {new_on_a:?}"
            );
            let new_on_b = tx.foreign_proposals_get_all_new(block_b.id(), usize::MAX)?;
            assert!(
                new_on_b.is_empty(),
                "expected no new FPs on block_b chain, got {new_on_b:?}"
            );
            Ok::<_, StorageError>(())
        })
        .unwrap();

    // Clearing `block_b` resets `fp_proposed` to `New` (available again on `block_b`'s branch), while
    // `block_a`'s preserved `ProposedInBlockIndex` entry still suppresses `get_all_new` on `block_a`'s chain.
    store
        .with_write_tx(|tx| tx.foreign_proposals_clear_proposed_in(block_b.id()))
        .unwrap();

    store
        .with_read_tx(|tx| {
            let new_on_a = tx.foreign_proposals_get_all_new(block_a.id(), usize::MAX)?;
            assert!(
                new_on_a.is_empty(),
                "expected block_a index to still suppress fp_proposed, got {new_on_a:?}"
            );
            let new_on_b = tx.foreign_proposals_get_all_new(block_b.id(), usize::MAX)?;
            assert_eq!(new_on_b.len(), 1);
            assert_eq!(new_on_b[0].block_id(), fp_proposed.block_id());
            assert_eq!(new_on_b[0].status(), ForeignProposalStatus::New);
            Ok::<_, StorageError>(())
        })
        .unwrap();

    // Re-propose in `block_b`, then clear `block_a` and verify `Confirmed`/`Invalid` are untouched and
    // `fp_proposed` remains `Proposed` in `block_b`.
    store
        .with_write_tx(|tx| {
            tx.foreign_proposals_set_status(
                fp_proposed.block_id(),
                ForeignProposalStatus::Proposed,
                Some(block_b.id()),
            )?;
            tx.foreign_proposals_clear_proposed_in(block_a.id())?;
            Ok::<_, StorageError>(())
        })
        .unwrap();

    store
        .with_read_tx(|tx| {
            let records =
                tx.foreign_proposals_get_any([fp_confirmed.block_id(), fp_invalid.block_id(), fp_proposed.block_id()])?;
            assert_eq!(records.len(), 3);
            for rec in &records {
                if rec.block_id() == fp_confirmed.block_id() {
                    assert_eq!(rec.status(), ForeignProposalStatus::Confirmed);
                } else if rec.block_id() == fp_invalid.block_id() {
                    assert_eq!(rec.status(), ForeignProposalStatus::Invalid);
                } else {
                    assert_eq!(rec.block_id(), fp_proposed.block_id());
                    // Still Proposed in block_b after clearing block_a
                    assert_eq!(rec.status(), ForeignProposalStatus::Proposed);
                    assert_eq!(rec.proposed_in_block(), Some(block_b.id()));
                }
            }
            let new_on_a = tx.foreign_proposals_get_all_new(block_a.id(), usize::MAX)?;
            assert_eq!(new_on_a.len(), 1);
            assert_eq!(new_on_a[0].block_id(), fp_proposed.block_id());
            let new_on_b = tx.foreign_proposals_get_all_new(block_b.id(), usize::MAX)?;
            assert!(new_on_b.is_empty());
            Ok::<_, StorageError>(())
        })
        .unwrap();
}

#[test]
fn state_sync_missing_values_get_earliest_includes_max_substate_address() {
    use tari_ootle_common_types::{SubstateAddress, shard::Shard};
    use tari_ootle_storage::{StateStore, StateStoreReadTransaction, StateStoreWriteTransaction};

    use crate::{DatabaseOptions, RocksDbStateStore};

    let tmp = tempfile::tempdir().unwrap();
    let store = RocksDbStateStore::<String>::open(tmp.path().join("db"), DatabaseOptions::default()).unwrap();
    let shard = Shard::from(1u32);
    let max_addr = SubstateAddress::max();

    store
        .with_write_tx(|tx| tx.state_sync_missing_value_insert(shard, &max_addr, 7))
        .unwrap();

    let earliest = store
        .with_read_tx(|tx| tx.state_sync_missing_values_get_earliest(shard, 0))
        .unwrap();
    assert_eq!(earliest, Some((max_addr, 7)));
}

#[test]
fn template_provider_checks_liveness_and_value_presence() {
    use tari_engine_types::{
        published_template::{PublishedTemplate, PublishedTemplateAddress, TemplateBlob, TemplateName},
        substate::{SubstateId, SubstateValue},
    };
    use tari_ootle_common_types::{
        SubstateVersion,
        VersionedSubstateId,
        services::template_provider::TemplateProvider,
        shard::Shard,
    };
    use tari_ootle_storage::{
        StateStore,
        StateStoreWriteTransaction,
        consensus_models::{SubstateTransition, SubstateUpdateBatch, SubstateValueOrHash},
    };
    use tari_ootle_transaction::Network;
    use tari_template_lib_types::{Hash32, TemplateAddress};

    use crate::{DatabaseOptions, RocksDbStateStore};

    fn make_template(seed: u8) -> (TemplateAddress, SubstateValue) {
        let addr = TemplateAddress::from_array([seed; 32]);
        let template = PublishedTemplate {
            template_name: TemplateName::new_checked("TestTemplate").unwrap(),
            author: Default::default(),
            binary: TemplateBlob::new_checked(vec![seed; 8]).unwrap(),
            at_epoch: 0,
            metadata_hash: None,
        };
        (addr, SubstateValue::Template(template))
    }

    let tmp = tempfile::tempdir().unwrap();
    let store = RocksDbStateStore::<String>::open(tmp.path().join("db"), DatabaseOptions::default()).unwrap();

    let (live_addr, live_val) = make_template(1);
    let (destroyed_addr, destroyed_val) = make_template(2);
    let hash_only_addr = TemplateAddress::from_array([3u8; 32]);

    store
        .with_write_tx(|tx| {
            let mut batch = SubstateUpdateBatch::new(Network::LocalNet, Epoch(0));
            batch.with_transition(Shard::first(), 1).extend([
                SubstateTransition::Up {
                    id: SubstateId::Template(PublishedTemplateAddress::from_hash(live_addr)),
                    version: SubstateVersion::ZERO,
                    substate_or_hash: SubstateValueOrHash::Value(Box::new(live_val)),
                },
                SubstateTransition::Up {
                    id: SubstateId::Template(PublishedTemplateAddress::from_hash(destroyed_addr)),
                    version: SubstateVersion::ZERO,
                    substate_or_hash: SubstateValueOrHash::Value(Box::new(destroyed_val)),
                },
                SubstateTransition::Up {
                    id: SubstateId::Template(PublishedTemplateAddress::from_hash(hash_only_addr)),
                    version: SubstateVersion::ZERO,
                    substate_or_hash: SubstateValueOrHash::Hash(Hash32::from_array([9u8; 32])),
                },
            ]);
            batch
                .with_transition(Shard::first(), 2)
                .push(SubstateTransition::Down {
                    id: VersionedSubstateId::new(
                        SubstateId::Template(PublishedTemplateAddress::from_hash(destroyed_addr)),
                        SubstateVersion::ZERO,
                    ),
                });
            tx.substates_commit_batch(batch)
        })
        .unwrap();

    assert!(store.has_template(&live_addr).unwrap());
    assert!(store.get_template(&live_addr).unwrap().is_some());

    assert!(!store.has_template(&destroyed_addr).unwrap());
    assert!(store.get_template(&destroyed_addr).unwrap().is_none());

    assert!(!store.has_template(&hash_only_addr).unwrap());
    assert!(store.get_template(&hash_only_addr).unwrap().is_none());
}

#[test]
#[expect(clippy::too_many_lines)]
fn reader_zero_limit_and_descending_offset_bounds() {
    use tari_common_types::types::CompressedPublicKey;
    use tari_consensus_types::{Decision, PcId, ProposalCertificate, ShardGroupAccumulatedData};
    use tari_ootle_common_types::{ExtraData, NumPreshards, ProtocolVersion, ShardGroup};
    use tari_ootle_storage::{
        StateStore,
        StateStoreReadTransaction,
        StateStoreWriteTransaction,
        consensus_models::{
            Block,
            BlockCommands,
            BlockHeader,
            BlockHeaderMetadata,
            BlockPledge,
            CommandsCommitProof,
            Evidence,
            ForeignProposal,
            ForeignProposalRecord,
        },
    };
    use tari_ootle_transaction::Network;
    use tari_sidechain::{QuorumDecision, SidechainBlockCommitProof, SidechainBlockHeader};
    use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

    use crate::{DatabaseOptions, RocksDbStateStore};

    let tmp = tempfile::tempdir().unwrap();
    let store = RocksDbStateStore::<String>::open(tmp.path().join("db"), DatabaseOptions::default()).unwrap();

    let shard_group = ShardGroup::all_shards(NumPreshards::P256);
    let zero = Block::zero_block(Network::LocalNet, NumPreshards::P256);
    let justify = ProposalCertificate::new(
        *zero.id().hash(),
        *zero.id(),
        zero.height(),
        zero.epoch(),
        shard_group,
        ProposalCertificate::genesis_signatures(),
        QuorumDecision::Accept,
    );
    let commands = BlockCommands::empty();
    let header = BlockHeader::create_unsigned(
        Network::LocalNet,
        ProtocolVersion::V0,
        *zero.id(),
        justify.calculate_id(),
        None,
        NodeHeight(1),
        Epoch(0),
        shard_group,
        RistrettoPublicKeyBytes::default(),
        FixedHash::zero(),
        &commands,
        0,
        1,
        FixedHash::zero(),
        ShardGroupAccumulatedData::default(),
        ExtraData::new(),
    )
    .unwrap();
    let block_a = Block::new(header, justify, commands, None);

    let metadata = BlockHeaderMetadata {
        total_leader_fee: 0,
        timestamp: 0,
        extra_data: Default::default(),
        timeout_certificate_id: None,
        is_dummy: false,
    };
    let proof = CommandsCommitProof::new_latest(vec![], SidechainBlockCommitProof {
        header: SidechainBlockHeader {
            network: 0,
            protocol_version: 0,
            parent_id: Default::default(),
            justify_id: Default::default(),
            height: 1,
            epoch: 0,
            epoch_hash: Default::default(),
            shard_group: tari_sidechain::ShardGroup {
                start: 0,
                end_inclusive: 127,
            },
            proposed_by: CompressedPublicKey::default(),
            state_merkle_root: Default::default(),
            command_merkle_root: Default::default(),
            transaction_merkle_root: Default::default(),
            signature: Default::default(),
            accumulated_data: Default::default(),
            metadata_hash: metadata.calculate_hash(),
        },
        proof_elements: vec![],
    });
    let fp = ForeignProposalRecord::new(ForeignProposal::new(proof, BlockPledge::default(), metadata));

    store
        .with_write_tx(|tx| {
            zero.insert(tx)?;
            tx.blocks_set_qcs(zero.id(), Some(&PcId::zero()), Some(&PcId::zero()))?;
            tx.locked_block_set(&zero.as_locked())?;
            block_a.insert(tx)?;
            tx.leaf_block_set(&block_a.as_leaf())?;
            tx.foreign_proposals_save(&fp)?;
            tx.transaction_pool_insert_new(
                transaction_id_from_seed(1),
                Decision::Commit,
                &Evidence::empty(),
                true,
                false,
                Epoch(10),
                0,
            )?;
            Ok::<_, StorageError>(())
        })
        .unwrap();

    store
        .with_read_tx(|tx| {
            assert!(tx.blocks_get_parent_chain(block_a.id(), 0)?.is_empty());
            assert_eq!(tx.blocks_get_parent_chain(block_a.id(), 1)?.len(), 1);

            assert!(tx.foreign_proposals_get_all_new(block_a.id(), 0)?.is_empty());
            assert_eq!(tx.foreign_proposals_get_all_new(block_a.id(), 1)?.len(), 1);

            assert!(
                tx.blocks_get_all_between(Epoch(0), NodeHeight(0), NodeHeight(1), true, 0)?
                    .is_empty()
            );
            assert_eq!(
                tx.blocks_get_all_between(Epoch(0), NodeHeight(0), NodeHeight(1), true, 1)?
                    .len(),
                1
            );

            assert!(tx.blocks_get_paginated(0, 0, None, None, None, None)?.is_empty());
            assert_eq!(tx.blocks_get_paginated(1, 0, None, None, None, None)?.len(), 1);

            // Leaf height is 1: offset 1 returns genesis (height 0), offset 2 (> leaf height) returns empty
            let at_leaf_offset = tx.blocks_get_paginated(10, 1, None, None, None, Some(Ordering::Descending))?;
            assert_eq!(at_leaf_offset.len(), 1);
            assert_eq!(at_leaf_offset[0].id(), zero.id());
            assert!(
                tx.blocks_get_paginated(10, 2, None, None, None, Some(Ordering::Descending))?
                    .is_empty()
            );

            assert!(tx.transaction_pool_get_all(0)?.is_empty());
            assert_eq!(tx.transaction_pool_get_all(1)?.len(), 1);

            Ok::<_, StorageError>(())
        })
        .unwrap();
}

#[test]
fn fixed_bytes_codec_rejects_wrong_length_and_decode_error_formats_correctly() {
    use crate::{
        codecs::{DbDecoder, DbEncoder, FixedBytesCodec},
        error::RocksDbStorageError,
    };

    let codec = FixedBytesCodec::<4>;
    assert_eq!(codec.encode_len(&[1u8, 2, 3, 4]).unwrap(), 4);
    assert!(codec.encode_len(&[1u8, 2, 3]).is_err());
    assert!(codec.encode(&[1u8, 2, 3]).is_err());
    let mut buf = Vec::new();
    assert!(codec.encode_into(&[1u8, 2, 3, 4, 5], &mut buf).is_err());

    let err: RocksDbStorageError = <FixedBytesCodec<4> as DbDecoder<[u8; 4]>>::decode(&codec, &[1u8, 2]).unwrap_err();
    assert!(err.to_string().starts_with("Decode error:"), "got: {err}");
}
