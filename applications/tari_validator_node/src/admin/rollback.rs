//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Rollback orchestrator.
//!
//! Composes the break-glass rollback primitives (state tree truncate, substate rewind,
//! epoch-indexed deletions, applied-directive persistence) with the consensus on-hold
//! control plane to execute a governance-signed `RollbackToEpochCheckpoint` directive
//! atomically.
//!
//! ## Flow
//!
//! 1. Verify the directive signature against the configured governance public key.
//! 2. Idempotency check: if the directive ID is already in `applied_directives`, return the stored outcome without
//!    mutating state.
//! 3. Load the `EpochCheckpoint` for `target_epoch + local_shard_group`.
//! 4. Request `ConsensusCurrentState::OnHold`. The hotstuff worker unwinds its run loop cleanly; no consensus mutations
//!    are in flight while we hold.
//! 5. Atomic write transaction: a. Truncate the state tree for each shard in `shard_group ∪ {global}` to the version
//!    recorded in the checkpoint. b. Rewind substates for each shard to the same version — inverting every state
//!    transition with version > target restores fee claims and any other balance-bearing substate to its pre-rollback
//!    value. c. Delete every epoch-indexed record with `epoch > target_epoch` (blocks, QCs, checkpoints, foreign
//!    proposals, validator stats) and clear the singleton bookkeeping pointers. d. Persist an `AppliedDirective` record
//!    so a replay of the same directive ID is a no-op.
//! 6. Release the on-hold. The state machine transitions OnHold → Idle → CheckSync → Running. The hotstuff
//!    `create_genesis_block_if_required` path recognises that no block exists at `(current_epoch, height=0)` and
//!    creates a fresh genesis using the truncated state merkle root, repopulating bookkeeping pointers from it.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use log::*;
use tari_consensus_types::{ConsensusDirective, DirectiveKind};
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_epoch_manager::{EpochManagerError, EpochManagerReader, service::EpochManagerHandle};
use tari_ootle_common_types::{Epoch, optional::Optional, shard::Shard};
use tari_ootle_p2p::PeerAddress;
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
    consensus_models::{AppliedDirective, EpochCheckpoint},
};

use crate::consensus::{ConsensusHandle, ConsensusHandleError, spec::ValidatorNodeStateStore};

const LOG_TARGET: &str = "tari::validator_node::admin::rollback";

/// Timeout waiting for the consensus state machine to enter / leave OnHold.
const ON_HOLD_STATE_TIMEOUT: Duration = Duration::from_secs(30);

