//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::VersionedSubstateIdRef;
use tari_ootle_storage::consensus_models::SubstateChange;

/// The changes a block's store holds over the chain the block extends.
///
/// A substate version the block did not change is read from that chain, where it never changes: each version of a
/// substate is created once on a chain, and a committed or proposed block's changes are fixed. A version the block
/// changed may still change while the block is built, since a block rewrites a validator fee pool in place.
pub trait BlockChanges {
    /// The change the block made at the version `id` names, if it made one.
    fn block_change(&self, id: VersionedSubstateIdRef<'_>) -> Option<&SubstateChange>;
}
