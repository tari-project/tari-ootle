//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus_types::BlockId;
use tari_ootle_storage::consensus_models::SubstateChange;

use crate::{
    codecs::{BlockIdCodec, DefaultCodec, KeyPrefix},
    column_families::block::BlockCf,
    prefixed,
    traits::Cf,
};

prefixed!(BlockDiffRecordPrefix, KeyPrefix::BlockDiffRecords);

/// A block's substate changes in the order it made them.
pub struct BlockDiffRecordCf;

impl Cf for BlockDiffRecordCf {
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
    type Prefix = BlockDiffRecordPrefix;
    type Value = Vec<SubstateChange>;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        BlockCf::name()
    }
}

/// The per-change tables block diffs were stored in up to schema version 1. Only the migration to version 2 reads
/// them, moving each block's changes into [`BlockDiffRecordCf`].
pub mod legacy {
    use tari_consensus_types::BlockId;
    use tari_engine_types::substate::SubstateId;
    use tari_ootle_common_types::SubstateVersion;
    use tari_ootle_storage::consensus_models::SubstateChange;

    use crate::{
        codecs::{
            BlockDiffKeyCodec,
            BlockIdSeqSubstateIdVersion,
            DefaultCodec,
            KeyPrefix,
            SubstateIdBlockIdVersionSeq,
            UnitCodec,
        },
        column_families::block::BlockCf,
        prefixed,
        traits::Cf,
    };

    pub struct BlockDiffKey {
        pub block_id: BlockId,
        pub substate_id: SubstateId,
        pub version: SubstateVersion,
        pub is_up: bool,
        /// Retains the ordering of the substate changes in the block. This limits the maximum number of substate
        /// changes in a block to u32::MAX (4,294,967,295).
        pub sequence: u32,
    }

    prefixed!(BlockDiffPrefix, KeyPrefix::BlockDiffs);

    /// Ordered substate transitions for a block.
    /// Schema: (BlockId, Seq, SubstateId, Version, IsUp) -> Vec<SubstateChange>
    pub struct BlockDiffCf;

    impl Cf for BlockDiffCf {
        type Key = BlockDiffKey;
        type KeyCodec = BlockDiffKeyCodec<BlockIdSeqSubstateIdVersion>;
        type Prefix = BlockDiffPrefix;
        type Value = SubstateChange;
        type ValueCodec = DefaultCodec<Self::Value>;

        fn name() -> &'static str {
            BlockCf::name()
        }
    }

    prefixed!(SubstateIdBlockDiffPrefix, KeyPrefix::BlockDiffsBySubstateId);

    /// Query for block diffs by substate id. This is a secondary index that allows querying by substate id.
    /// Schema: (SubstateId, BlockId, Version, Seq) -> ()
    pub struct SubstateIdIndex;

    impl Cf for SubstateIdIndex {
        type Key = BlockDiffKey;
        type KeyCodec = BlockDiffKeyCodec<SubstateIdBlockIdVersionSeq>;
        type Prefix = SubstateIdBlockDiffPrefix;
        type Value = ();
        type ValueCodec = UnitCodec;

        fn name() -> &'static str {
            BlockCf::name()
        }
    }
}