/// Successful outcome of applying a rollback directive. The `already_applied` flag lets
/// callers distinguish a fresh apply from an idempotent replay.
#[derive(Debug, Clone)]
pub struct RollbackOutcome {
    pub already_applied: bool,
    pub target_epoch: Epoch,
    pub applied_at_epoch: Epoch,
    pub applied_at_block_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RollbackError {
    #[error("Governance public key is not configured")]
    NoGovernanceKey,
    #[error("Directive signature verification failed: {0}")]
    InvalidDirective(#[from] tari_consensus_types::DirectiveError),
    #[error("Unsupported directive kind")]
    UnsupportedDirectiveKind,
    #[error("No epoch checkpoint found for target epoch {0}")]
    NoCheckpointForEpoch(Epoch),
    #[error("Consensus hold failed: {0}")]
    ConsensusHold(#[from] ConsensusHandleError),
    #[error("Epoch manager error: {0}")]
    EpochManager(#[from] EpochManagerError),
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("State tree root mismatch after rollback for shard {shard}")]
    StateRootMismatch { shard: Shard },
}

/// Apply a signed rollback directive. `directive` must already be decoded; signature
/// verification happens inside this function against `gov_pk`.
pub async fn apply_rollback_directive(
    directive: ConsensusDirective,
    state_store: ValidatorNodeStateStore,
    consensus_handle: ConsensusHandle,
    epoch_manager: EpochManagerHandle<PeerAddress>,
    gov_pk: &RistrettoPublicKey,
) -> Result<RollbackOutcome, RollbackError> {
    directive.verify(gov_pk)?;
    let directive_id = directive.id();

    let target_epoch = match directive.body().kind {
        DirectiveKind::RollbackToEpochCheckpoint { target_epoch } => Epoch(target_epoch),
    };

    // Idempotency short-circuit.
    let existing = state_store.with_read_tx(|tx| tx.applied_directive_get(&directive_id).optional())?;
    if let Some(record) = existing {
        info!(
            target: LOG_TARGET,
            "Directive {} already applied at epoch {}; skipping",
            directive_id,
            record.applied_at_epoch,
        );
        return Ok(RollbackOutcome {
            already_applied: true,
            target_epoch: record.body.kind.target_epoch().unwrap_or(record.applied_at_epoch),
            applied_at_epoch: record.applied_at_epoch,
            applied_at_block_id: Some(hex::encode(record.applied_at_block_id.as_bytes())),
        });
    }

    // Load the target checkpoint. Must exist locally — we do not fetch from peers as part
    // of a rollback; if the operator is asking to roll back to a checkpoint we don't have,
    // something is very wrong and we should refuse rather than guess.
    let committee_info = epoch_manager.get_local_committee_info(target_epoch).await?;
    let shard_group = committee_info.shard_group();

    let checkpoint: EpochCheckpoint = state_store
        .with_read_tx(|tx| EpochCheckpoint::get_by_shard_group(tx, target_epoch, shard_group).optional())?
        .ok_or(RollbackError::NoCheckpointForEpoch(target_epoch))?;

    // Basic shape check. Full QC validation was done when the checkpoint was stored; we're
    // trusting the local copy here, not accepting it from a peer.
    checkpoint
        .validate_well_formed()
        .map_err(|e| RollbackError::Storage(StorageError::QueryError { reason: e.to_string() }))?;

    info!(
        target: LOG_TARGET,
        "Applying rollback directive {} to epoch {} (shard group {})",
        directive_id,
        target_epoch,
        shard_group,
    );

    // 1. Request on-hold. This guarantees the hotstuff worker has unwound its run loop before we touch storage.
    consensus_handle.request_on_hold(ON_HOLD_STATE_TIMEOUT).await?;

    // 2. Single atomic write tx for all storage mutations.
    let leaf_block_id_hex_before: Option<String> = state_store
        .with_read_tx(|tx| tx.leaf_block_get(target_epoch).optional())?
        .map(|lb| hex::encode(lb.block_id.as_bytes()));

    let current_epoch = consensus_handle.current_epoch();
    let applied_at_unix_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let rollback_result = state_store.with_write_tx(|tx| {
        // a. Truncate state tree to the checkpoint's per-shard versions.
        for shard_iter in shard_group.shard_iter().chain(std::iter::once(Shard::global())) {
            let version = checkpoint.get_shard_state_version(shard_iter);
            tx.state_tree_truncate_to_version(shard_iter, version)?;
            // b. Rewind substates for the same shard.
            tx.substates_rewind_to_state_version(shard_iter, version)?;
        }

        // c. Delete all epoch-indexed records with epoch > target + clear bookkeeping.
        tx.rollback_delete_after_epoch(target_epoch)?;

        // d. Record that this directive has been applied.
        let applied = AppliedDirective {
            directive_id,
            body: directive.body().clone(),
            applied_at_epoch: current_epoch,
            applied_at_block_id: tari_consensus_types::BlockId::zero(),
            applied_at_unix_secs,
        };
        tx.applied_directive_save(&applied)?;

        Ok::<_, StorageError>(())
    });
    let () = rollback_result?;

    info!(
        target: LOG_TARGET,
        "Storage mutations committed for directive {}; previous leaf block was {}",
        directive_id,
        leaf_block_id_hex_before.as_deref().unwrap_or("<none>"),
    );

    // 3. Release on-hold. State machine will transition through CheckSync and either reach Running directly or (if the
    //    rollback left us behind L1) Syncing. Either way hotstuff.start() goes through create_genesis_block_if_required
    //    which recreates bookkeeping for the current L1 epoch using the truncated state tree.
    consensus_handle.release_on_hold(ON_HOLD_STATE_TIMEOUT).await?;

    Ok(RollbackOutcome {
        already_applied: false,
        target_epoch,
        applied_at_epoch: current_epoch,
        applied_at_block_id: None,
    })
}
