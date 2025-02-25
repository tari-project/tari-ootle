//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_common_types::Epoch;
use tari_dan_storage::{StateStore, StateStoreReadTransaction, StateStoreWriteTransaction};

mod foreign_proposals_test {
    use tari_dan_common_types::{NumPreshards, ShardGroup};
    use tari_dan_storage::consensus_models::{
        Block,
        BlockPledge,
        ForeignProposal,
        ForeignProposalStatus,
        QuorumCertificate,
    };

    use super::*;
    use crate::helper::{assert_eq_debug, create_random_block_id, create_rocksdb, create_sqlite};

    #[test]
    fn foreign_proposals_sqlite() {
        let db = create_sqlite();
        db.foreign_keys_off().unwrap();
        foreign_proposals_operations(db);
    }

    #[test]
    fn foreign_proposals_rocksdb() {
        let db = create_rocksdb();
        foreign_proposals_operations(db);
    }

    fn foreign_proposals_operations(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();

        let network = Default::default();
        let shard_group = ShardGroup::all_shards(NumPreshards::P4);
        let epoch = Epoch::zero();

        let block = Block::zero_block(network, NumPreshards::P64);
        tx.blocks_insert(&block).unwrap();

        let justify_qc = QuorumCertificate::genesis(epoch, shard_group);

        // foreign_proposals_upsert
        let proposal_1 = ForeignProposal {
            block: block.clone(),
            block_pledge: BlockPledge::new(),
            justify_qc,
            proposed_by_block: None,
            status: ForeignProposalStatus::New,
        };
        tx.foreign_proposals_upsert(&proposal_1, None).unwrap();

        // foreign_proposals_get_any
        let res = tx.foreign_proposals_get_any(vec![block.id()]).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq_debug(&res[0], &proposal_1);

        // foreign_proposals_exists
        let res = tx.foreign_proposals_exists(block.id()).unwrap();
        assert!(res);
        let res = tx.foreign_proposals_exists(&create_random_block_id()).unwrap();
        assert!(!res);

        // foreign_proposals_get_all_new
        // TODO: uncomment and test when "locked_block" functionality is implemented
        // let res = tx.foreign_proposals_get_all_new(block.id(), 10).unwrap();
        // assert_eq!(res.len(), 1);
        // assert_eq_debug(&res[0], &proposal_1);

        // foreign_proposal_get_all_pending
        // TODO: uncomment and test when "get_block_ids_with_commands_between" is implemented
        // let res = tx.foreign_proposal_get_all_pending(block.id(), block.id()).unwrap();
        // assert_eq!(res.len(), 1);

        // foreign_proposals_has_unconfirmed
        let res = tx.foreign_proposals_has_unconfirmed(epoch).unwrap();
        assert!(res);
        let res = tx.foreign_proposals_has_unconfirmed(Epoch(1)).unwrap();
        assert!(!res);

        // foreign_proposals_set_status
        let updated_status = ForeignProposalStatus::Confirmed;
        tx.foreign_proposals_set_status(block.id(), updated_status).unwrap();
        let res = tx.foreign_proposals_get_any(vec![block.id()]).unwrap();
        let confirmed_proposal = res[0].clone();
        assert_eq!(confirmed_proposal.status, updated_status);

        // foreign_proposals_delete
        tx.foreign_proposals_delete(block.id()).unwrap();
        let res = tx.foreign_proposals_exists(block.id()).unwrap();
        assert!(!res);

        tx.rollback().unwrap();
    }
}
